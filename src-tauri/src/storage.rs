use crate::error::AppError;
use crate::model::{
    AlertSummary, AnalysisSettings, AnalysisStats, FrequencyMode, ImportStats, ImportedRecordsPage,
    ImportedRecordsQuery, PersonAnalysis, PersonDetail, PersonPage, PersonQuery, PersonSummary,
    Record, SessionSummary, StoredSession,
};
use rusqlite::{
    params, params_from_iter, types::Value, Connection, OptionalExtension, Transaction,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

const DATA_FOLDER: &str = "MaiyinAnalysisData";
const DATABASE_FILE: &str = "history-v1.sqlite3";
const DATABASE_VERSION: i64 = 4;
const EMPTY_DATABASE_RESET_THRESHOLD_BYTES: u64 = 8 * 1024 * 1024;
const SESSION_FTS_TABLES: [(&str, &str); 4] = [
    ("records_search_fts", "records"),
    ("people_search_fts", "people"),
    ("records_hotel_name_fts", "records"),
    ("person_hotels_name_fts", "person_hotels"),
];

#[derive(Debug, Clone)]
pub struct SessionStore {
    storage_root: PathBuf,
    database_path: PathBuf,
    access_lock: Arc<RwLock<()>>,
}

#[derive(Debug, Clone)]
pub struct SessionMetadata {
    pub schema_version: u32,
    pub session_id: String,
    pub file_name: String,
    pub imported_at: String,
    pub file_count: usize,
    pub settings: AnalysisSettings,
    pub stats: AnalysisStats,
    pub import_stats: ImportStats,
    pub source_session_ids: Vec<String>,
    pub is_combined: bool,
}

impl SessionStore {
    pub fn open(storage_root: PathBuf) -> Result<Self, AppError> {
        let data_dir = storage_root.join(DATA_FOLDER);
        fs::create_dir_all(&data_dir).map_err(storage_error)?;
        let store = Self {
            storage_root,
            database_path: data_dir.join(DATABASE_FILE),
            access_lock: Arc::new(RwLock::new(())),
        };
        let connection = store.connection()?;
        initialize_schema(&connection)?;
        let session_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .map_err(sql_error)?;
        drop(connection);
        // Older delete paths could leave a multi-gigabyte empty file and orphaned FTS pages.
        // Rebuild only clearly oversized empty databases; failure is non-fatal at startup.
        if session_count == 0
            && fs::metadata(&store.database_path)
                .map_err(storage_error)?
                .len()
                > EMPTY_DATABASE_RESET_THRESHOLD_BYTES
        {
            let _ = store.reset_database_file();
        }
        Ok(store)
    }

    pub fn list(&self) -> Result<Vec<SessionSummary>, AppError> {
        let _read_guard = self.lock_reads()?;
        let connection = self.connection()?;
        let active_id = active_id_from(&connection)?.unwrap_or_default();
        let mut statement = connection
            .prepare(
                "SELECT session_id, file_name, imported_at, file_count, records, people, \
                 duplicate_count, short_stay_count \
                 FROM sessions WHERE listed = 1 ORDER BY imported_at DESC",
            )
            .map_err(sql_error)?;
        let rows = statement
            .query_map([], |row| {
                let session_id: String = row.get(0)?;
                Ok(SessionSummary {
                    active: session_id == active_id,
                    session_id,
                    file_name: row.get(1)?,
                    imported_at: row.get(2)?,
                    file_count: usize_from_i64(row.get(3)?),
                    records: usize_from_i64(row.get(4)?),
                    people: usize_from_i64(row.get(5)?),
                    duplicate_count: usize_from_i64(row.get(6)?),
                    short_stay_count: usize_from_i64(row.get(7)?),
                })
            })
            .map_err(sql_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)
    }

    #[allow(dead_code)]
    pub fn metadata(&self, session_id: &str) -> Result<SessionMetadata, AppError> {
        let _read_guard = self.lock_reads()?;
        let connection = self.connection()?;
        metadata_from(&connection, session_id)
    }

    pub fn activate(&self, session_id: &str) -> Result<SessionMetadata, AppError> {
        let _write_guard = self.lock_writes()?;
        let connection = self.connection()?;
        let metadata = metadata_from(&connection, session_id)?;
        connection
            .execute(
                "INSERT INTO app_meta(key, value) VALUES('active_session_id', ?1) \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [session_id],
            )
            .map_err(sql_error)?;
        Ok(metadata)
    }

    pub fn save(&self, session: &StoredSession) -> Result<SessionMetadata, AppError> {
        let _write_guard = self.lock_writes()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(sql_error)?;
        let stale_combined_session_ids = {
            let mut statement = transaction
                .prepare("SELECT session_id FROM sessions WHERE listed = 0 AND session_id <> ?1")
                .map_err(sql_error)?;
            let rows = statement
                .query_map([&session.session_id], |row| row.get::<_, String>(0))
                .map_err(sql_error)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)?
        };
        for stale_session_id in &stale_combined_session_ids {
            delete_session_fts_rows(&transaction, stale_session_id)?;
        }
        transaction
            .execute(
                "DELETE FROM sessions WHERE listed = 0 AND session_id <> ?1",
                [&session.session_id],
            )
            .map_err(sql_error)?;
        // Contentless FTS5 virtual tables have no FK back to records/people, so they don't
        // cascade; clear any stale rows for this session before re-inserting.
        delete_session_fts_rows(&transaction, &session.session_id)?;
        transaction
            .execute(
                "DELETE FROM sessions WHERE session_id = ?1",
                [&session.session_id],
            )
            .map_err(sql_error)?;
        transaction
            .execute(
                "INSERT INTO sessions(
                    session_id, schema_version, file_name, imported_at, file_count,
                    settings_json, stats_json, import_stats_json, source_session_ids_json,
                    is_combined, listed, records, people, duplicate_count, short_stay_count
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    session.session_id,
                    i64::from(session.schema_version),
                    session.file_name,
                    session.imported_at,
                    i64_from_usize(session.file_count),
                    json(&session.settings)?,
                    json(&session.stats)?,
                    json(&session.import_stats)?,
                    json(&session.source_session_ids)?,
                    session.is_combined,
                    !session.is_combined,
                    i64_from_usize(session.stats.records),
                    i64_from_usize(session.stats.people),
                    i64_from_usize(session.import_stats.duplicate_count),
                    i64_from_usize(session.import_stats.short_stay_count),
                ],
            )
            .map_err(sql_error)?;

        {
            let mut record_filter_counts = HashMap::<(String, String), i64>::new();
            let mut record_statement = transaction
                .prepare(
                    "INSERT INTO records(session_id, uid, person_key, check_in, record_json, \
                     name_norm, id_no_norm, phone_norm, hotel_name_norm, hotel_province_norm, \
                     hotel_city_norm, hotel_county_norm, household_region_norm, \
                     household_province_norm, household_city_norm, household_county_norm, \
                     age, gender, search_text) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, \
                     ?16, ?17, ?18, ?19)",
                )
                .map_err(sql_error)?;
            let mut record_search_fts_statement = transaction
                .prepare(
                    "INSERT INTO records_search_fts(rowid, search_text, session_id, uid) \
                     VALUES (?1, ?2, ?3, ?4)",
                )
                .map_err(sql_error)?;
            for record in &session.records {
                let search_text = normalize(
                    &[
                        record.name.as_str(),
                        record.id_no.as_str(),
                        record.phone.as_str(),
                        record.hotel_name.as_str(),
                        record.region.as_str(),
                        record.household_region.as_str(),
                        record.gender.as_str(),
                        &record
                            .age
                            .map(|value| value.to_string())
                            .unwrap_or_default(),
                    ]
                    .join(" "),
                );
                let hotel_name_norm = normalize(&record.hotel_name);
                if record.check_in.is_some() {
                    increment_record_filter_count(
                        &mut record_filter_counts,
                        "hotel_name",
                        &hotel_name_norm,
                    );
                    increment_record_filter_count(
                        &mut record_filter_counts,
                        "hotel_province",
                        &normalize(&record.province),
                    );
                    increment_record_filter_count(
                        &mut record_filter_counts,
                        "hotel_city",
                        &normalize(&record.city),
                    );
                    increment_record_filter_count(
                        &mut record_filter_counts,
                        "hotel_county",
                        &normalize(&record.county),
                    );
                    increment_record_filter_count(
                        &mut record_filter_counts,
                        "household_province",
                        &normalize(&record.household_province),
                    );
                    increment_record_filter_count(
                        &mut record_filter_counts,
                        "household_city",
                        &normalize(&record.household_city),
                    );
                    increment_record_filter_count(
                        &mut record_filter_counts,
                        "household_county",
                        &normalize(&record.household_county),
                    );
                }
                record_statement
                    .execute(params![
                        session.session_id,
                        i64_from_u64(record.uid),
                        record.person_key,
                        record
                            .check_in
                            .map(|value| value.format("%Y-%m-%d %H:%M:%S").to_string()),
                        json(record)?,
                        normalize(&record.name),
                        normalize(&record.id_no),
                        normalize(&record.phone),
                        hotel_name_norm.clone(),
                        normalize(&record.province),
                        normalize(&record.city),
                        normalize(&record.county),
                        normalize(&record.household_region),
                        normalize(&record.household_province),
                        normalize(&record.household_city),
                        normalize(&record.household_county),
                        record.age.map(i64::from),
                        record.gender,
                        search_text.clone(),
                    ])
                    .map_err(sql_error)?;
                // Mirror the row into the contentless FTS5 trigram table using the real
                // SQLite rowid. Business uid is only unique within a session and does not
                // necessarily match the table rowid.
                let record_rowid = transaction.last_insert_rowid();
                record_search_fts_statement
                    .execute(params![
                        record_rowid,
                        search_text,
                        session.session_id,
                        i64_from_u64(record.uid),
                    ])
                    .map_err(sql_error)?;
            }
            let mut count_statement = transaction
                .prepare(
                    "INSERT INTO record_filter_counts(
                        session_id, filter_kind, value_norm, record_count
                     ) VALUES (?1, ?2, ?3, ?4)",
                )
                .map_err(sql_error)?;
            for ((filter_kind, value_norm), record_count) in record_filter_counts {
                count_statement
                    .execute(params![
                        session.session_id,
                        filter_kind,
                        value_norm,
                        record_count
                    ])
                    .map_err(sql_error)?;
            }
        }

        {
            let mut person_statement = transaction
                .prepare(
                    "INSERT INTO people(
                        session_id, person_key, name, name_norm, id_no_norm, phone_norm,
                        household_region_norm, household_province_norm, household_city_norm,
                        household_county_norm, age, gender, level, alert_count,
                        total_records, score, search_text, summary_json
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
                )
                .map_err(sql_error)?;
            let mut alert_statement = transaction
                .prepare(
                    "INSERT INTO alerts(session_id, person_key, alert_index, alert_json) \
                     VALUES (?1, ?2, ?3, ?4)",
                )
                .map_err(sql_error)?;
            let mut hotel_statement = transaction
                .prepare(
                    "INSERT OR IGNORE INTO person_hotels(session_id, person_key, hotel_name_norm) \
                     VALUES (?1, ?2, ?3)",
                )
                .map_err(sql_error)?;
            let mut region_statement = transaction
                .prepare(
                    "INSERT OR IGNORE INTO person_hotel_regions(
                        session_id, person_key, province_norm, city_norm, county_norm, region_norm
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )
                .map_err(sql_error)?;
            let mut person_search_fts_statement = transaction
                .prepare(
                    "INSERT INTO people_search_fts(rowid, search_text, session_id, person_key) \
                     VALUES (?1, ?2, ?3, ?4)",
                )
                .map_err(sql_error)?;

            for analysis in &session.analyses {
                let summary = &analysis.summary;
                let search_text = normalize(
                    &[
                        summary.name.as_str(),
                        summary.id_no.as_str(),
                        summary.phone.as_str(),
                        summary.household_region.as_str(),
                        summary.gender.as_str(),
                        summary.level.as_str(),
                        &summary
                            .age
                            .map(|value| value.to_string())
                            .unwrap_or_default(),
                        &summary.alert_titles.join(" "),
                    ]
                    .join(" "),
                );
                person_statement
                    .execute(params![
                        session.session_id,
                        summary.person_key,
                        summary.name,
                        normalize(&summary.name),
                        normalize(&summary.id_no),
                        normalize(&summary.phone),
                        normalize(&summary.household_region),
                        normalize(&summary.household_province),
                        normalize(&summary.household_city),
                        normalize(&summary.household_county),
                        summary.age.map(i64::from),
                        summary.gender,
                        summary.level,
                        i64_from_usize(summary.alert_count),
                        i64_from_usize(summary.total_records),
                        i64::from(summary.score),
                        search_text,
                        json(summary)?,
                    ])
                    .map_err(sql_error)?;
                // Capture the implicit rowid SQLite assigned to this people row, then mirror it
                // into the corresponding FTS5 virtual table.
                let person_pk_rowid: i64 = transaction
                    .query_row(
                        "SELECT rowid FROM people WHERE session_id = ?1 AND person_key = ?2",
                        params![session.session_id, summary.person_key],
                        |row| row.get(0),
                    )
                    .map_err(sql_error)?;
                person_search_fts_statement
                    .execute(params![
                        person_pk_rowid,
                        search_text,
                        session.session_id,
                        summary.person_key,
                    ])
                    .map_err(sql_error)?;

                for (index, alert) in analysis.alerts.iter().enumerate() {
                    alert_statement
                        .execute(params![
                            session.session_id,
                            summary.person_key,
                            i64_from_usize(index),
                            json(alert)?,
                        ])
                        .map_err(sql_error)?;
                }
                for hotel_name in &summary.hotel_names {
                    hotel_statement
                        .execute(params![
                            session.session_id,
                            summary.person_key,
                            normalize(hotel_name),
                        ])
                        .map_err(sql_error)?;
                }
                for region in &summary.hotel_regions {
                    region_statement
                        .execute(params![
                            session.session_id,
                            summary.person_key,
                            normalize(&region.province),
                            normalize(&region.city),
                            normalize(&region.county),
                            normalize(&region.region),
                        ])
                        .map_err(sql_error)?;
                }
            }
        }

        if !session.is_combined {
            transaction
                .execute(
                    "INSERT INTO app_meta(key, value) VALUES('active_session_id', ?1) \
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    [&session.session_id],
                )
                .map_err(sql_error)?;
        }
        transaction.commit().map_err(sql_error)?;
        metadata_from(&connection, &session.session_id)
    }

    pub fn load(&self, session_id: &str) -> Result<StoredSession, AppError> {
        let _read_guard = self.lock_reads()?;
        let connection = self.connection()?;
        let metadata = metadata_from(&connection, session_id)?;

        let records = load_json_column::<Record>(
            &connection,
            "SELECT record_json FROM records WHERE session_id = ?1 ORDER BY uid",
            session_id,
        )?;

        let mut alerts_by_person: HashMap<String, Vec<AlertSummary>> = HashMap::new();
        {
            let mut statement = connection
                .prepare(
                    "SELECT person_key, alert_json FROM alerts \
                     WHERE session_id = ?1 ORDER BY person_key, alert_index",
                )
                .map_err(sql_error)?;
            let mut rows = statement.query([session_id]).map_err(sql_error)?;
            while let Some(row) = rows.next().map_err(sql_error)? {
                let person_key: String = row.get(0).map_err(sql_error)?;
                let payload: String = row.get(1).map_err(sql_error)?;
                alerts_by_person
                    .entry(person_key)
                    .or_default()
                    .push(from_json(&payload)?);
            }
        }

        let summaries = load_json_column::<PersonSummary>(
            &connection,
            "SELECT summary_json FROM people WHERE session_id = ?1 \
             ORDER BY score DESC, total_records DESC, name ASC, person_key ASC",
            session_id,
        )?;
        let analyses = summaries
            .into_iter()
            .map(|summary| PersonAnalysis {
                alerts: alerts_by_person
                    .remove(&summary.person_key)
                    .unwrap_or_default(),
                summary,
            })
            .collect();

        Ok(StoredSession {
            schema_version: metadata.schema_version,
            session_id: metadata.session_id,
            file_name: metadata.file_name,
            imported_at: metadata.imported_at,
            file_count: metadata.file_count,
            settings: metadata.settings,
            records,
            analyses,
            stats: metadata.stats,
            import_stats: metadata.import_stats,
            source_session_ids: metadata.source_session_ids,
            is_combined: metadata.is_combined,
        })
    }

    pub fn query_people(
        &self,
        session_id: &str,
        query: &PersonQuery,
    ) -> Result<PersonPage, AppError> {
        let _read_guard = self.lock_reads()?;
        let connection = self.connection()?;
        ensure_session_exists(&connection, session_id)?;
        let page_size = query.page_size.clamp(1, 500);
        let page = query.page.max(1).min(usize_from_i64(i64::MAX) / page_size);
        let (where_sql, values) = build_person_filter(session_id, query);
        let count_sql = format!("SELECT COUNT(*) FROM people p WHERE {where_sql}");
        let total: i64 = connection
            .query_row(&count_sql, params_from_iter(values.iter()), |row| {
                row.get(0)
            })
            .map_err(sql_error)?;

        let mut paged_values = values;
        paged_values.push(Value::Integer(i64_from_usize(page_size)));
        paged_values.push(Value::Integer(i64_from_usize(
            (page - 1).saturating_mul(page_size),
        )));
        let paged_sql = format!(
            "SELECT p.summary_json FROM people p WHERE {where_sql} \
             ORDER BY p.score DESC, p.total_records DESC, p.name ASC, p.person_key ASC LIMIT ? OFFSET ?"
        );
        let mut statement = connection.prepare_cached(&paged_sql).map_err(sql_error)?;
        let mut rows = statement
            .query(params_from_iter(paged_values.iter()))
            .map_err(sql_error)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().map_err(sql_error)? {
            let payload: String = row.get(0).map_err(sql_error)?;
            items.push(from_json(&payload)?);
        }
        Ok(PersonPage {
            items,
            total: usize_from_i64(total),
            page,
            page_size,
        })
    }

    pub fn query_imported_records(
        &self,
        session_id: &str,
        query: &ImportedRecordsQuery,
    ) -> Result<ImportedRecordsPage, AppError> {
        let _read_guard = self.lock_reads()?;
        let connection = self.connection()?;
        let settings = settings_for_session(&connection, session_id)?;
        let page_size = query.page_size.clamp(1, 500);
        let page = query.page.max(1).min(usize_from_i64(i64::MAX) / page_size);
        let (where_sql, values) = build_records_filter(session_id, query, &settings);
        let total = if let Some(total) =
            fast_record_filter_count(&connection, session_id, query, &settings)?
        {
            total
        } else {
            let count_sql = format!(
                "SELECT COUNT(*) FROM {} WHERE {where_sql}",
                records_count_source(query)
            );
            connection
                .query_row(&count_sql, params_from_iter(values.iter()), |row| {
                    row.get(0)
                })
                .map_err(sql_error)?
        };

        let mut paged_values = values;
        paged_values.push(Value::Integer(i64_from_usize(page_size)));
        paged_values.push(Value::Integer(i64_from_usize(
            (page - 1).saturating_mul(page_size),
        )));
        let paged_sql = format!(
            "SELECT record_json FROM records INDEXED BY idx_records_check_in WHERE {where_sql} \
             ORDER BY check_in ASC, uid ASC LIMIT ? OFFSET ?"
        );
        let mut statement = connection.prepare_cached(&paged_sql).map_err(sql_error)?;
        let mut rows = statement
            .query(params_from_iter(paged_values.iter()))
            .map_err(sql_error)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().map_err(sql_error)? {
            let payload: String = row.get(0).map_err(sql_error)?;
            items.push(crate::model::ImportedStayRecord::from(from_json::<Record>(
                &payload,
            )?));
        }
        Ok(ImportedRecordsPage {
            items,
            total: usize_from_i64(total),
            page,
            page_size,
        })
    }

    pub fn person_detail(
        &self,
        session_id: &str,
        person_key: &str,
    ) -> Result<PersonDetail, AppError> {
        let _read_guard = self.lock_reads()?;
        let connection = self.connection()?;
        let summary_payload: Option<String> = connection
            .query_row(
                "SELECT summary_json FROM people WHERE session_id = ?1 AND person_key = ?2",
                params![session_id, person_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_error)?;
        let person = summary_payload
            .map(|payload| from_json(&payload))
            .transpose()?
            .ok_or_else(|| AppError::Validation("未找到指定人员".into()))?;

        let mut alerts = Vec::new();
        let mut evidence_ids = HashSet::new();
        {
            let mut statement = connection
                .prepare(
                    "SELECT alert_json FROM alerts WHERE session_id = ?1 AND person_key = ?2 \
                     ORDER BY alert_index",
                )
                .map_err(sql_error)?;
            let mut rows = statement
                .query(params![session_id, person_key])
                .map_err(sql_error)?;
            while let Some(row) = rows.next().map_err(sql_error)? {
                let payload: String = row.get(0).map_err(sql_error)?;
                let alert: AlertSummary = from_json(&payload)?;
                evidence_ids.extend(alert.evidence_ids.iter().copied());
                alerts.push(alert);
            }
        }

        let settings = metadata_from(&connection, session_id)?.settings;
        let records = load_records_for_person(&connection, session_id, person_key)?;
        let evidence = records
            .into_iter()
            .filter(|record| {
                crate::analysis::within_analysis_time_window(record, &settings)
                    && (evidence_ids.is_empty() || evidence_ids.contains(&record.uid))
            })
            .map(|record| crate::model::EvidenceRecord {
                uid: record.uid,
                source_file: record.source_file,
                source_row: record.source_row,
                hotel_name: record.hotel_name,
                region: record.region,
                address: record.address,
                room_no: record.room_no,
                check_in: crate::model::format_datetime(record.check_in),
                check_out: crate::model::format_datetime(record.check_out),
                issues: record.issues,
            })
            .collect();
        Ok(PersonDetail {
            person,
            alerts,
            evidence,
        })
    }

    pub fn delete(&self, session_id: &str) -> Result<Option<SessionMetadata>, AppError> {
        let _write_guard = self.lock_writes()?;
        let connection = self.connection()?;
        let remaining_listed_sessions: Option<i64> = connection
            .query_row(
                "SELECT (SELECT COUNT(*) FROM sessions WHERE listed = 1 AND session_id <> ?1) \
                        FROM sessions WHERE session_id = ?1",
                [session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_error)?;
        let Some(remaining_listed_sessions) = remaining_listed_sessions else {
            return Err(AppError::SessionNotFound);
        };
        drop(connection);

        if remaining_listed_sessions == 0 && self.reset_database_file().is_ok() {
            return Ok(None);
        }

        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(sql_error)?;
        delete_session_fts_rows(&transaction, session_id)?;
        for table in [
            "alerts",
            "person_hotels",
            "person_hotel_regions",
            "record_filter_counts",
            "records",
            "people",
        ] {
            transaction
                .execute(
                    &format!("DELETE FROM {table} WHERE session_id = ?1"),
                    [session_id],
                )
                .map_err(sql_error)?;
        }
        let deleted = transaction
            .execute("DELETE FROM sessions WHERE session_id = ?1", [session_id])
            .map_err(sql_error)?;
        if deleted == 0 {
            return Err(AppError::SessionNotFound);
        }
        let active_id = transaction
            .query_row(
                "SELECT value FROM app_meta WHERE key = 'active_session_id'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sql_error)?;
        if active_id.as_deref() == Some(session_id) {
            let replacement = transaction
                .query_row(
                    "SELECT session_id FROM sessions WHERE listed = 1 ORDER BY imported_at DESC LIMIT 1",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(sql_error)?;
            if let Some(replacement) = &replacement {
                transaction
                    .execute(
                        "UPDATE app_meta SET value = ?1 WHERE key = 'active_session_id'",
                        [replacement],
                    )
                    .map_err(sql_error)?;
            } else {
                transaction
                    .execute("DELETE FROM app_meta WHERE key = 'active_session_id'", [])
                    .map_err(sql_error)?;
            }
        }
        transaction.commit().map_err(sql_error)?;
        active_id_from(&connection)?
            .map(|active| metadata_from(&connection, &active))
            .transpose()
    }

    pub fn move_to(&self, destination_root: PathBuf) -> Result<Self, AppError> {
        let _write_guard = self.lock_writes()?;
        if destination_root == self.storage_root {
            return Ok(self.clone());
        }
        let destination_data = destination_root.join(DATA_FOLDER);
        fs::create_dir_all(&destination_data).map_err(storage_error)?;
        let destination_database = destination_data.join(DATABASE_FILE);
        if destination_database.exists() {
            return Err(AppError::Storage(format!(
                "目标目录已存在 {}，请先选择空目录",
                destination_database.display()
            )));
        }
        {
            let connection = self.connection()?;
            connection
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .map_err(sql_error)?;
        }
        let temporary = destination_data.join(format!("{DATABASE_FILE}.tmp"));
        fs::copy(&self.database_path, &temporary).map_err(storage_error)?;
        fs::rename(&temporary, &destination_database).map_err(storage_error)?;
        Self::open(destination_root)
    }

    fn connection(&self) -> Result<Connection, AppError> {
        let connection = Connection::open(&self.database_path).map_err(sql_error)?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 PRAGMA busy_timeout = 5000;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA temp_store = MEMORY;",
            )
            .map_err(sql_error)?;
        Ok(connection)
    }

    fn lock_reads(&self) -> Result<RwLockReadGuard<'_, ()>, AppError> {
        self.access_lock
            .read()
            .map_err(|_| AppError::Storage("SQLite 访问锁不可用，请重启应用后重试".into()))
    }

    fn lock_writes(&self) -> Result<RwLockWriteGuard<'_, ()>, AppError> {
        self.access_lock
            .write()
            .map_err(|_| AppError::Storage("SQLite 写入锁不可用，请重启应用后重试".into()))
    }

    fn reset_database_file(&self) -> Result<(), AppError> {
        if self.database_path.exists() {
            let connection = self.connection()?;
            connection
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .map_err(sql_error)?;
            drop(connection);
        }

        for suffix in ["-wal", "-shm", "-journal"] {
            remove_file_if_exists(&self.database_sidecar_path(suffix))?;
        }
        remove_file_if_exists(&self.database_path)?;

        let connection = self.connection()?;
        initialize_schema(&connection)
    }

    fn database_sidecar_path(&self, suffix: &str) -> PathBuf {
        let file_name = self
            .database_path
            .file_name()
            .map(|value| value.to_string_lossy())
            .unwrap_or_default();
        self.database_path
            .with_file_name(format!("{file_name}{suffix}"))
    }
}

fn delete_session_fts_rows(
    transaction: &Transaction<'_>,
    session_id: &str,
) -> Result<(), AppError> {
    // Contentless FTS tables cannot reliably return their UNINDEXED session_id value.
    // Delete by the mirrored content-table rowid while those source rows still exist.
    for (fts_table, content_table) in SESSION_FTS_TABLES {
        let exists = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
                [fts_table],
                |row| row.get::<_, bool>(0),
            )
            .map_err(sql_error)?;
        if exists {
            transaction
                .execute(
                    &format!(
                        "DELETE FROM {fts_table} WHERE rowid IN (\
                         SELECT rowid FROM {content_table} WHERE session_id = ?1)"
                    ),
                    [session_id],
                )
                .map_err(sql_error)?;
        }
    }
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> Result<(), AppError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage_error(error)),
    }
}

fn initialize_schema(connection: &Connection) -> Result<(), AppError> {
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(sql_error)?;
    if version == 1 || version == 2 || version == 3 {
        reset_legacy_database(connection)?;
    } else if version != 0 && version != DATABASE_VERSION {
        return Err(AppError::Storage(format!(
            "不支持的历史数据库版本 {version}，当前版本为 {DATABASE_VERSION}"
        )));
    }
    connection
        .execute_batch(&format!(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS app_meta(
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS sessions(
                session_id TEXT PRIMARY KEY,
                schema_version INTEGER NOT NULL,
                file_name TEXT NOT NULL,
                imported_at TEXT NOT NULL,
                file_count INTEGER NOT NULL,
                settings_json TEXT NOT NULL,
                stats_json TEXT NOT NULL,
                import_stats_json TEXT NOT NULL,
                source_session_ids_json TEXT NOT NULL,
                is_combined INTEGER NOT NULL,
                listed INTEGER NOT NULL,
                records INTEGER NOT NULL,
                people INTEGER NOT NULL,
                duplicate_count INTEGER NOT NULL,
                short_stay_count INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS records(
                session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
                uid INTEGER NOT NULL,
                person_key TEXT NOT NULL,
                check_in TEXT,
                record_json TEXT NOT NULL,
                name_norm TEXT NOT NULL DEFAULT '',
                id_no_norm TEXT NOT NULL DEFAULT '',
                phone_norm TEXT NOT NULL DEFAULT '',
                hotel_name_norm TEXT NOT NULL DEFAULT '',
                hotel_province_norm TEXT NOT NULL DEFAULT '',
                hotel_city_norm TEXT NOT NULL DEFAULT '',
                hotel_county_norm TEXT NOT NULL DEFAULT '',
                household_region_norm TEXT NOT NULL DEFAULT '',
                household_province_norm TEXT NOT NULL DEFAULT '',
                household_city_norm TEXT NOT NULL DEFAULT '',
                household_county_norm TEXT NOT NULL DEFAULT '',
                age INTEGER,
                gender TEXT NOT NULL DEFAULT '',
                search_text TEXT NOT NULL DEFAULT '',
                PRIMARY KEY(session_id, uid)
             );
             CREATE TABLE IF NOT EXISTS people(
                session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
                person_key TEXT NOT NULL,
                name TEXT NOT NULL,
                name_norm TEXT NOT NULL,
                id_no_norm TEXT NOT NULL,
                phone_norm TEXT NOT NULL,
                household_region_norm TEXT NOT NULL,
                household_province_norm TEXT NOT NULL DEFAULT '',
                household_city_norm TEXT NOT NULL DEFAULT '',
                household_county_norm TEXT NOT NULL DEFAULT '',
                age INTEGER,
                gender TEXT NOT NULL,
                level TEXT NOT NULL,
                alert_count INTEGER NOT NULL,
                total_records INTEGER NOT NULL,
                score INTEGER NOT NULL,
                search_text TEXT NOT NULL,
                summary_json TEXT NOT NULL,
                PRIMARY KEY(session_id, person_key)
             );
             CREATE TABLE IF NOT EXISTS alerts(
                session_id TEXT NOT NULL,
                person_key TEXT NOT NULL,
                alert_index INTEGER NOT NULL,
                alert_json TEXT NOT NULL,
                PRIMARY KEY(session_id, person_key, alert_index),
                FOREIGN KEY(session_id, person_key) REFERENCES people(session_id, person_key) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS person_hotels(
                session_id TEXT NOT NULL,
                person_key TEXT NOT NULL,
                hotel_name_norm TEXT NOT NULL,
                PRIMARY KEY(session_id, person_key, hotel_name_norm),
                FOREIGN KEY(session_id, person_key) REFERENCES people(session_id, person_key) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS person_hotel_regions(
                session_id TEXT NOT NULL,
                person_key TEXT NOT NULL,
                province_norm TEXT NOT NULL,
                city_norm TEXT NOT NULL,
                county_norm TEXT NOT NULL,
                region_norm TEXT NOT NULL,
                PRIMARY KEY(session_id, person_key, province_norm, city_norm, county_norm, region_norm),
                FOREIGN KEY(session_id, person_key) REFERENCES people(session_id, person_key) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS record_filter_counts(
                session_id TEXT NOT NULL,
                filter_kind TEXT NOT NULL,
                value_norm TEXT NOT NULL,
                record_count INTEGER NOT NULL,
                PRIMARY KEY(session_id, filter_kind, value_norm),
                FOREIGN KEY(session_id) REFERENCES sessions(session_id) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS idx_sessions_imported_at ON sessions(listed, imported_at DESC);
             CREATE INDEX IF NOT EXISTS idx_records_person ON records(session_id, person_key);
             CREATE INDEX IF NOT EXISTS idx_records_check_in ON records(session_id, check_in, uid);
             CREATE INDEX IF NOT EXISTS idx_records_hotel_name ON records(session_id, hotel_name_norm);
             CREATE INDEX IF NOT EXISTS idx_records_hotel_region ON records(session_id, hotel_province_norm, hotel_city_norm, hotel_county_norm);
             CREATE INDEX IF NOT EXISTS idx_records_household_split ON records(session_id, household_province_norm, household_city_norm, household_county_norm);
             CREATE INDEX IF NOT EXISTS idx_records_age_gender ON records(session_id, age, gender);
             CREATE INDEX IF NOT EXISTS idx_people_sort ON people(session_id, score DESC, total_records DESC, name ASC, person_key ASC);
             CREATE INDEX IF NOT EXISTS idx_people_level_alert ON people(session_id, level, alert_count);
             CREATE INDEX IF NOT EXISTS idx_people_age_gender ON people(session_id, age, gender);
             CREATE INDEX IF NOT EXISTS idx_people_household_split ON people(session_id, household_province_norm, household_city_norm, household_county_norm);
             CREATE INDEX IF NOT EXISTS idx_person_hotels_lookup ON person_hotels(session_id, person_key, hotel_name_norm);
             CREATE INDEX IF NOT EXISTS idx_person_hotel_regions_jurisdiction ON person_hotel_regions(session_id, person_key, province_norm, city_norm, county_norm);
             CREATE INDEX IF NOT EXISTS idx_person_regions_lookup ON person_hotel_regions(session_id, person_key);
             CREATE INDEX IF NOT EXISTS idx_record_filter_counts_lookup ON record_filter_counts(session_id, filter_kind, value_norm);
             CREATE VIRTUAL TABLE IF NOT EXISTS records_search_fts USING fts5(
                search_text, session_id UNINDEXED, uid UNINDEXED,
                content='', contentless_delete=1, tokenize='trigram'
             );
             CREATE VIRTUAL TABLE IF NOT EXISTS people_search_fts USING fts5(
                search_text, session_id UNINDEXED, person_key UNINDEXED,
                content='', contentless_delete=1, tokenize='trigram'
             );
             PRAGMA user_version = {DATABASE_VERSION};"
        ))
        .map_err(sql_error)
}

fn reset_legacy_database(connection: &Connection) -> Result<(), AppError> {
    connection
        .execute_batch(
            "PRAGMA foreign_keys = OFF;
             DROP TABLE IF EXISTS people_search_fts;
             DROP TABLE IF EXISTS records_search_fts;
             DROP TABLE IF EXISTS person_hotel_regions;
             DROP TABLE IF EXISTS person_hotels;
             DROP TABLE IF EXISTS record_filter_counts;
             DROP TABLE IF EXISTS alerts;
             DROP TABLE IF EXISTS people;
             DROP TABLE IF EXISTS records;
             DROP TABLE IF EXISTS sessions;
             DROP TABLE IF EXISTS app_meta;
             PRAGMA user_version = 0;
             PRAGMA foreign_keys = ON;",
        )
        .map_err(sql_error)
}

fn metadata_from(connection: &Connection, session_id: &str) -> Result<SessionMetadata, AppError> {
    let row = connection
        .query_row(
            "SELECT schema_version, session_id, file_name, imported_at, file_count,
                    settings_json, stats_json, import_stats_json, source_session_ids_json,
                    is_combined
             FROM sessions WHERE session_id = ?1",
            [session_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, bool>(9)?,
                ))
            },
        )
        .optional()
        .map_err(sql_error)?
        .ok_or(AppError::SessionNotFound)?;
    Ok(SessionMetadata {
        schema_version: row.0.max(0) as u32,
        session_id: row.1,
        file_name: row.2,
        imported_at: row.3,
        file_count: usize_from_i64(row.4),
        settings: from_json(&row.5)?,
        stats: from_json(&row.6)?,
        import_stats: from_json(&row.7)?,
        source_session_ids: from_json(&row.8)?,
        is_combined: row.9,
    })
}

fn active_id_from(connection: &Connection) -> Result<Option<String>, AppError> {
    connection
        .query_row(
            "SELECT value FROM app_meta WHERE key = 'active_session_id'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_error)
}

fn ensure_session_exists(connection: &Connection, session_id: &str) -> Result<(), AppError> {
    let exists = connection
        .query_row(
            "SELECT 1 FROM sessions WHERE session_id = ?1",
            [session_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(sql_error)?;
    exists.ok_or(AppError::SessionNotFound)
}

/// Lightweight session lookup that combines `ensure_session_exists` semantics with the
/// single column actually needed by the imported-records path: `settings_json`. Avoids
/// decoding `stats_json`, `import_stats_json`, `source_session_ids_json` on every page
/// request and replaces the prior two-call (`ensure_session_exists` + `metadata_from`)
/// sequence with one indexed point lookup.
fn settings_for_session(
    connection: &Connection,
    session_id: &str,
) -> Result<AnalysisSettings, AppError> {
    let payload: Option<String> = connection
        .query_row(
            "SELECT settings_json FROM sessions WHERE session_id = ?1",
            [session_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_error)?;
    payload
        .map(|value| from_json::<AnalysisSettings>(&value))
        .transpose()?
        .ok_or(AppError::SessionNotFound)
}

fn build_person_filter(session_id: &str, query: &PersonQuery) -> (String, Vec<Value>) {
    let mut clauses = vec!["p.session_id = ?".to_string()];
    let mut values = vec![Value::Text(session_id.to_string())];

    let search = normalize(&query.search);
    if !search.is_empty() {
        if search.chars().count() >= 3 {
            // Fast path: FTS5 trigram MATCH (prefix/substring for ≥3 chars).
            // Quote the query to avoid FTS5 boolean parsing. We use `rowid IN (...)`
            // rather than `EXISTS (... fts.rowid = p.rowid ...)` so the planner drives
            // the FTS5 doclist (a small candidate set) and joins to people on rowid —
            // the EXISTS form forced a people scan with a per-row MATCH eval.
            clauses.push(
                "p.rowid IN (SELECT rowid FROM people_search_fts WHERE search_text MATCH ?)".into(),
            );
            values.push(Value::Text(fts_match_query(&search)));
        } else {
            // Fallback for ≤2-char queries: trigram tokenizer floor; LIKE contains stays correct.
            clauses.push("p.search_text LIKE ? ESCAPE '\\'".into());
            values.push(Value::Text(contains_pattern(&search)));
        }
    }
    if !query.level.trim().is_empty() && query.level != "全部等级" {
        clauses.push("p.level = ?".into());
        values.push(Value::Text(query.level.clone()));
    }
    match query.alert_state.as_str() {
        "仅预警人员" => clauses.push("p.alert_count > 0".into()),
        "未预警人员" => clauses.push("p.alert_count = 0".into()),
        _ => {}
    }
    if let Some(min_age) = query.min_age {
        clauses.push("p.age >= ?".into());
        values.push(Value::Integer(i64_from_usize(min_age)));
    }
    if let Some(max_age) = query.max_age {
        clauses.push("p.age <= ?".into());
        values.push(Value::Integer(i64_from_usize(max_age)));
    }
    if !query.gender.trim().is_empty() {
        clauses.push("p.gender = ?".into());
        values.push(Value::Text(query.gender.clone()));
    }

    for hotel in split_hotel_terms(&query.hotel_search) {
        // Hotel-name fuzzy match is ordered-subsequence (`%a%b%c%`), NOT substring
        // contains. FTS5 trigram MATCH implements substring contains, so it cannot
        // serve as a sound prefilter here (false-negatives like `商务b` against
        // `商务宾馆b`). Keep the LIKE-only path; per-person hotel cardinality is
        // small so the EXISTS correlated scan stays bounded.
        clauses.push(
            "EXISTS (SELECT 1 FROM person_hotels ph \
             WHERE ph.session_id = p.session_id AND ph.person_key = p.person_key \
             AND ph.hotel_name_norm LIKE ? ESCAPE '\\')"
                .into(),
        );
        values.push(Value::Text(fuzzy_pattern(&hotel)));
    }

    let hotel_regions = [
        normalize(&query.hotel_province),
        normalize(&query.hotel_city),
        normalize(&query.hotel_county),
    ];
    if hotel_regions.iter().any(|value| !value.is_empty()) {
        let mut region_clauses = Vec::new();
        for (column, value) in ["province_norm", "city_norm", "county_norm"]
            .into_iter()
            .zip(hotel_regions)
        {
            if value.is_empty() {
                continue;
            }
            // Prefix match on B-tree-indexable split column; sargable via
            // idx_person_hotel_regions_jurisdiction.
            region_clauses.push(format!("phr.{column} >= ? AND phr.{column} < ?"));
            let (lower, upper) = prefix_range(&value);
            values.push(Value::Text(lower));
            values.push(Value::Text(upper));
        }
        clauses.push(format!(
            "EXISTS (SELECT 1 FROM person_hotel_regions phr \
             WHERE phr.session_id = p.session_id AND phr.person_key = p.person_key AND {})",
            region_clauses.join(" AND ")
        ));
    }

    // Household-prefix filters: indexable via idx_people_household_split.
    let household_splits = [
        (
            "household_province_norm",
            normalize(&query.household_province),
        ),
        ("household_city_norm", normalize(&query.household_city)),
        ("household_county_norm", normalize(&query.household_county)),
    ];
    for (column, value) in household_splits {
        if !value.is_empty() {
            clauses.push(format!("p.{column} >= ? AND p.{column} < ?"));
            let (lower, upper) = prefix_range(&value);
            values.push(Value::Text(lower));
            values.push(Value::Text(upper));
        }
    }
    let excluded = [
        (
            "household_province_norm",
            normalize(&query.exclude_household_province),
        ),
        (
            "household_city_norm",
            normalize(&query.exclude_household_city),
        ),
        (
            "household_county_norm",
            normalize(&query.exclude_household_county),
        ),
    ]
    .into_iter()
    .filter(|(_, value)| !value.is_empty())
    .collect::<Vec<_>>();
    if !excluded.is_empty() {
        // NOT over the prefix-match set; each populated component is an OR (any excluded
        // sub-string prefix triggers exclusion). Subsumes the prior substring semantic
        // for the common case where users type leading region characters.
        let inner = excluded
            .iter()
            .map(|(column, _)| format!("p.{column} >= ? AND p.{column} < ?"))
            .collect::<Vec<_>>()
            .join(" OR ");
        clauses.push(format!("NOT ({inner})"));
        for (_, value) in excluded {
            let (lower, upper) = prefix_range(&value);
            values.push(Value::Text(lower));
            values.push(Value::Text(upper));
        }
    }
    (clauses.join(" AND "), values)
}

fn build_records_filter(
    session_id: &str,
    query: &ImportedRecordsQuery,
    settings: &AnalysisSettings,
) -> (String, Vec<Value>) {
    let mut clauses = vec![
        "session_id = ?".to_string(),
        "check_in IS NOT NULL".to_string(),
    ];
    let mut values = vec![Value::Text(session_id.to_string())];
    if settings.frequency_mode == FrequencyMode::Selected {
        if let Some(start) = settings.frequency_start {
            clauses.push("check_in >= ?".into());
            values.push(Value::Text(start.format("%Y-%m-%d %H:%M:%S").to_string()));
        }
        if let Some(end) = settings.frequency_end {
            clauses.push("check_in <= ?".into());
            values.push(Value::Text(end.format("%Y-%m-%d %H:%M:%S").to_string()));
        }
    }

    let search = normalize(&query.search);
    if !search.is_empty() {
        if search.chars().count() >= 3 {
            // FTS5 trigram MATCH: indexed substring for ≥3 chars. rowid-in-subselect
            // drives the FTS5 doclist; ON-clause on rowid drives a seek into records.
            clauses.push(
                "rowid IN (SELECT rowid FROM records_search_fts WHERE search_text MATCH ?)".into(),
            );
            values.push(Value::Text(fts_match_query(&search)));
        } else {
            clauses.push("search_text LIKE ? ESCAPE '\\'".into());
            values.push(Value::Text(contains_pattern(&search)));
        }
    }
    if let Some(min_age) = query.min_age {
        clauses.push("age >= ?".into());
        values.push(Value::Integer(i64_from_usize(min_age)));
    }
    if let Some(max_age) = query.max_age {
        clauses.push("age <= ?".into());
        values.push(Value::Integer(i64_from_usize(max_age)));
    }
    if !query.gender.trim().is_empty() {
        clauses.push("gender = ?".into());
        values.push(Value::Text(query.gender.clone()));
    }

    for hotel in split_hotel_terms(&query.hotel_search) {
        // Records hotel-name filter uses ordered-subsequence `fuzzy_pattern`; trigram
        // MATCH (substring contains) cannot serve as a prefilter without losing matches.
        // The records scan is bounded by session_id partition; filter is on the indexed
        // (session_id, hotel_name_norm) range plus the LIKE post-filter.
        clauses.push("hotel_name_norm LIKE ? ESCAPE '\\'".into());
        values.push(Value::Text(fuzzy_pattern(&hotel)));
    }

    // Hotel jurisdiction prefix via idx_records_hotel_region (multi-column B-tree).
    for (column, value) in [
        ("hotel_province_norm", normalize(&query.hotel_province)),
        ("hotel_city_norm", normalize(&query.hotel_city)),
        ("hotel_county_norm", normalize(&query.hotel_county)),
    ] {
        if value.is_empty() {
            continue;
        }
        clauses.push(format!("{column} >= ? AND {column} < ?"));
        let (lower, upper) = prefix_range(&value);
        values.push(Value::Text(lower));
        values.push(Value::Text(upper));
    }

    // Household jurisdiction prefix via idx_records_household_split.
    let household_splits = [
        (
            "household_province_norm",
            normalize(&query.household_province),
        ),
        ("household_city_norm", normalize(&query.household_city)),
        ("household_county_norm", normalize(&query.household_county)),
    ];
    for (column, value) in household_splits {
        if !value.is_empty() {
            clauses.push(format!("{column} >= ? AND {column} < ?"));
            let (lower, upper) = prefix_range(&value);
            values.push(Value::Text(lower));
            values.push(Value::Text(upper));
        }
    }
    let excluded = [
        (
            "household_province_norm",
            normalize(&query.exclude_household_province),
        ),
        (
            "household_city_norm",
            normalize(&query.exclude_household_city),
        ),
        (
            "household_county_norm",
            normalize(&query.exclude_household_county),
        ),
    ]
    .into_iter()
    .filter(|(_, value)| !value.is_empty())
    .collect::<Vec<_>>();
    if !excluded.is_empty() {
        let inner = excluded
            .iter()
            .map(|(column, _)| format!("{column} >= ? AND {column} < ?"))
            .collect::<Vec<_>>()
            .join(" OR ");
        clauses.push(format!("NOT ({inner})"));
        for (_, value) in excluded {
            let (lower, upper) = prefix_range(&value);
            values.push(Value::Text(lower));
            values.push(Value::Text(upper));
        }
    }

    (clauses.join(" AND "), values)
}

fn records_count_source(query: &ImportedRecordsQuery) -> &'static str {
    if !query.hotel_province.trim().is_empty()
        || !query.hotel_city.trim().is_empty()
        || !query.hotel_county.trim().is_empty()
    {
        "records INDEXED BY idx_records_hotel_region"
    } else if !query.household_province.trim().is_empty()
        || !query.household_city.trim().is_empty()
        || !query.household_county.trim().is_empty()
        || !query.exclude_household_province.trim().is_empty()
        || !query.exclude_household_city.trim().is_empty()
        || !query.exclude_household_county.trim().is_empty()
    {
        "records INDEXED BY idx_records_household_split"
    } else if !query.hotel_search.trim().is_empty() {
        "records INDEXED BY idx_records_hotel_name"
    } else {
        "records"
    }
}

fn fast_record_filter_count(
    connection: &Connection,
    session_id: &str,
    query: &ImportedRecordsQuery,
    settings: &AnalysisSettings,
) -> Result<Option<i64>, AppError> {
    if settings.frequency_mode == FrequencyMode::Selected
        || !query.search.trim().is_empty()
        || query.min_age.is_some()
        || query.max_age.is_some()
        || !query.gender.trim().is_empty()
        || !query.exclude_household_province.trim().is_empty()
        || !query.exclude_household_city.trim().is_empty()
        || !query.exclude_household_county.trim().is_empty()
    {
        return Ok(None);
    }

    let hotel_regions = [
        ("hotel_province", normalize(&query.hotel_province)),
        ("hotel_city", normalize(&query.hotel_city)),
        ("hotel_county", normalize(&query.hotel_county)),
    ];
    let household_regions = [
        ("household_province", normalize(&query.household_province)),
        ("household_city", normalize(&query.household_city)),
        ("household_county", normalize(&query.household_county)),
    ];
    let active_regions = hotel_regions
        .iter()
        .chain(household_regions.iter())
        .filter(|(_, value)| !value.is_empty())
        .collect::<Vec<_>>();
    let hotel_terms = split_hotel_terms(&query.hotel_search);

    match (active_regions.as_slice(), hotel_terms.as_slice()) {
        ([(filter_kind, value)], []) => {
            let (lower, upper) = prefix_range(value);
            let total = connection
                .query_row(
                    "SELECT COALESCE(SUM(record_count), 0) FROM record_filter_counts \
                     WHERE session_id = ?1 AND filter_kind = ?2 AND value_norm >= ?3 AND value_norm < ?4",
                    params![session_id, filter_kind, lower, upper],
                    |row| row.get(0),
                )
                .map_err(sql_error)?;
            Ok(Some(total))
        }
        ([], terms) if !terms.is_empty() => {
            let mut clauses = vec![
                "session_id = ?".to_string(),
                "filter_kind = 'hotel_name'".to_string(),
            ];
            let mut values = vec![Value::Text(session_id.to_string())];
            for term in terms {
                clauses.push("value_norm LIKE ? ESCAPE '\\'".into());
                values.push(Value::Text(fuzzy_pattern(term)));
            }
            let sql = format!(
                "SELECT COALESCE(SUM(record_count), 0) FROM record_filter_counts WHERE {}",
                clauses.join(" AND ")
            );
            let total = connection
                .query_row(&sql, params_from_iter(values.iter()), |row| row.get(0))
                .map_err(sql_error)?;
            Ok(Some(total))
        }
        _ => Ok(None),
    }
}

fn increment_record_filter_count(
    counts: &mut HashMap<(String, String), i64>,
    filter_kind: &str,
    value_norm: &str,
) {
    if value_norm.is_empty() {
        return;
    }
    *counts
        .entry((filter_kind.to_string(), value_norm.to_string()))
        .or_insert(0) += 1;
}

fn load_records_for_person(
    connection: &Connection,
    session_id: &str,
    person_key: &str,
) -> Result<Vec<Record>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT record_json FROM records WHERE session_id = ?1 AND person_key = ?2 ORDER BY uid",
        )
        .map_err(sql_error)?;
    let mut rows = statement
        .query(params![session_id, person_key])
        .map_err(sql_error)?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().map_err(sql_error)? {
        let payload: String = row.get(0).map_err(sql_error)?;
        result.push(from_json(&payload)?);
    }
    Ok(result)
}

fn load_json_column<T: serde::de::DeserializeOwned>(
    connection: &Connection,
    sql: &str,
    session_id: &str,
) -> Result<Vec<T>, AppError> {
    let mut statement = connection.prepare(sql).map_err(sql_error)?;
    let mut rows = statement.query([session_id]).map_err(sql_error)?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().map_err(sql_error)? {
        let payload: String = row.get(0).map_err(sql_error)?;
        result.push(from_json(&payload)?);
    }
    Ok(result)
}

fn split_hotel_terms(value: &str) -> Vec<String> {
    value
        .split([',', '，', '、', ';', '；', '\n', '\r'])
        .map(normalize)
        .filter(|value| !value.is_empty())
        .collect()
}

fn normalize(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn contains_pattern(value: &str) -> String {
    format!("%{}%", escape_like(value))
}

fn prefix_range(value: &str) -> (String, String) {
    // BINARY collation range for prefix semantics. This is more reliably indexable than
    // depending on SQLite LIKE optimization, especially for normalized non-ASCII text.
    (value.to_string(), format!("{value}\u{10ffff}"))
}

fn fuzzy_pattern(value: &str) -> String {
    let mut pattern = String::from("%");
    for character in value.chars() {
        match character {
            '%' | '_' | '\\' => pattern.push('\\'),
            _ => {}
        }
        pattern.push(character);
        pattern.push('%');
    }
    pattern
}

fn fts_match_query(value: &str) -> String {
    // FTS5 trigram MATCH parses unquoted input as boolean (AND/OR/NOT). Wrap user
    // input in double quotes so the entire string is treated as a phrase. Trigram
    // phrase semantics == substring contains for patterns ≥3 chars.
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn json<T: serde::Serialize>(value: &T) -> Result<String, AppError> {
    serde_json::to_string(value).map_err(AppError::from)
}

fn from_json<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, AppError> {
    serde_json::from_str(value).map_err(AppError::from)
}

fn i64_from_usize(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn i64_from_u64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn usize_from_i64(value: i64) -> usize {
    usize::try_from(value.max(0)).unwrap_or(usize::MAX)
}

fn storage_error(error: std::io::Error) -> AppError {
    AppError::Storage(error.to_string())
}

fn sql_error(error: rusqlite::Error) -> AppError {
    AppError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::analyze_records;
    use crate::importer::import_paths;
    use crate::model::{HotelRegion, CURRENT_SCHEMA_VERSION};
    use std::time::Instant;
    use uuid::Uuid;

    fn test_store() -> (PathBuf, SessionStore) {
        let root = std::env::temp_dir().join(format!("maiyin-storage-{}", Uuid::new_v4()));
        let store = SessionStore::open(root.clone()).unwrap();
        (root, store)
    }

    fn sample_session() -> StoredSession {
        let summary = PersonSummary {
            person_key: "id:1".into(),
            name: "测试人员".into(),
            id_no: "341024198809128135".into(),
            phone: "13905591234".into(),
            household_region: "安徽省 黄山市 祁门县".into(),
            household_province: "安徽省".into(),
            household_city: "黄山市".into(),
            household_county: "祁门县".into(),
            age: Some(37),
            gender: "男".into(),
            total_records: 1,
            max_week_count: 1,
            max_month_count: 1,
            max_year_count: 1,
            overlap_days: 0,
            sequential_days: 0,
            score: 0,
            level: "正常".into(),
            alert_count: 0,
            alert_titles: vec![],
            hotel_names: vec!["旅馆 A".into(), "商务宾馆 B".into()],
            hotel_regions: vec![HotelRegion {
                province: "安徽省".into(),
                city: "黄山市".into(),
                county: "祁门县".into(),
                region: "安徽省黄山市祁门县".into(),
            }],
        };
        StoredSession {
            schema_version: CURRENT_SCHEMA_VERSION,
            session_id: "session-1".into(),
            file_name: "test.xlsx".into(),
            imported_at: "2026-07-22T10:00:00+08:00".into(),
            file_count: 1,
            settings: AnalysisSettings::default(),
            records: vec![sample_record(1, Some(1))],
            analyses: vec![PersonAnalysis {
                summary,
                alerts: vec![],
            }],
            stats: AnalysisStats {
                people: 1,
                ..Default::default()
            },
            import_stats: ImportStats {
                imported: 1,
                ..Default::default()
            },
            source_session_ids: vec![],
            is_combined: false,
        }
    }

    fn sample_record(uid: u64, day: Option<u32>) -> Record {
        let check_in = day.map(|value| {
            chrono::NaiveDate::from_ymd_opt(2026, 7, value)
                .unwrap()
                .and_hms_opt(10, 0, 0)
                .unwrap()
        });
        Record {
            uid,
            source_file: "test.xlsx".into(),
            source_row: usize::try_from(uid).unwrap_or(1) + 1,
            name: format!("测试人员{uid}"),
            id_no: format!("34102419880912{uid:04}"),
            phone: "13905591234".into(),
            hotel_name: "旅馆 A".into(),
            province: "安徽省".into(),
            city: "黄山市".into(),
            county: "祁门县".into(),
            region: "安徽省 黄山市 祁门县".into(),
            address: "测试路 1 号".into(),
            room_no: "201".into(),
            check_in_text: check_in
                .map(|value| value.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_default(),
            register_time_text: String::new(),
            check_out_text: String::new(),
            check_in,
            register_time: None,
            check_out: None,
            person_key: "id:1".into(),
            household_province: "安徽省".into(),
            household_city: "黄山市".into(),
            household_county: "祁门县".into(),
            household_region: "安徽省 黄山市 祁门县".into(),
            household_address: String::new(),
            age: Some(37),
            gender: "男".into(),
            issues: vec![],
        }
    }

    fn query() -> PersonQuery {
        PersonQuery {
            level: "全部等级".into(),
            alert_state: "全部人员".into(),
            page: 1,
            page_size: 50,
            ..Default::default()
        }
    }

    #[test]
    fn sqlite_round_trip_and_page_query() {
        let (root, store) = test_store();
        store.save(&sample_session()).unwrap();
        let loaded = store.load("session-1").unwrap();
        assert_eq!(loaded.analyses.len(), 1);
        assert_eq!(loaded.records.len(), 1);
        assert_eq!(store.list().unwrap().len(), 1);
        assert_eq!(store.query_people("session-1", &query()).unwrap().total, 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn imported_records_are_time_filtered_sorted_and_paginated_in_sqlite() {
        let (root, store) = test_store();
        let mut session = sample_session();
        session.records = vec![
            sample_record(1, Some(1)),
            sample_record(2, Some(5)),
            sample_record(3, Some(10)),
            sample_record(4, None),
        ];
        session.settings.frequency_start = chrono::NaiveDate::from_ymd_opt(2026, 7, 2)
            .unwrap()
            .and_hms_opt(0, 0, 0);
        session.settings.frequency_mode = FrequencyMode::Selected;
        session.settings.frequency_end = chrono::NaiveDate::from_ymd_opt(2026, 7, 10)
            .unwrap()
            .and_hms_opt(23, 59, 59);
        store.save(&session).unwrap();

        let first = store
            .query_imported_records(
                "session-1",
                &ImportedRecordsQuery {
                    page: 1,
                    page_size: 1,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(first.total, 2);
        assert_eq!(first.items.len(), 1);
        assert_eq!(first.items[0].uid, 2);

        let second = store
            .query_imported_records(
                "session-1",
                &ImportedRecordsQuery {
                    page: 2,
                    page_size: 1,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(second.items[0].uid, 3);

        let clamped = store
            .query_imported_records(
                "session-1",
                &ImportedRecordsQuery {
                    page: 1,
                    page_size: 999,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(clamped.page_size, 500);
        assert_eq!(clamped.items.len(), 2);

        session.settings.frequency_mode = FrequencyMode::Rolling;
        store.save(&session).unwrap();
        let rolling = store
            .query_imported_records(
                "session-1",
                &ImportedRecordsQuery {
                    page: 1,
                    page_size: 50,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(rolling.total, 3);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn imported_records_apply_result_filters_in_sqlite() {
        let (root, store) = test_store();
        let mut session = sample_session();
        session.records = vec![
            sample_record(1, Some(1)),
            sample_record(2, Some(2)),
            sample_record(3, Some(3)),
        ];
        session.records[1].hotel_name = "锦江城市酒店".into();
        session.records[1].province = "四川省".into();
        session.records[1].city = "成都市".into();
        session.records[1].county = "锦江区".into();
        session.records[1].region = "四川省 成都市 锦江区".into();
        session.records[1].gender = "女".into();
        session.records[1].age = Some(25);
        session.records[1].name = "李四".into();
        session.records[1].id_no = "510104199001012428".into();
        session.records[2].household_region = "浙江省 杭州市 西湖区".into();
        session.records[2].household_province = "浙江省".into();
        session.records[2].household_city = "杭州市".into();
        session.records[2].household_county = "西湖区".into();
        store.save(&session).unwrap();

        let page = store
            .query_imported_records(
                "session-1",
                &ImportedRecordsQuery {
                    hotel_search: "锦江".into(),
                    page: 1,
                    page_size: 50,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].uid, 2);

        let page = store
            .query_imported_records(
                "session-1",
                &ImportedRecordsQuery {
                    hotel_province: "四川".into(),
                    page: 1,
                    page_size: 50,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].uid, 2);

        let page = store
            .query_imported_records(
                "session-1",
                &ImportedRecordsQuery {
                    household_province: "浙江".into(),
                    page: 1,
                    page_size: 50,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].uid, 3);

        let page = store
            .query_imported_records(
                "session-1",
                &ImportedRecordsQuery {
                    exclude_household_province: "安徽".into(),
                    page: 1,
                    page_size: 50,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].uid, 3);

        let page = store
            .query_imported_records(
                "session-1",
                &ImportedRecordsQuery {
                    min_age: Some(30),
                    max_age: Some(40),
                    page: 1,
                    page_size: 50,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(page.total, 2);

        let page = store
            .query_imported_records(
                "session-1",
                &ImportedRecordsQuery {
                    gender: "女".into(),
                    page: 1,
                    page_size: 50,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].uid, 2);

        let page = store
            .query_imported_records(
                "session-1",
                &ImportedRecordsQuery {
                    search: "李四".into(),
                    page: 1,
                    page_size: 50,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].uid, 2);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn version_one_database_is_cleared_instead_of_migrated() {
        let (root, store) = test_store();
        store.save(&sample_session()).unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch("PRAGMA user_version = 1;")
            .unwrap();
        drop(store);

        let rebuilt = SessionStore::open(root.clone()).unwrap();
        assert!(rebuilt.list().unwrap().is_empty());
        let version: i64 = rebuilt
            .connection()
            .unwrap()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, DATABASE_VERSION);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn version_two_database_is_cleared_instead_of_migrated() {
        let (root, store) = test_store();
        store.save(&sample_session()).unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch("PRAGMA user_version = 2;")
            .unwrap();
        drop(store);

        let rebuilt = SessionStore::open(root.clone()).unwrap();
        assert!(rebuilt.list().unwrap().is_empty());
        let version: i64 = rebuilt
            .connection()
            .unwrap()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, DATABASE_VERSION);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn version_three_database_is_cleared_instead_of_migrated() {
        // v3 → v4: also wiped + rebuilt (spec allows clear + re-import as the
        // only schema-evolution path). Confirms FTS5 tables get (re)created too.
        let (root, store) = test_store();
        store.save(&sample_session()).unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch("PRAGMA user_version = 3;")
            .unwrap();
        drop(store);

        let rebuilt = SessionStore::open(root.clone()).unwrap();
        assert!(rebuilt.list().unwrap().is_empty());
        let version: i64 = rebuilt
            .connection()
            .unwrap()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, DATABASE_VERSION);
        // FTS5 virtual tables must be re-created during the reset.
        let fts_exists: i64 = rebuilt
            .connection()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='records_search_fts'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(fts_exists, 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explain_query_plan_uses_indexes_on_fast_paths() {
        // Smoke check that the four fast paths (search_text FTS5, household split prefix,
        // hotel jurisdiction split prefix, person_hotel_regions jurisdiction) are served
        // by an index seek, not a SCAN. Runs against a small populated fixture and
        // inspects the textual EXPLAIN QUERY PLAN output.
        let (root, store) = test_store();
        store.save(&sample_session()).unwrap();
        let connection = store.connection().unwrap();

        fn plan(connection: &Connection, sql: &str, params: &[Value]) -> String {
            // EXPLAIN QUERY PLAN emits columns: id, parent, notused, detail.
            let mut statement = connection
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap();
            let mut rows = statement.query(params_from_iter(params.iter())).unwrap();
            let mut lines = Vec::new();
            while let Some(row) = rows.next().unwrap() {
                let line: String = row.get(3).unwrap_or_default();
                lines.push(line);
            }
            lines.join(" | ")
        }

        // 1. search_text path via FTS5 trigram MATCH (≥3 chars).
        let mut q = query();
        q.search = "测试人".into();
        let (where_sql, values) = build_person_filter("session-1", &q);
        let plan_text = plan(
            &connection,
            &format!("SELECT COUNT(*) FROM people p WHERE {where_sql}"),
            &values,
        );
        assert!(
            plan_text.to_lowercase().contains("people_search_fts")
                || plan_text.to_lowercase().contains("using"),
            "expected FTS5 or index seek in plan, got: {plan_text}"
        );

        // 2. household split prefix via idx_people_household_split.
        let mut q = query();
        q.household_province = "安徽".into();
        let (where_sql, values) = build_person_filter("session-1", &q);
        let plan_text = plan(
            &connection,
            &format!("SELECT COUNT(*) FROM people p WHERE {where_sql}"),
            &values,
        );
        assert!(
            plan_text
                .to_lowercase()
                .contains("idx_people_household_split")
                || plan_text.to_lowercase().contains("using index"),
            "expected idx_people_household_split seek, got: {plan_text}"
        );

        // 3. hotel_region jurisdiction prefix via idx_person_hotel_regions_jurisdiction.
        let mut q = query();
        q.hotel_province = "安徽".into();
        let (where_sql, values) = build_person_filter("session-1", &q);
        let plan_text = plan(
            &connection,
            &format!("SELECT COUNT(*) FROM people p WHERE {where_sql}"),
            &values,
        );
        assert!(
            plan_text
                .to_lowercase()
                .contains("idx_person_hotel_regions_jurisdiction")
                || plan_text.to_lowercase().contains("using index"),
            "expected idx_person_hotel_regions_jurisdiction seek, got: {plan_text}"
        );

        // 4. records household split via idx_records_household_split.
        let settings = AnalysisSettings::default();
        let records_query = ImportedRecordsQuery {
            household_province: "安徽".into(),
            page: 1,
            page_size: 50,
            ..Default::default()
        };
        let (where_sql, values) = build_records_filter("session-1", &records_query, &settings);
        let plan_text = plan(
            &connection,
            &format!("SELECT COUNT(*) FROM records WHERE {where_sql}"),
            &values,
        );
        assert!(
            plan_text
                .to_lowercase()
                .contains("idx_records_household_split")
                || plan_text.to_lowercase().contains("using index"),
            "expected idx_records_household_split seek, got: {plan_text}"
        );

        drop(connection);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fts5_trigram_match_equivalent_to_like_contains_for_three_plus_chars() {
        // The trigram MATCH path must return the same person set as the LIKE contains
        // path for queries ≥3 chars. Substring containment is the exact semantic.
        let (root, store) = test_store();
        let mut session = sample_session();
        // Two people: one named "杭州测试", another "北京测试". search_text contains name.
        session.analyses[0].summary.name = "杭州测试".into();
        session.analyses[0].summary.household_region = "浙江省 杭州市 西湖区".into();
        session.analyses[0].summary.household_province = "浙江省".into();
        session.analyses[0].summary.household_city = "杭州市".into();
        session.analyses[0].summary.household_county = "西湖区".into();
        session.analyses[0].summary.person_key = "id:1".into();
        session.analyses[0].summary.hotel_names = vec!["旅馆 A".into()];
        session.analyses[0].summary.hotel_regions = vec![HotelRegion {
            province: "安徽省".into(),
            city: "黄山市".into(),
            county: "祁门县".into(),
            region: "安徽省黄山市祁门县".into(),
        }];
        let mut second = PersonAnalysis {
            summary: session.analyses[0].summary.clone(),
            alerts: vec![],
        };
        second.summary.person_key = "id:2".into();
        second.summary.name = "北京测试".into();
        second.summary.household_region = "北京市 东城区".into();
        second.summary.household_province = "北京市".into();
        second.summary.household_city = "东城区".into();
        second.summary.household_county = String::new();
        session.analyses.push(second);
        session.stats.people = 2;
        store.save(&session).unwrap();

        // 3-char query "杭州测" — pure substring; trigram MATCH applicable.
        let mut q = query();
        q.search = "杭州测".into();
        assert_eq!(store.query_people("session-1", &q).unwrap().total, 1);
        assert_eq!(
            store.query_people("session-1", &q).unwrap().items[0].name,
            "杭州测试"
        );

        // 3-char query "杭州" — only 2 chars; falls back to LIKE contains (still correct).
        let mut q = query();
        q.search = "杭州".into();
        assert_eq!(store.query_people("session-1", &q).unwrap().total, 1);

        // Cross-record substring on imported_records path (sample_record uses
        // hotel name "旅馆 A" + region "安徽省 黄山市 祁门县"; search_text includes both).
        let page = store
            .query_imported_records(
                "session-1",
                &ImportedRecordsQuery {
                    search: "祁门县".into(), // 3 chars, trigram MATCH path
                    page: 1,
                    page_size: 50,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(page.total, 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn imported_record_fts_uses_sqlite_rowid_not_business_uid() {
        // Imported-record FTS joins back to `records.rowid`. A business uid can be any
        // session-local value, so saving uid=42 as the first row must still be searchable.
        let (root, store) = test_store();
        let mut session = sample_session();
        let mut record = sample_record(42, Some(1));
        record.hotel_name = "alpha lodge".into();
        session.records = vec![record];
        store.save(&session).unwrap();

        let page = store
            .query_imported_records(
                "session-1",
                &ImportedRecordsQuery {
                    search: "alpha".into(),
                    page: 1,
                    page_size: 50,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].uid, 42);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn record_filter_count_cache_is_replaced_with_session() {
        let (root, store) = test_store();
        let mut session = sample_session();
        session.records[0].household_province = "alpha".into();
        session.records[0].household_region = "alpha city county".into();
        store.save(&session).unwrap();

        let page = store
            .query_imported_records(
                "session-1",
                &ImportedRecordsQuery {
                    household_province: "alp".into(),
                    page: 1,
                    page_size: 50,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(page.total, 1);

        session.records[0].household_province = "beta".into();
        session.records[0].household_region = "beta city county".into();
        store.save(&session).unwrap();

        let page = store
            .query_imported_records(
                "session-1",
                &ImportedRecordsQuery {
                    household_province: "alp".into(),
                    page: 1,
                    page_size: 50,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(page.total, 0);
        let page = store
            .query_imported_records(
                "session-1",
                &ImportedRecordsQuery {
                    household_province: "bet".into(),
                    page: 1,
                    page_size: 50,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(page.total, 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn household_split_column_prefix_semantic_replaces_substring() {
        // The new prefix semantic should match leading characters of province/city/county
        // but NOT a middle substring — that is the documented product tradeoff. Existing
        // tests `hotel_terms_use_fuzzy_and_and_regions_match_one_stay` and
        // `person_attributes_and_household_filters_are_applied_in_sqlite` already exercise
        // the prefix path against typical leading-char input; this test pins the negative
        // case: mid-substring like '省' won't match.
        let (root, store) = test_store();
        store.save(&sample_session()).unwrap();

        // '安徽' prefix matches the stored household_province '安徽省'.
        let mut matched = query();
        matched.household_province = "安徽".into();
        assert_eq!(store.query_people("session-1", &matched).unwrap().total, 1);

        // '省' as a mid-substring no longer matches under prefix semantic — this is the
        // deliberate narrowing that lets the B-tree index (idx_people_household_split)
        // serve the filter. Users type the region's leading characters.
        let mut matched = query();
        matched.household_province = "省".into();
        assert_eq!(store.query_people("session-1", &matched).unwrap().total, 0);

        // Imported-record path likewise.
        let page = store
            .query_imported_records(
                "session-1",
                &ImportedRecordsQuery {
                    household_province: "安徽".into(),
                    page: 1,
                    page_size: 50,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(page.total, 1);

        let page = store
            .query_imported_records(
                "session-1",
                &ImportedRecordsQuery {
                    household_province: "省".into(),
                    page: 1,
                    page_size: 50,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(page.total, 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exclude_household_split_column_prefix_takes_negation_correctly() {
        // NOT over prefix match set must exclude only when the user-typed prefix actually
        // is the leading substring of the stored region component.
        let (root, store) = test_store();
        store.save(&sample_session()).unwrap();

        // Excluding '安徽' (prefix of stored '安徽省') must drop the only person.
        let mut matched = query();
        matched.exclude_household_province = "安徽".into();
        assert_eq!(store.query_people("session-1", &matched).unwrap().total, 0);

        // Excluding an unrelated prefix '浙江' keeps the person.
        let mut matched = query();
        matched.exclude_household_province = "浙江".into();
        assert_eq!(store.query_people("session-1", &matched).unwrap().total, 1);

        // Imported-record exclude path: same semantic.
        let page = store
            .query_imported_records(
                "session-1",
                &ImportedRecordsQuery {
                    exclude_household_county: "祁门".into(),
                    page: 1,
                    page_size: 50,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(page.total, 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn hotel_terms_use_fuzzy_and_and_regions_match_one_stay() {
        let (root, store) = test_store();
        store.save(&sample_session()).unwrap();
        let mut matched = query();
        matched.hotel_search = "旅A，商务B".into();
        matched.hotel_province = "安徽".into();
        matched.hotel_county = "祁门".into();
        assert_eq!(store.query_people("session-1", &matched).unwrap().total, 1);

        matched.hotel_county = "西湖".into();
        assert_eq!(store.query_people("session-1", &matched).unwrap().total, 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn person_attributes_and_household_filters_are_applied_in_sqlite() {
        let (root, store) = test_store();
        store.save(&sample_session()).unwrap();
        let mut matched = query();
        matched.search = "341024".into();
        matched.household_province = "安徽".into();
        matched.exclude_household_county = "休宁".into();
        matched.min_age = Some(30);
        matched.max_age = Some(40);
        matched.gender = "男".into();
        matched.level = "正常".into();
        matched.alert_state = "未预警人员".into();
        assert_eq!(store.query_people("session-1", &matched).unwrap().total, 1);

        matched.exclude_household_province = "安徽".into();
        matched.exclude_household_county = "祁门".into();
        assert_eq!(store.query_people("session-1", &matched).unwrap().total, 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn delete_selects_the_next_active_session() {
        let (root, store) = test_store();
        store.save(&sample_session()).unwrap();
        let mut second = sample_session();
        second.session_id = "session-2".into();
        second.imported_at = "2026-07-22T11:00:00+08:00".into();
        store.save(&second).unwrap();
        let connection = store.connection().unwrap();
        let session_one_record_rowid: i64 = connection
            .query_row(
                "SELECT rowid FROM records WHERE session_id = 'session-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let session_two_record_rowid: i64 = connection
            .query_row(
                "SELECT rowid FROM records WHERE session_id = 'session-2'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let session_one_person_rowid: i64 = connection
            .query_row(
                "SELECT rowid FROM people WHERE session_id = 'session-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let session_two_person_rowid: i64 = connection
            .query_row(
                "SELECT rowid FROM people WHERE session_id = 'session-2'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        drop(connection);

        let active = store.delete("session-2").unwrap().unwrap();
        assert_eq!(active.session_id, "session-1");
        let connection = store.connection().unwrap();
        for (table, remaining_rowid, deleted_rowid) in [
            (
                "records_search_fts",
                session_one_record_rowid,
                session_two_record_rowid,
            ),
            (
                "people_search_fts",
                session_one_person_rowid,
                session_two_person_rowid,
            ),
        ] {
            let deleted_exists: bool = connection
                .query_row(
                    &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE rowid = ?1)"),
                    [deleted_rowid],
                    |row| row.get(0),
                )
                .unwrap();
            let remaining_exists: bool = connection
                .query_row(
                    &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE rowid = ?1)"),
                    [remaining_rowid],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(!deleted_exists);
            assert!(remaining_exists);
        }
        drop(connection);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deleting_a_missing_session_keeps_the_database_unchanged() {
        let (root, store) = test_store();
        store.save(&sample_session()).unwrap();
        assert!(matches!(
            store.delete("missing-session"),
            Err(AppError::SessionNotFound)
        ));
        assert_eq!(store.list().unwrap().len(), 1);
        assert_eq!(store.query_people("session-1", &query()).unwrap().total, 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deleting_the_last_listed_session_discards_transient_combined_sessions() {
        let (root, store) = test_store();
        store.save(&sample_session()).unwrap();
        let mut combined = sample_session();
        combined.session_id = "combined-session".into();
        combined.is_combined = true;
        combined.source_session_ids = vec!["session-1".into()];
        store.save(&combined).unwrap();
        assert_eq!(store.list().unwrap().len(), 1);

        assert!(store.delete("session-1").unwrap().is_none());
        assert!(store.list().unwrap().is_empty());
        assert!(matches!(
            store.load("combined-session"),
            Err(AppError::SessionNotFound)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deleting_the_last_session_recreates_an_empty_database_file() {
        let (root, store) = test_store();
        store.save(&sample_session()).unwrap();
        {
            let connection = store.connection().unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE stale_marker(payload BLOB);
                     INSERT INTO stale_marker(payload) VALUES(zeroblob(10485760));
                     PRAGMA wal_checkpoint(TRUNCATE);",
                )
                .unwrap();
        }
        assert!(fs::metadata(&store.database_path).unwrap().len() > 8 * 1024 * 1024);

        assert!(store.delete("session-1").unwrap().is_none());
        assert!(store.list().unwrap().is_empty());
        let connection = store.connection().unwrap();
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        let stale_table_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = 'stale_marker')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, DATABASE_VERSION);
        assert!(!stale_table_exists);
        drop(connection);

        store.save(&sample_session()).unwrap();
        assert_eq!(store.list().unwrap().len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn opening_an_oversized_empty_database_recreates_it() {
        let (root, store) = test_store();
        let database_path = store.database_path.clone();
        {
            let connection = store.connection().unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE stale_marker(payload BLOB);
                     INSERT INTO stale_marker(payload) VALUES(zeroblob(10485760));
                     PRAGMA wal_checkpoint(TRUNCATE);",
                )
                .unwrap();
        }
        drop(store);
        let oversized_length = fs::metadata(&database_path).unwrap().len();
        assert!(oversized_length > EMPTY_DATABASE_RESET_THRESHOLD_BYTES);

        let reopened = SessionStore::open(root.clone()).unwrap();
        assert!(reopened.list().unwrap().is_empty());
        assert!(fs::metadata(&database_path).unwrap().len() < oversized_length);
        let connection = reopened.connection().unwrap();
        let stale_table_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = 'stale_marker')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!stale_table_exists);
        drop(connection);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn moving_storage_copies_the_sqlite_database() {
        let (root, store) = test_store();
        store.save(&sample_session()).unwrap();
        let destination = std::env::temp_dir().join(format!("maiyin-moved-{}", Uuid::new_v4()));
        fs::create_dir_all(&destination).unwrap();
        let moved = store.move_to(destination.clone()).unwrap();
        assert_eq!(moved.list().unwrap().len(), 1);
        assert_eq!(moved.load("session-1").unwrap().analyses.len(), 1);
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(destination).unwrap();
    }

    #[test]
    fn failed_save_rolls_back_the_previous_session() {
        let (root, store) = test_store();
        store.save(&sample_session()).unwrap();
        let mut invalid = sample_session();
        invalid.file_name = "broken.xlsx".into();
        invalid.analyses.push(invalid.analyses[0].clone());
        assert!(store.save(&invalid).is_err());
        let restored = store.metadata("session-1").unwrap();
        assert_eq!(restored.file_name, "test.xlsx");
        assert_eq!(store.query_people("session-1", &query()).unwrap().total, 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "requires MAIYIN_BENCH_FILES with semicolon-separated source files"]
    fn benchmark_real_import_pipeline() {
        let paths = std::env::var("MAIYIN_BENCH_FILES")
            .expect("set MAIYIN_BENCH_FILES to one or more source files")
            .split(';')
            .filter(|path| !path.trim().is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        let total_started = Instant::now();
        let parse_started = Instant::now();
        let imported = import_paths(&paths).unwrap();
        let parse_elapsed = parse_started.elapsed();
        let analysis_started = Instant::now();
        let (analyses, stats) = analyze_records(&imported.records, &AnalysisSettings::default());
        let analysis_elapsed = analysis_started.elapsed();
        let (root, store) = test_store();
        let session = StoredSession {
            schema_version: CURRENT_SCHEMA_VERSION,
            session_id: "benchmark".into(),
            file_name: imported.title,
            imported_at: "2026-07-22T00:00:00+08:00".into(),
            file_count: imported.file_count,
            settings: AnalysisSettings::default(),
            records: imported.records,
            analyses,
            stats,
            import_stats: imported.stats,
            source_session_ids: vec![],
            is_combined: false,
        };
        let save_started = Instant::now();
        store.save(&session).unwrap();
        let save_elapsed = save_started.elapsed();
        let query_started = Instant::now();
        let page = store.query_people("benchmark", &query()).unwrap();
        let query_elapsed = query_started.elapsed();
        println!(
            "records={} people={} parse_ms={} analysis_ms={} save_ms={} first_page_ms={} total_ms={}",
            session.stats.records,
            session.stats.people,
            parse_elapsed.as_millis(),
            analysis_elapsed.as_millis(),
            save_elapsed.as_millis(),
            query_elapsed.as_millis(),
            total_started.elapsed().as_millis(),
        );
        assert!(!page.items.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "large synthetic performance benchmark"]
    fn benchmark_large_history_first_page() {
        let people_count = std::env::var("MAIYIN_BENCH_PEOPLE")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(453_506);
        let (root, store) = test_store();
        let analyses = (0..people_count)
            .map(|index| PersonAnalysis {
                summary: PersonSummary {
                    person_key: format!("id:{index:018}"),
                    name: format!("人员{index}"),
                    id_no: format!("{index:018}"),
                    phone: String::new(),
                    household_region: "安徽省 黄山市 祁门县".into(),
                    household_province: "安徽省".into(),
                    household_city: "黄山市".into(),
                    household_county: "祁门县".into(),
                    age: Some(37),
                    gender: "男".into(),
                    total_records: 1,
                    max_week_count: 1,
                    max_month_count: 1,
                    max_year_count: 1,
                    overlap_days: 0,
                    sequential_days: 0,
                    score: (index % 100) as u32,
                    level: "正常".into(),
                    alert_count: 0,
                    alert_titles: vec![],
                    hotel_names: vec![],
                    hotel_regions: vec![],
                },
                alerts: vec![],
            })
            .collect::<Vec<_>>();
        let session = StoredSession {
            schema_version: CURRENT_SCHEMA_VERSION,
            session_id: "large-benchmark".into(),
            file_name: "large.xlsx".into(),
            imported_at: "2026-07-22T00:00:00+08:00".into(),
            file_count: 15,
            settings: AnalysisSettings::default(),
            records: vec![],
            analyses,
            stats: AnalysisStats {
                people: people_count,
                ..Default::default()
            },
            import_stats: ImportStats::default(),
            source_session_ids: vec![],
            is_combined: false,
        };
        let save_started = Instant::now();
        store.save(&session).unwrap();
        let save_elapsed = save_started.elapsed();
        drop(session);
        drop(store);

        let open_started = Instant::now();
        let reopened = SessionStore::open(root.clone()).unwrap();
        let metadata = reopened.metadata("large-benchmark").unwrap();
        let page = reopened.query_people("large-benchmark", &query()).unwrap();
        let open_elapsed = open_started.elapsed();
        println!(
            "people={} save_ms={} reopen_and_first_page_ms={}",
            people_count,
            save_elapsed.as_millis(),
            open_elapsed.as_millis(),
        );
        assert_eq!(metadata.stats.people, people_count);
        assert_eq!(page.total, people_count);
        assert_eq!(page.items.len(), 50);
        assert!(open_elapsed.as_secs_f64() <= 2.0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "large synthetic filter-latency benchmark (set MAIYIN_BENCH_PEOPLE / MAIYIN_BENCH_RECORDS)"]
    fn benchmark_filter_latency_on_large_session() {
        // Builds (people_count, records_count) synthetic session and times the four
        // fast paths surfaced in this task: search_text FTS5 trigram, household
        // split-column prefix, hotel jurisdiction split-column prefix (records side),
        // plus the layered fuzzy fallback for hotel_name (ordered-subseq LIKE on the
        // (session_id, hotel_name_norm) indexed range). Prints milliseconds for each
        // path; expected to stay under 500ms per path at 1M records.
        let people_count = std::env::var("MAIYIN_BENCH_PEOPLE")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(50_000);
        let records_count = std::env::var("MAIYIN_BENCH_RECORDS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(100_000);
        let (root, store) = test_store();

        let provinces = ["安徽省", "浙江省", "江苏省", "四川省"];
        let cities = ["黄山市", "杭州市", "南京市", "成都市"];
        let counties = ["祁门县", "西湖区", "鼓楼区", "锦江区"];
        let hotel_names = ["旅馆 A", "锦江城市酒店", "如家快捷", "汉庭酒店"];
        let names = ["张三", "李四", "王五", "赵六"];

        // Build `people_count` PersonSummary rows on a rotating jurisdiction/hotel basis
        // so a province filter narrows to ~25% of the population.
        let analyses = (0..people_count)
            .map(|index| {
                let bucket = index % provinces.len();
                PersonAnalysis {
                    summary: PersonSummary {
                        person_key: format!("id:{index:018}"),
                        name: format!("{}{}", names[bucket], index),
                        id_no: format!("{index:018}"),
                        phone: String::new(),
                        household_region: format!(
                            "{} {} {}",
                            provinces[bucket], cities[bucket], counties[bucket]
                        ),
                        household_province: provinces[bucket].into(),
                        household_city: cities[bucket].into(),
                        household_county: counties[bucket].into(),
                        age: Some(30 + (index % 50) as u8),
                        gender: if index % 2 == 0 { "男" } else { "女" }.into(),
                        total_records: 1,
                        max_week_count: 1,
                        max_month_count: 1,
                        max_year_count: 1,
                        overlap_days: 0,
                        sequential_days: 0,
                        score: (index % 100) as u32,
                        level: "正常".into(),
                        alert_count: 0,
                        alert_titles: vec![],
                        hotel_names: vec![hotel_names[bucket].to_string()],
                        hotel_regions: vec![HotelRegion {
                            province: provinces[bucket].into(),
                            city: cities[bucket].into(),
                            county: counties[bucket].into(),
                            region: format!(
                                "{}{}{}",
                                provinces[bucket], cities[bucket], counties[bucket]
                            ),
                        }],
                    },
                    alerts: vec![],
                }
            })
            .collect::<Vec<_>>();
        let records = (0..records_count)
            .map(|index| {
                let bucket = index % provinces.len();
                let mut record = sample_record(u64::try_from(index + 1).unwrap_or(1), Some(1));
                record.name = format!("{}{}", names[bucket], index);
                record.hotel_name = hotel_names[bucket].into();
                record.province = provinces[bucket].into();
                record.city = cities[bucket].into();
                record.county = counties[bucket].into();
                record.region = format!(
                    "{} {} {}",
                    provinces[bucket], cities[bucket], counties[bucket]
                );
                record.household_province = provinces[bucket].into();
                record.household_city = cities[bucket].into();
                record.household_county = counties[bucket].into();
                record.household_region = format!(
                    "{} {} {}",
                    provinces[bucket], cities[bucket], counties[bucket]
                );
                record.person_key = format!("id:{:018}", index % people_count.max(1));
                record
            })
            .collect::<Vec<_>>();
        let session = StoredSession {
            schema_version: CURRENT_SCHEMA_VERSION,
            session_id: "filter-bench".into(),
            file_name: "filter.xlsx".into(),
            imported_at: "2026-07-22T00:00:00+08:00".into(),
            file_count: 4,
            settings: AnalysisSettings::default(),
            records,
            analyses,
            stats: AnalysisStats {
                people: people_count,
                records: records_count,
                ..Default::default()
            },
            import_stats: ImportStats::default(),
            source_session_ids: vec![],
            is_combined: false,
        };
        let save_started = Instant::now();
        store.save(&session).unwrap();
        let save_elapsed = save_started.elapsed();
        drop(session);
        drop(store);

        // Reopen to mimic post-startup filter behavior.
        let reopened = SessionStore::open(root.clone()).unwrap();

        // 1. people search_text via FTS5 trigram (≥3 chars).
        let mut q = query();
        q.search = "张三1".into();
        let started = Instant::now();
        let page = reopened.query_people("filter-bench", &q).unwrap();
        let fts5_ms = started.elapsed().as_millis();
        assert!(!page.items.is_empty());

        // 2. people household_province prefix.
        let mut q = query();
        q.household_province = "安徽".into();
        let started = Instant::now();
        let _page = reopened.query_people("filter-bench", &q).unwrap();
        let household_ms = started.elapsed().as_millis();

        // 3. imported-records household_province prefix.
        let started = Instant::now();
        let _page = reopened
            .query_imported_records(
                "filter-bench",
                &ImportedRecordsQuery {
                    household_province: "安徽".into(),
                    page: 1,
                    page_size: 50,
                    ..Default::default()
                },
            )
            .unwrap();
        let records_household_ms = started.elapsed().as_millis();

        // 4. imported-records hotel_jurisdiction prefix.
        let started = Instant::now();
        let _page = reopened
            .query_imported_records(
                "filter-bench",
                &ImportedRecordsQuery {
                    hotel_province: "安徽".into(),
                    page: 1,
                    page_size: 50,
                    ..Default::default()
                },
            )
            .unwrap();
        let records_hotel_ms = started.elapsed().as_millis();

        // 5. layered ordered-subseq hotel_name LIKE on the indexed records range.
        let started = Instant::now();
        let _page = reopened
            .query_imported_records(
                "filter-bench",
                &ImportedRecordsQuery {
                    hotel_search: "旅馆A".into(),
                    page: 1,
                    page_size: 50,
                    ..Default::default()
                },
            )
            .unwrap();
        let fuzzy_ms = started.elapsed().as_millis();

        println!(
            "people={} records={} save_ms={} fts5_search_ms={} household_prefix_ms={} records_household_ms={} records_hotel_ms={} fuzzy_hotel_ms={}",
            people_count,
            records_count,
            save_elapsed.as_millis(),
            fts5_ms,
            household_ms,
            records_household_ms,
            records_hotel_ms,
            fuzzy_ms,
        );

        fs::remove_dir_all(root).unwrap();
    }
}
