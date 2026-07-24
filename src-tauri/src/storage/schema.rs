use super::sql_error;
use crate::error::AppError;
use rusqlite::Connection;
pub(crate) const DATA_FOLDER: &str = "MaiyinAnalysisData";

pub(crate) const DATABASE_FILE: &str = "history-v1.sqlite3";

pub(crate) const DATABASE_VERSION: i64 = 5;

pub(crate) const EMPTY_DATABASE_RESET_THRESHOLD_BYTES: u64 = 8 * 1024 * 1024;

pub(crate) const BULK_SAVE_CACHE_KIB: i64 = 128 * 1024;

pub(crate) const DATABASE_PAGE_SIZE: i64 = 16 * 1024;

pub(crate) const SESSION_FTS_TABLES: [(&str, &str); 6] = [
    ("records_search_fts", "records"),
    ("people_search_fts", "people"),
    ("records_search_fts_v2", "records"),
    ("people_search_fts_v2", "people"),
    ("records_hotel_name_fts", "records"),
    ("person_hotels_name_fts", "person_hotels"),
];

pub(crate) fn initialize_schema(connection: &Connection) -> Result<(), AppError> {
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(sql_error)?;
    if version == 1 || version == 2 || version == 3 {
        reset_legacy_database(connection)?;
    } else if version != 0 && version != 4 && version != DATABASE_VERSION {
        return Err(AppError::Storage(format!(
            "不支持的历史数据库版本 {version}，当前版本为 {DATABASE_VERSION}"
        )));
    }
    if version == 0 {
        #[cfg(test)]
        let page_size = std::env::var("MAIYIN_BENCH_PAGE_SIZE")
            .ok()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(DATABASE_PAGE_SIZE);
        #[cfg(not(test))]
        let page_size = DATABASE_PAGE_SIZE;
        connection
            .pragma_update(None, "page_size", page_size)
            .map_err(sql_error)?;
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
                record_json BLOB NOT NULL,
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
                summary_json BLOB NOT NULL,
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
             -- These explicit indexes duplicate the composite PRIMARY KEY indexes exactly
             -- (or use only their left-most prefix). Keeping them doubles/triples B-tree
             -- maintenance for the hottest people child-table inserts without improving
             -- the supported lookup plans. Drop them during the lossless v4 -> v5 upgrade
             -- as well as when opening an early v5 database created by a development build.
             DROP INDEX IF EXISTS idx_person_hotels_lookup;
             DROP INDEX IF EXISTS idx_person_hotel_regions_jurisdiction;
             DROP INDEX IF EXISTS idx_person_regions_lookup;
             DROP INDEX IF EXISTS idx_record_filter_counts_lookup;
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
             CREATE VIRTUAL TABLE IF NOT EXISTS records_search_fts USING fts5(
                search_text, session_id UNINDEXED, uid UNINDEXED,
                content='', contentless_delete=1, tokenize='trigram'
             );
             CREATE VIRTUAL TABLE IF NOT EXISTS people_search_fts USING fts5(
                search_text, session_id UNINDEXED, person_key UNINDEXED,
                content='', contentless_delete=1, tokenize='trigram'
             );
             CREATE VIRTUAL TABLE IF NOT EXISTS records_search_fts_v2 USING fts5(
                search_text, content='records', content_rowid='rowid', tokenize='trigram',
                detail=none, columnsize=0
             );
             CREATE VIRTUAL TABLE IF NOT EXISTS people_search_fts_v2 USING fts5(
                search_text, content='people', content_rowid='rowid', tokenize='trigram',
                detail=none, columnsize=0
             );
             PRAGMA user_version = {DATABASE_VERSION};"
        ))
        .map_err(sql_error)
}

pub(crate) fn reset_legacy_database(connection: &Connection) -> Result<(), AppError> {
    connection
        .execute_batch(
            "PRAGMA foreign_keys = OFF;
             DROP TABLE IF EXISTS people_search_fts_v2;
             DROP TABLE IF EXISTS records_search_fts_v2;
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
