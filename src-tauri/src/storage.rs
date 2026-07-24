use crate::error::AppError;
use crate::model::{
    AlertSummary, AnalysisSettings, AnalysisStats, ImportStats, ImportedRecordsPage,
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
#[cfg(test)]
use std::time::Instant;

mod compress;
mod filter;
mod schema;
#[cfg(test)]
mod tests;
mod write;

pub(crate) use compress::{compressed_json, from_json, from_stored_json, json};
pub(crate) use filter::{
    build_person_filter, build_records_filter, fast_record_filter_count,
    increment_record_filter_count, normalize, records_count_source,
};
pub(crate) use schema::{
    initialize_schema, BULK_SAVE_CACHE_KIB, DATABASE_FILE, DATA_FOLDER,
    EMPTY_DATABASE_RESET_THRESHOLD_BYTES, SESSION_FTS_TABLES,
};
pub(crate) use write::{
    insert_analysis_rows, insert_people_search_index, insert_record_batches, prepare_record_chunk,
    SAVE_PREPARE_CHUNK_SIZE,
};

// Keep generated multi-row INSERT statements below SQLite's historical 999-variable
// default as well as the higher limit used by the bundled build.

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
        connection
            .pragma_update(None, "cache_size", -BULK_SAVE_CACHE_KIB)
            .map_err(sql_error)?;
        let transaction = connection.transaction().map_err(sql_error)?;
        #[cfg(test)]
        let save_timing_enabled = std::env::var_os("MAIYIN_SAVE_TIMINGS").is_some();
        #[cfg(test)]
        let save_started = Instant::now();
        #[cfg(test)]
        let save_mark = |label: &str| {
            if save_timing_enabled {
                eprintln!(
                    "save_stage={} elapsed_ms={}",
                    label,
                    save_started.elapsed().as_millis()
                );
            }
        };
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
        #[cfg(test)]
        save_mark("session_row");

        {
            let mut record_filter_counts = HashMap::<(String, String), i64>::new();
            std::thread::scope(|scope| -> Result<(), AppError> {
                let (sender, receiver) = std::sync::mpsc::sync_channel(1);
                let records = &session.records;
                let producer = scope.spawn(move || {
                    for chunk in records.chunks(SAVE_PREPARE_CHUNK_SIZE) {
                        let prepared = prepare_record_chunk(chunk);
                        let stop = prepared.is_err();
                        if sender.send(prepared).is_err() || stop {
                            break;
                        }
                    }
                });
                let consumer_result = (|| -> Result<(), AppError> {
                    for prepared_records in receiver {
                        let prepared_records = prepared_records?;
                        for prepared in &prepared_records {
                            let record = prepared.record;
                            if record.check_in.is_some() {
                                increment_record_filter_count(
                                    &mut record_filter_counts,
                                    "hotel_name",
                                    &prepared.hotel_name_norm,
                                );
                                increment_record_filter_count(
                                    &mut record_filter_counts,
                                    "hotel_province",
                                    &prepared.hotel_province_norm,
                                );
                                increment_record_filter_count(
                                    &mut record_filter_counts,
                                    "hotel_city",
                                    &prepared.hotel_city_norm,
                                );
                                increment_record_filter_count(
                                    &mut record_filter_counts,
                                    "hotel_county",
                                    &prepared.hotel_county_norm,
                                );
                                increment_record_filter_count(
                                    &mut record_filter_counts,
                                    "household_province",
                                    &prepared.household_province_norm,
                                );
                                increment_record_filter_count(
                                    &mut record_filter_counts,
                                    "household_city",
                                    &prepared.household_city_norm,
                                );
                                increment_record_filter_count(
                                    &mut record_filter_counts,
                                    "household_county",
                                    &prepared.household_county_norm,
                                );
                            }
                        }
                        insert_record_batches(
                            &transaction,
                            &session.session_id,
                            &prepared_records,
                        )?;
                    }
                    Ok(())
                })();
                producer
                    .join()
                    .map_err(|_| AppError::Storage("record preparation worker panicked".into()))?;
                consumer_result
            })?;
            #[cfg(test)]
            save_mark("records_base");
            // Mirror all rows into the contentless FTS5 trigram table in one SQLite statement.
            // The SELECT uses the source table's real rowid, not the session-local business
            // uid, and avoids one Rust/SQLite round-trip per imported record.
            transaction
                .execute(
                    "INSERT INTO records_search_fts_v2(rowid, search_text) \
                     SELECT rowid, search_text FROM records \
                     WHERE session_id = ?1",
                    [&session.session_id],
                )
                .map_err(sql_error)?;
            #[cfg(test)]
            save_mark("records_fts");
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
            #[cfg(test)]
            save_mark("records_and_fts");
        }

        insert_analysis_rows(&transaction, &session.session_id, &session.analyses)?;
        #[cfg(test)]
        save_mark("people_base");
        insert_people_search_index(&transaction, &session.session_id)?;
        #[cfg(test)]
        save_mark("people_fts");

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
        #[cfg(test)]
        save_mark("commit");
        metadata_from(&connection, &session.session_id)
    }

    pub fn replace_analysis(
        &self,
        session_id: &str,
        schema_version: u32,
        settings: &AnalysisSettings,
        analyses: &[PersonAnalysis],
        stats: &AnalysisStats,
    ) -> Result<SessionMetadata, AppError> {
        let settings_json = json(settings)?;
        let stats_json = json(stats)?;
        let _write_guard = self.lock_writes()?;
        let mut connection = self.connection()?;
        connection
            .pragma_update(None, "cache_size", -BULK_SAVE_CACHE_KIB)
            .map_err(sql_error)?;
        let transaction = connection.transaction().map_err(sql_error)?;
        ensure_session_exists(&transaction, session_id)?;

        // Delete FTS documents through their mirrored source rowids before the people
        // cascade removes those source rows. Raw records and their indexes are unchanged.
        delete_analysis_fts_rows(&transaction, session_id)?;
        transaction
            .execute("DELETE FROM people WHERE session_id = ?1", [session_id])
            .map_err(sql_error)?;
        transaction
            .execute(
                "UPDATE sessions SET
                    schema_version = ?1,
                    settings_json = ?2,
                    stats_json = ?3,
                    records = ?4,
                    people = ?5
                 WHERE session_id = ?6",
                params![
                    i64::from(schema_version),
                    settings_json,
                    stats_json,
                    i64_from_usize(stats.records),
                    i64_from_usize(stats.people),
                    session_id,
                ],
            )
            .map_err(sql_error)?;
        insert_analysis_rows(&transaction, session_id, analyses)?;
        insert_people_search_index(&transaction, session_id)?;
        transaction.commit().map_err(sql_error)?;
        metadata_from(&connection, session_id)
    }

    pub fn load_records(
        &self,
        session_id: &str,
    ) -> Result<(SessionMetadata, Vec<Record>), AppError> {
        let _read_guard = self.lock_reads()?;
        let connection = self.connection()?;
        let metadata = metadata_from(&connection, session_id)?;
        let records = load_session_records(&connection, session_id)?;
        Ok((metadata, records))
    }

    pub fn load(&self, session_id: &str) -> Result<StoredSession, AppError> {
        let _read_guard = self.lock_reads()?;
        let connection = self.connection()?;
        let metadata = metadata_from(&connection, session_id)?;
        let records = load_session_records(&connection, session_id)?;

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
            let payload: Value = row.get(0).map_err(sql_error)?;
            items.push(from_stored_json(payload)?);
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
            let payload: Value = row.get(0).map_err(sql_error)?;
            items.push(crate::model::ImportedStayRecord::from(from_stored_json::<
                Record,
            >(
                payload
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
        let summary_payload: Option<Value> = connection
            .query_row(
                "SELECT summary_json FROM people WHERE session_id = ?1 AND person_key = ?2",
                params![session_id, person_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_error)?;
        let person = summary_payload
            .map(from_stored_json)
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
    delete_fts_rows_for_sources(
        transaction,
        session_id,
        &["records", "people", "person_hotels"],
    )
}

fn delete_analysis_fts_rows(
    transaction: &Transaction<'_>,
    session_id: &str,
) -> Result<(), AppError> {
    delete_fts_rows_for_sources(transaction, session_id, &["people", "person_hotels"])
}

fn delete_fts_rows_for_sources(
    transaction: &Transaction<'_>,
    session_id: &str,
    source_tables: &[&str],
) -> Result<(), AppError> {
    // Contentless FTS tables cannot reliably return their UNINDEXED session_id value.
    // Delete by the mirrored content-table rowid while those source rows still exist.
    for (fts_table, content_table) in SESSION_FTS_TABLES {
        if !source_tables.contains(&content_table) {
            continue;
        }
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
        let payload: Value = row.get(0).map_err(sql_error)?;
        result.push(from_stored_json(payload)?);
    }
    Ok(result)
}

fn load_session_records(
    connection: &Connection,
    session_id: &str,
) -> Result<Vec<Record>, AppError> {
    load_json_column(
        connection,
        "SELECT record_json FROM records WHERE session_id = ?1 ORDER BY uid",
        session_id,
    )
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
        let payload: Value = row.get(0).map_err(sql_error)?;
        result.push(from_stored_json(payload)?);
    }
    Ok(result)
}

pub(crate) fn i64_from_usize(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

pub(crate) fn i64_from_u64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

pub(crate) fn usize_from_i64(value: i64) -> usize {
    usize::try_from(value.max(0)).unwrap_or(usize::MAX)
}

pub(crate) fn storage_error(error: std::io::Error) -> AppError {
    AppError::Storage(error.to_string())
}

pub(crate) fn sql_error(error: rusqlite::Error) -> AppError {
    AppError::Storage(error.to_string())
}
