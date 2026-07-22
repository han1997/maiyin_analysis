use crate::error::AppError;
use crate::model::{
    AlertSummary, AnalysisSettings, AnalysisStats, ImportStats, ImportedRecordsPage,
    ImportedRecordsQuery, PersonAnalysis, PersonDetail, PersonPage, PersonQuery, PersonSummary,
    Record, SessionSummary, StoredSession,
};
use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

const DATA_FOLDER: &str = "MaiyinAnalysisData";
const DATABASE_FILE: &str = "history-v1.sqlite3";
const DATABASE_VERSION: i64 = 2;

#[derive(Debug, Clone)]
pub struct SessionStore {
    storage_root: PathBuf,
    database_path: PathBuf,
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
        };
        let connection = store.connection()?;
        initialize_schema(&connection)?;
        Ok(store)
    }

    pub fn list(&self) -> Result<Vec<SessionSummary>, AppError> {
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

    pub fn active_id(&self) -> Result<Option<String>, AppError> {
        active_id_from(&self.connection()?)
    }

    pub fn metadata(&self, session_id: &str) -> Result<SessionMetadata, AppError> {
        let connection = self.connection()?;
        metadata_from(&connection, session_id)
    }

    pub fn activate(&self, session_id: &str) -> Result<SessionMetadata, AppError> {
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
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(sql_error)?;
        transaction
            .execute(
                "DELETE FROM sessions WHERE listed = 0 AND session_id <> ?1",
                [&session.session_id],
            )
            .map_err(sql_error)?;
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
            let mut record_statement = transaction
                .prepare(
                    "INSERT INTO records(session_id, uid, person_key, check_in, record_json) \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )
                .map_err(sql_error)?;
            for record in &session.records {
                record_statement
                    .execute(params![
                        session.session_id,
                        i64_from_u64(record.uid),
                        record.person_key,
                        record
                            .check_in
                            .map(|value| value.format("%Y-%m-%d %H:%M:%S").to_string()),
                        json(record)?,
                    ])
                    .map_err(sql_error)?;
            }
        }

        {
            let mut person_statement = transaction
                .prepare(
                    "INSERT INTO people(
                        session_id, person_key, name, name_norm, id_no_norm, phone_norm,
                        household_region_norm, age, gender, level, alert_count,
                        total_records, score, search_text, summary_json
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
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
        self.metadata(&session.session_id)
    }

    pub fn load(&self, session_id: &str) -> Result<StoredSession, AppError> {
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
        let connection = self.connection()?;
        ensure_session_exists(&connection, session_id)?;
        let page_size = query.page_size.clamp(1, 500);
        let page = query.page.max(1).min(usize_from_i64(i64::MAX) / page_size);
        let (where_sql, values) = build_person_filter(session_id, query);
        let total: i64 = connection
            .query_row(
                &format!("SELECT COUNT(*) FROM people p WHERE {where_sql}"),
                params_from_iter(values.iter()),
                |row| row.get(0),
            )
            .map_err(sql_error)?;

        let mut paged_values = values;
        paged_values.push(Value::Integer(i64_from_usize(page_size)));
        paged_values.push(Value::Integer(i64_from_usize(
            (page - 1).saturating_mul(page_size),
        )));
        let mut statement = connection
            .prepare(&format!(
                "SELECT p.summary_json FROM people p WHERE {where_sql} \
                 ORDER BY p.score DESC, p.total_records DESC, p.name ASC, p.person_key ASC LIMIT ? OFFSET ?"
            ))
            .map_err(sql_error)?;
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
        let connection = self.connection()?;
        ensure_session_exists(&connection, session_id)?;
        let settings = metadata_from(&connection, session_id)?.settings;
        let page_size = query.page_size.clamp(1, 500);
        let page = query.page.max(1).min(usize_from_i64(i64::MAX) / page_size);
        let mut clauses = vec![
            "session_id = ?".to_string(),
            "check_in IS NOT NULL".to_string(),
        ];
        let mut values = vec![Value::Text(session_id.to_string())];
        if let Some(start) = settings.frequency_start {
            clauses.push("check_in >= ?".into());
            values.push(Value::Text(start.format("%Y-%m-%d %H:%M:%S").to_string()));
        }
        if let Some(end) = settings.frequency_end {
            clauses.push("check_in <= ?".into());
            values.push(Value::Text(end.format("%Y-%m-%d %H:%M:%S").to_string()));
        }
        let where_sql = clauses.join(" AND ");
        let total: i64 = connection
            .query_row(
                &format!("SELECT COUNT(*) FROM records WHERE {where_sql}"),
                params_from_iter(values.iter()),
                |row| row.get(0),
            )
            .map_err(sql_error)?;

        let mut paged_values = values;
        paged_values.push(Value::Integer(i64_from_usize(page_size)));
        paged_values.push(Value::Integer(i64_from_usize(
            (page - 1).saturating_mul(page_size),
        )));
        let mut statement = connection
            .prepare(&format!(
                "SELECT record_json FROM records WHERE {where_sql} \
                 ORDER BY check_in ASC, uid ASC LIMIT ? OFFSET ?"
            ))
            .map_err(sql_error)?;
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
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(sql_error)?;
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
        self.active_id()?
            .map(|active| self.metadata(&active))
            .transpose()
    }

    pub fn move_to(&self, destination_root: PathBuf) -> Result<Self, AppError> {
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
}

fn initialize_schema(connection: &Connection) -> Result<(), AppError> {
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(sql_error)?;
    if version == 1 {
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
             CREATE INDEX IF NOT EXISTS idx_sessions_imported_at ON sessions(listed, imported_at DESC);
             CREATE INDEX IF NOT EXISTS idx_records_person ON records(session_id, person_key);
             CREATE INDEX IF NOT EXISTS idx_records_check_in ON records(session_id, check_in, uid);
             CREATE INDEX IF NOT EXISTS idx_people_sort ON people(session_id, score DESC, total_records DESC, name ASC, person_key ASC);
             CREATE INDEX IF NOT EXISTS idx_people_level_alert ON people(session_id, level, alert_count);
             CREATE INDEX IF NOT EXISTS idx_people_age_gender ON people(session_id, age, gender);
             CREATE INDEX IF NOT EXISTS idx_person_hotels_lookup ON person_hotels(session_id, person_key, hotel_name_norm);
             CREATE INDEX IF NOT EXISTS idx_person_regions_lookup ON person_hotel_regions(session_id, person_key);
             PRAGMA user_version = {DATABASE_VERSION};"
        ))
        .map_err(sql_error)
}

fn reset_legacy_database(connection: &Connection) -> Result<(), AppError> {
    connection
        .execute_batch(
            "PRAGMA foreign_keys = OFF;
             DROP TABLE IF EXISTS person_hotel_regions;
             DROP TABLE IF EXISTS person_hotels;
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

fn build_person_filter(session_id: &str, query: &PersonQuery) -> (String, Vec<Value>) {
    let mut clauses = vec!["p.session_id = ?".to_string()];
    let mut values = vec![Value::Text(session_id.to_string())];

    let search = normalize(&query.search);
    if !search.is_empty() {
        clauses.push("p.search_text LIKE ? ESCAPE '\\'".into());
        values.push(Value::Text(contains_pattern(&search)));
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
            region_clauses.push(format!(
                "(phr.{column} LIKE ? ESCAPE '\\' OR phr.region_norm LIKE ? ESCAPE '\\')"
            ));
            let pattern = contains_pattern(&value);
            values.push(Value::Text(pattern.clone()));
            values.push(Value::Text(pattern));
        }
        clauses.push(format!(
            "EXISTS (SELECT 1 FROM person_hotel_regions phr \
             WHERE phr.session_id = p.session_id AND phr.person_key = p.person_key AND {})",
            region_clauses.join(" AND ")
        ));
    }

    for value in [
        &query.household_province,
        &query.household_city,
        &query.household_county,
    ] {
        let value = normalize(value);
        if !value.is_empty() {
            clauses.push("p.household_region_norm LIKE ? ESCAPE '\\'".into());
            values.push(Value::Text(contains_pattern(&value)));
        }
    }
    let excluded = [
        &query.exclude_household_province,
        &query.exclude_household_city,
        &query.exclude_household_county,
    ]
    .into_iter()
    .map(|value| normalize(value))
    .filter(|value| !value.is_empty())
    .collect::<Vec<_>>();
    if !excluded.is_empty() {
        clauses.push(format!(
            "NOT ({})",
            vec!["p.household_region_norm LIKE ? ESCAPE '\\'"; excluded.len()].join(" AND ")
        ));
        values.extend(
            excluded
                .into_iter()
                .map(|value| Value::Text(contains_pattern(&value))),
        );
    }
    (clauses.join(" AND "), values)
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
                },
            )
            .unwrap();
        assert_eq!(clamped.page_size, 500);
        assert_eq!(clamped.items.len(), 2);
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
        let active = store.delete("session-2").unwrap().unwrap();
        assert_eq!(active.session_id, "session-1");
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
}
