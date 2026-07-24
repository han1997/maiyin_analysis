use super::{compressed_json, i64_from_u64, i64_from_usize, json, normalize, sql_error};
use crate::error::AppError;
use crate::model::{PersonAnalysis, Record};
use rayon::prelude::*;
use rusqlite::{params_from_iter, ToSql, Transaction};
pub(crate) const SAVE_PREPARE_CHUNK_SIZE: usize = 4_096;

pub(crate) const BULK_INSERT_VARIABLE_LIMIT: usize = 900;

pub(crate) struct PreparedRecord<'a> {
    pub(crate) record: &'a Record,
    pub(crate) uid: i64,
    pub(crate) check_in: Option<String>,
    pub(crate) record_json: Vec<u8>,
    pub(crate) name_norm: String,
    pub(crate) id_no_norm: String,
    pub(crate) phone_norm: String,
    pub(crate) hotel_name_norm: String,
    pub(crate) hotel_province_norm: String,
    pub(crate) hotel_city_norm: String,
    pub(crate) hotel_county_norm: String,
    pub(crate) household_region_norm: String,
    pub(crate) household_province_norm: String,
    pub(crate) household_city_norm: String,
    pub(crate) household_county_norm: String,
    pub(crate) age: Option<i64>,
    pub(crate) search_text: String,
}

pub(crate) struct PreparedPerson<'a> {
    pub(crate) analysis: &'a PersonAnalysis,
    pub(crate) summary_json: Vec<u8>,
    pub(crate) name_norm: String,
    pub(crate) id_no_norm: String,
    pub(crate) phone_norm: String,
    pub(crate) household_region_norm: String,
    pub(crate) household_province_norm: String,
    pub(crate) household_city_norm: String,
    pub(crate) household_county_norm: String,
    pub(crate) age: Option<i64>,
    pub(crate) alert_count: i64,
    pub(crate) total_records: i64,
    pub(crate) score: i64,
    pub(crate) search_text: String,
    pub(crate) alert_json: Vec<String>,
    pub(crate) hotel_names_norm: Vec<String>,
    pub(crate) hotel_regions_norm: Vec<[String; 4]>,
}

pub(crate) fn prepare_record_chunk(
    records: &[Record],
) -> Result<Vec<PreparedRecord<'_>>, AppError> {
    records
        .par_iter()
        .map(|record| {
            let age = record
                .age
                .map(|value| value.to_string())
                .unwrap_or_default();
            let search_text = normalize(
                &[
                    record.name.as_str(),
                    record.id_no.as_str(),
                    record.phone.as_str(),
                    record.hotel_name.as_str(),
                    record.region.as_str(),
                    record.household_region.as_str(),
                    record.gender.as_str(),
                    age.as_str(),
                ]
                .join(" "),
            );
            Ok(PreparedRecord {
                record,
                uid: i64_from_u64(record.uid),
                check_in: record
                    .check_in
                    .map(|value| value.format("%Y-%m-%d %H:%M:%S").to_string()),
                record_json: compressed_json(record)?,
                name_norm: normalize(&record.name),
                id_no_norm: normalize(&record.id_no),
                phone_norm: normalize(&record.phone),
                hotel_name_norm: normalize(&record.hotel_name),
                hotel_province_norm: normalize(&record.province),
                hotel_city_norm: normalize(&record.city),
                hotel_county_norm: normalize(&record.county),
                household_region_norm: normalize(&record.household_region),
                household_province_norm: normalize(&record.household_province),
                household_city_norm: normalize(&record.household_city),
                household_county_norm: normalize(&record.household_county),
                age: record.age.map(i64::from),
                search_text,
            })
        })
        .collect::<Vec<Result<_, AppError>>>()
        .into_iter()
        .collect()
}

pub(crate) fn prepare_person_chunk(
    analyses: &[PersonAnalysis],
) -> Result<Vec<PreparedPerson<'_>>, AppError> {
    analyses
        .par_iter()
        .map(|analysis| {
            let summary = &analysis.summary;
            let age = summary
                .age
                .map(|value| value.to_string())
                .unwrap_or_default();
            let alert_titles = summary.alert_titles.join(" ");
            let search_text = normalize(
                &[
                    summary.name.as_str(),
                    summary.id_no.as_str(),
                    summary.phone.as_str(),
                    summary.household_region.as_str(),
                    summary.gender.as_str(),
                    summary.level.as_str(),
                    age.as_str(),
                    alert_titles.as_str(),
                ]
                .join(" "),
            );
            Ok(PreparedPerson {
                analysis,
                summary_json: compressed_json(summary)?,
                name_norm: normalize(&summary.name),
                id_no_norm: normalize(&summary.id_no),
                phone_norm: normalize(&summary.phone),
                household_region_norm: normalize(&summary.household_region),
                household_province_norm: normalize(&summary.household_province),
                household_city_norm: normalize(&summary.household_city),
                household_county_norm: normalize(&summary.household_county),
                age: summary.age.map(i64::from),
                alert_count: i64_from_usize(summary.alert_count),
                total_records: i64_from_usize(summary.total_records),
                score: i64::from(summary.score),
                search_text,
                alert_json: analysis
                    .alerts
                    .iter()
                    .map(json)
                    .collect::<Result<Vec<_>, _>>()?,
                hotel_names_norm: summary
                    .hotel_names
                    .iter()
                    .map(|value| normalize(value))
                    .collect(),
                hotel_regions_norm: summary
                    .hotel_regions
                    .iter()
                    .map(|region| {
                        [
                            normalize(&region.province),
                            normalize(&region.city),
                            normalize(&region.county),
                            normalize(&region.region),
                        ]
                    })
                    .collect(),
            })
        })
        .collect::<Vec<Result<_, AppError>>>()
        .into_iter()
        .collect()
}

pub(crate) fn insert_analysis_rows(
    transaction: &Transaction<'_>,
    session_id: &str,
    analyses: &[PersonAnalysis],
) -> Result<(), AppError> {
    std::thread::scope(|scope| -> Result<(), AppError> {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let producer = scope.spawn(move || {
            for chunk in analyses.chunks(SAVE_PREPARE_CHUNK_SIZE) {
                let prepared = prepare_person_chunk(chunk);
                let stop = prepared.is_err();
                if sender.send(prepared).is_err() || stop {
                    break;
                }
            }
        });
        let consumer_result = (|| -> Result<(), AppError> {
            for prepared_people in receiver {
                let prepared_people = prepared_people?;
                insert_person_batches(transaction, session_id, &prepared_people)?;
                insert_alert_batches(transaction, session_id, &prepared_people)?;
                insert_person_hotel_batches(transaction, session_id, &prepared_people)?;
                insert_person_hotel_region_batches(transaction, session_id, &prepared_people)?;
            }
            Ok(())
        })();
        producer
            .join()
            .map_err(|_| AppError::Storage("people preparation worker panicked".into()))?;
        consumer_result
    })
}

pub(crate) fn insert_people_search_index(
    transaction: &Transaction<'_>,
    session_id: &str,
) -> Result<(), AppError> {
    // `people.rowid` is the only valid FTS rowid; person_key is session-local and
    // cannot safely stand in for it.
    transaction
        .execute(
            "INSERT INTO people_search_fts_v2(rowid, search_text) \
             SELECT rowid, search_text FROM people \
             WHERE session_id = ?1",
            [session_id],
        )
        .map_err(sql_error)?;
    Ok(())
}

pub(crate) fn insert_record_batches(
    transaction: &Transaction<'_>,
    session_id: &str,
    records: &[PreparedRecord<'_>],
) -> Result<(), AppError> {
    const COLUMN_COUNT: usize = 19;
    let max_rows = BULK_INSERT_VARIABLE_LIMIT / COLUMN_COUNT;
    for rows in records.chunks(max_rows) {
        let sql = multi_row_insert_sql(
            "INSERT INTO records(\
             session_id, uid, person_key, check_in, record_json, name_norm, id_no_norm, \
             phone_norm, hotel_name_norm, hotel_province_norm, hotel_city_norm, \
             hotel_county_norm, household_region_norm, household_province_norm, \
             household_city_norm, household_county_norm, age, gender, search_text) VALUES ",
            "(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            rows.len(),
        );
        let mut values = Vec::<&dyn ToSql>::with_capacity(rows.len() * COLUMN_COUNT);
        for prepared in rows {
            values.push(&session_id);
            values.push(&prepared.uid);
            values.push(&prepared.record.person_key);
            values.push(&prepared.check_in);
            values.push(&prepared.record_json);
            values.push(&prepared.name_norm);
            values.push(&prepared.id_no_norm);
            values.push(&prepared.phone_norm);
            values.push(&prepared.hotel_name_norm);
            values.push(&prepared.hotel_province_norm);
            values.push(&prepared.hotel_city_norm);
            values.push(&prepared.hotel_county_norm);
            values.push(&prepared.household_region_norm);
            values.push(&prepared.household_province_norm);
            values.push(&prepared.household_city_norm);
            values.push(&prepared.household_county_norm);
            values.push(&prepared.age);
            values.push(&prepared.record.gender);
            values.push(&prepared.search_text);
        }
        transaction
            .prepare_cached(&sql)
            .map_err(sql_error)?
            .execute(params_from_iter(values))
            .map_err(sql_error)?;
    }
    Ok(())
}

pub(crate) fn insert_person_batches(
    transaction: &Transaction<'_>,
    session_id: &str,
    people: &[PreparedPerson<'_>],
) -> Result<(), AppError> {
    const COLUMN_COUNT: usize = 18;
    let max_rows = BULK_INSERT_VARIABLE_LIMIT / COLUMN_COUNT;
    for rows in people.chunks(max_rows) {
        let sql = multi_row_insert_sql(
            "INSERT INTO people(\
             session_id, person_key, name, name_norm, id_no_norm, phone_norm, \
             household_region_norm, household_province_norm, household_city_norm, \
             household_county_norm, age, gender, level, alert_count, total_records, score, \
             search_text, summary_json) VALUES ",
            "(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            rows.len(),
        );
        let mut values = Vec::<&dyn ToSql>::with_capacity(rows.len() * COLUMN_COUNT);
        for prepared in rows {
            let summary = &prepared.analysis.summary;
            values.push(&session_id);
            values.push(&summary.person_key);
            values.push(&summary.name);
            values.push(&prepared.name_norm);
            values.push(&prepared.id_no_norm);
            values.push(&prepared.phone_norm);
            values.push(&prepared.household_region_norm);
            values.push(&prepared.household_province_norm);
            values.push(&prepared.household_city_norm);
            values.push(&prepared.household_county_norm);
            values.push(&prepared.age);
            values.push(&summary.gender);
            values.push(&summary.level);
            values.push(&prepared.alert_count);
            values.push(&prepared.total_records);
            values.push(&prepared.score);
            values.push(&prepared.search_text);
            values.push(&prepared.summary_json);
        }
        transaction
            .prepare_cached(&sql)
            .map_err(sql_error)?
            .execute(params_from_iter(values))
            .map_err(sql_error)?;
    }
    Ok(())
}

pub(crate) fn insert_alert_batches(
    transaction: &Transaction<'_>,
    session_id: &str,
    people: &[PreparedPerson<'_>],
) -> Result<(), AppError> {
    const COLUMN_COUNT: usize = 4;
    let max_rows = BULK_INSERT_VARIABLE_LIMIT / COLUMN_COUNT;
    let mut rows = Vec::with_capacity(max_rows);
    for prepared in people {
        let person_key = prepared.analysis.summary.person_key.as_str();
        for (index, alert_json) in prepared.alert_json.iter().enumerate() {
            rows.push((person_key, i64_from_usize(index), alert_json.as_str()));
            if rows.len() == max_rows {
                execute_alert_batch(transaction, session_id, &rows)?;
                rows.clear();
            }
        }
    }
    if !rows.is_empty() {
        execute_alert_batch(transaction, session_id, &rows)?;
    }
    Ok(())
}

pub(crate) fn execute_alert_batch(
    transaction: &Transaction<'_>,
    session_id: &str,
    rows: &[(&str, i64, &str)],
) -> Result<(), AppError> {
    let sql = multi_row_insert_sql(
        "INSERT INTO alerts(session_id, person_key, alert_index, alert_json) VALUES ",
        "(?, ?, ?, ?)",
        rows.len(),
    );
    let mut values = Vec::<&dyn ToSql>::with_capacity(rows.len() * 4);
    for row in rows {
        values.push(&session_id);
        values.push(&row.0);
        values.push(&row.1);
        values.push(&row.2);
    }
    transaction
        .prepare_cached(&sql)
        .map_err(sql_error)?
        .execute(params_from_iter(values))
        .map_err(sql_error)?;
    Ok(())
}

pub(crate) fn insert_person_hotel_batches(
    transaction: &Transaction<'_>,
    session_id: &str,
    people: &[PreparedPerson<'_>],
) -> Result<(), AppError> {
    const COLUMN_COUNT: usize = 3;
    let max_rows = BULK_INSERT_VARIABLE_LIMIT / COLUMN_COUNT;
    let mut rows = Vec::with_capacity(max_rows);

    for prepared in people {
        let person_key = prepared.analysis.summary.person_key.as_str();
        for hotel_name in &prepared.hotel_names_norm {
            rows.push((person_key, hotel_name.as_str()));
            if rows.len() == max_rows {
                execute_person_hotel_batch(transaction, session_id, &rows)?;
                rows.clear();
            }
        }
    }
    if !rows.is_empty() {
        execute_person_hotel_batch(transaction, session_id, &rows)?;
    }
    Ok(())
}

pub(crate) fn execute_person_hotel_batch(
    transaction: &Transaction<'_>,
    session_id: &str,
    rows: &[(&str, &str)],
) -> Result<(), AppError> {
    let sql = multi_row_insert_sql(
        "INSERT OR IGNORE INTO person_hotels(\
         session_id, person_key, hotel_name_norm) VALUES ",
        "(?, ?, ?)",
        rows.len(),
    );
    let mut values = Vec::<&dyn ToSql>::with_capacity(rows.len() * 3);
    for (person_key, hotel_name) in rows {
        values.push(&session_id);
        values.push(person_key);
        values.push(hotel_name);
    }
    transaction
        .prepare_cached(&sql)
        .map_err(sql_error)?
        .execute(params_from_iter(values))
        .map_err(sql_error)?;
    Ok(())
}

pub(crate) fn insert_person_hotel_region_batches(
    transaction: &Transaction<'_>,
    session_id: &str,
    people: &[PreparedPerson<'_>],
) -> Result<(), AppError> {
    const COLUMN_COUNT: usize = 6;
    let max_rows = BULK_INSERT_VARIABLE_LIMIT / COLUMN_COUNT;
    let mut rows = Vec::with_capacity(max_rows);

    for prepared in people {
        let person_key = prepared.analysis.summary.person_key.as_str();
        for region in &prepared.hotel_regions_norm {
            rows.push((
                person_key,
                region[0].as_str(),
                region[1].as_str(),
                region[2].as_str(),
                region[3].as_str(),
            ));
            if rows.len() == max_rows {
                execute_person_hotel_region_batch(transaction, session_id, &rows)?;
                rows.clear();
            }
        }
    }
    if !rows.is_empty() {
        execute_person_hotel_region_batch(transaction, session_id, &rows)?;
    }
    Ok(())
}

pub(crate) fn execute_person_hotel_region_batch(
    transaction: &Transaction<'_>,
    session_id: &str,
    rows: &[(&str, &str, &str, &str, &str)],
) -> Result<(), AppError> {
    let sql = multi_row_insert_sql(
        "INSERT OR IGNORE INTO person_hotel_regions(\
         session_id, person_key, province_norm, city_norm, county_norm, region_norm) VALUES ",
        "(?, ?, ?, ?, ?, ?)",
        rows.len(),
    );
    let mut values = Vec::<&dyn ToSql>::with_capacity(rows.len() * 6);
    for (person_key, province, city, county, region) in rows {
        values.push(&session_id);
        values.push(person_key);
        values.push(province);
        values.push(city);
        values.push(county);
        values.push(region);
    }
    transaction
        .prepare_cached(&sql)
        .map_err(sql_error)?
        .execute(params_from_iter(values))
        .map_err(sql_error)?;
    Ok(())
}

pub(crate) fn multi_row_insert_sql(prefix: &str, value_group: &str, row_count: usize) -> String {
    debug_assert!(row_count > 0);
    let mut sql = String::with_capacity(prefix.len() + row_count * (value_group.len() + 2));
    sql.push_str(prefix);
    for index in 0..row_count {
        if index > 0 {
            sql.push_str(", ");
        }
        sql.push_str(value_group);
    }
    sql
}
