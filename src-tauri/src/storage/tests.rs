use super::compress::COMPRESSED_JSON_MAGIC;
use super::filter::split_filter_terms;
use super::schema::{DATABASE_PAGE_SIZE, DATABASE_VERSION};
use super::*;
use crate::analysis::analyze_records;
use crate::importer::import_paths;
use crate::model::{FrequencyMode, HotelRegion, CURRENT_SCHEMA_VERSION};
use uuid::Uuid;

type RecordStorageSnapshot = (
    Vec<(i64, i64, Vec<u8>)>,
    Vec<i64>,
    Vec<(String, String, i64)>,
);

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
        frequency_window_count: 1,
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

fn analyzed_session(records: Vec<Record>) -> StoredSession {
    let (analyses, stats) = analyze_records(&records, &AnalysisSettings::default(), None);
    StoredSession {
        schema_version: CURRENT_SCHEMA_VERSION,
        session_id: "session-1".into(),
        file_name: "analysis.xlsx".into(),
        imported_at: "2026-07-23T10:00:00+08:00".into(),
        file_count: 1,
        settings: AnalysisSettings::default(),
        import_stats: ImportStats {
            imported: records.len(),
            ..Default::default()
        },
        records,
        analyses,
        stats,
        source_session_ids: vec![],
        is_combined: false,
    }
}

fn reanalysis_benchmark_session(people_count: usize, records_count: usize) -> StoredSession {
    let mut records = Vec::with_capacity(records_count);
    for index in 0..records_count {
        let person_index = index % people_count.max(1);
        let mut record = sample_record(
            u64::try_from(index + 1).unwrap_or(u64::MAX),
            Some(u32::try_from(index % 28 + 1).unwrap_or(1)),
        );
        record.person_key = format!("id:{person_index:018}");
        record.name = format!("人员{person_index:09}");
        record.id_no = format!("{person_index:018}");
        record.hotel_name = format!("旅馆{}", index % 5);
        record.room_no = format!("{}", 300 + index % 20);
        records.push(record);
    }
    let mut session = analyzed_session(records);
    session.session_id = "reanalyze-benchmark".into();
    session.file_name = "reanalyze-benchmark.csv".into();
    session.file_count = 15;
    session
}

fn record_storage_snapshot(store: &SessionStore, session_id: &str) -> RecordStorageSnapshot {
    let connection = store.connection().unwrap();
    let records = {
        let mut statement = connection
            .prepare(
                "SELECT rowid, uid, record_json FROM records \
                 WHERE session_id = ?1 ORDER BY rowid",
            )
            .unwrap();
        statement
            .query_map([session_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    let record_fts_rowids = {
        let mut statement = connection
            .prepare(
                "SELECT rowid FROM records_search_fts_v2 WHERE rowid IN (
                    SELECT rowid FROM records WHERE session_id = ?1
                 ) ORDER BY rowid",
            )
            .unwrap();
        statement
            .query_map([session_id], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    let filter_counts = {
        let mut statement = connection
            .prepare(
                "SELECT filter_kind, value_norm, record_count FROM record_filter_counts \
                 WHERE session_id = ?1 ORDER BY filter_kind, value_norm",
            )
            .unwrap();
        statement
            .query_map([session_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    (records, record_fts_rowids, filter_counts)
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
    let connection = store.connection().unwrap();
    let page_size: i64 = connection
        .query_row("PRAGMA page_size", [], |row| row.get(0))
        .unwrap();
    let (record_type, record_payload): (String, Value) = connection
        .query_row(
            "SELECT typeof(record_json), record_json FROM records WHERE session_id = ?1",
            ["session-1"],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let (summary_type, summary_payload): (String, Value) = connection
        .query_row(
            "SELECT typeof(summary_json), summary_json FROM people WHERE session_id = ?1",
            ["session-1"],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(record_type, "blob");
    assert_eq!(summary_type, "blob");
    assert_eq!(page_size, DATABASE_PAGE_SIZE);
    assert!(matches!(
        record_payload,
        Value::Blob(payload) if payload.starts_with(COMPRESSED_JSON_MAGIC)
    ));
    assert!(matches!(
        summary_payload,
        Value::Blob(payload) if payload.starts_with(COMPRESSED_JSON_MAGIC)
    ));
    drop(connection);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn stored_json_reader_accepts_plain_blob_and_rejects_corrupt_lz4() {
    let record = sample_record(7, Some(1));
    let plain_blob = serde_json::to_vec(&record).unwrap();
    let decoded: Record = from_stored_json(Value::Blob(plain_blob)).unwrap();
    assert_eq!(decoded.uid, record.uid);

    let mut corrupt = COMPRESSED_JSON_MAGIC.to_vec();
    corrupt.extend_from_slice(&4_u32.to_le_bytes());
    assert!(matches!(
        from_stored_json::<Record>(Value::Blob(corrupt)),
        Err(AppError::Storage(_))
    ));
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
    // v3 to current: also wiped + rebuilt (spec allows clear + re-import as the
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
fn version_four_adds_compact_fts_without_clearing_history() {
    let (root, store) = test_store();
    let mut session = sample_session();
    session.analyses[0].summary.name = "legacy person".into();
    let mut record = sample_record(42, Some(1));
    record.hotel_name = "legacy hotel".into();
    session.records = vec![record];
    session.stats.records = 1;
    store.save(&session).unwrap();

    let connection = store.connection().unwrap();
    connection
        .execute_batch(
            "INSERT INTO records_search_fts(rowid, search_text, session_id, uid)
               SELECT rowid, search_text, session_id, uid FROM records;
             INSERT INTO people_search_fts(rowid, search_text, session_id, person_key)
               SELECT rowid, search_text, session_id, person_key FROM people;
             DROP TABLE records_search_fts_v2;
             DROP TABLE people_search_fts_v2;
             PRAGMA user_version = 4;",
        )
        .unwrap();
    drop(connection);
    drop(store);

    let migrated = SessionStore::open(root.clone()).unwrap();
    assert_eq!(migrated.list().unwrap().len(), 1);
    let version: i64 = migrated
        .connection()
        .unwrap()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, DATABASE_VERSION);

    let mut people_query = query();
    people_query.search = "legacy".into();
    assert_eq!(
        migrated
            .query_people("session-1", &people_query)
            .unwrap()
            .total,
        1
    );
    let records = migrated
        .query_imported_records(
            "session-1",
            &ImportedRecordsQuery {
                search: "legacy".into(),
                page: 1,
                page_size: 50,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(records.total, 1);
    assert_eq!(records.items[0].uid, 42);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn explain_query_plan_uses_indexes_on_fast_paths() {
    // Smoke check that FTS search and normalized region contains filters remain
    // session-bounded through the existing split-column/child-table indexes.
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

    // 2. household substring confirmation scans only the indexed session partition.
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

    // 3. hotel-region substring confirmation stays inside the correlated child rows.
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
            .contains("sqlite_autoindex_person_hotel_regions")
            || plan_text.to_lowercase().contains("using index"),
        "expected person_hotel_regions PRIMARY KEY seek, got: {plan_text}"
    );

    // 4. imported-record household confirmation uses the split-column index partition.
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
                household_province: "zzz,lph,lph".into(),
                page: 1,
                page_size: 50,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items.len(), 1);

    session.records[0].household_province = "beta".into();
    session.records[0].household_region = "beta city county".into();
    store.save(&session).unwrap();

    let page = store
        .query_imported_records(
            "session-1",
            &ImportedRecordsQuery {
                household_province: "zzz,lph".into(),
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
                household_province: "et,zzz".into(),
                page: 1,
                page_size: 50,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items.len(), 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn split_filter_terms_supports_all_separators_and_deduplicates() {
    assert_eq!(
        split_filter_terms(" 安徽,浙 江，江苏、四川;重庆；北京\n天津\r上海；安 徽 "),
        vec!["安徽", "浙江", "江苏", "四川", "重庆", "北京", "天津", "上海"]
    );
    assert_eq!(split_filter_terms("An Hui, an hui"), vec!["anhui"]);
}

#[test]
fn household_filters_use_normalized_substring_and_multi_value_or() {
    let (root, store) = test_store();
    store.save(&sample_session()).unwrap();

    let mut matched = query();
    matched.household_province = "浙江，徽 省".into();
    matched.household_city = "杭州、山 市".into();
    matched.household_county = "西湖\n门 县".into();
    assert_eq!(store.query_people("session-1", &matched).unwrap().total, 1);

    let mut matched = query();
    matched.household_province = "浙江，江苏".into();
    assert_eq!(store.query_people("session-1", &matched).unwrap().total, 0);

    let page = store
        .query_imported_records(
            "session-1",
            &ImportedRecordsQuery {
                hotel_province: "浙江,徽省".into(),
                hotel_city: "杭州；山市".into(),
                hotel_county: "西湖\r\n门县".into(),
                household_province: "浙江,徽省".into(),
                household_city: "杭州；山市".into(),
                household_county: "西湖\r\n门县".into(),
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
                household_province: "浙江,江苏".into(),
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
fn exclude_household_multi_value_substrings_take_negation_correctly() {
    let (root, store) = test_store();
    store.save(&sample_session()).unwrap();

    let mut matched = query();
    matched.exclude_household_province = "浙江,徽省".into();
    assert_eq!(store.query_people("session-1", &matched).unwrap().total, 0);

    let mut matched = query();
    matched.exclude_household_province = "浙江,江苏".into();
    assert_eq!(store.query_people("session-1", &matched).unwrap().total, 1);

    let page = store
        .query_imported_records(
            "session-1",
            &ImportedRecordsQuery {
                exclude_household_county: "西湖；门县".into(),
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
    let mut session = sample_session();
    session.analyses[0].summary.hotel_regions.push(HotelRegion {
        province: "浙江省".into(),
        city: "杭州市".into(),
        county: "西湖区".into(),
        region: "浙江省杭州市西湖区".into(),
    });
    store.save(&session).unwrap();
    let mut matched = query();
    matched.hotel_search = "旅A，商务B".into();
    matched.hotel_province = "江苏，徽省".into();
    matched.hotel_city = "南京；山市".into();
    matched.hotel_county = "西湖\n门县".into();
    assert_eq!(store.query_people("session-1", &matched).unwrap().total, 1);

    matched.hotel_province = "安徽".into();
    matched.hotel_city.clear();
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
            "records_search_fts_v2",
            session_one_record_rowid,
            session_two_record_rowid,
        ),
        (
            "people_search_fts_v2",
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
fn records_only_load_does_not_decode_stale_analysis_payloads() {
    let (root, store) = test_store();
    store.save(&sample_session()).unwrap();
    let connection = store.connection().unwrap();
    connection
        .execute(
            "UPDATE people SET summary_json = x'00' WHERE session_id = 'session-1'",
            [],
        )
        .unwrap();
    drop(connection);

    let (metadata, records) = store.load_records("session-1").unwrap();
    assert_eq!(metadata.session_id, "session-1");
    assert_eq!(records.len(), 1);
    assert!(store.load("session-1").is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn replacing_analysis_preserves_records_and_record_indexes() {
    let (root, store) = test_store();
    let session = analyzed_session(vec![sample_record(1, Some(1)), sample_record(2, Some(2))]);
    store.save(&session).unwrap();
    let before_storage = record_storage_snapshot(&store, "session-1");

    let settings = AnalysisSettings {
        frequency_mode: FrequencyMode::Selected,
        frequency_start: chrono::NaiveDate::from_ymd_opt(2026, 7, 2)
            .unwrap()
            .and_hms_opt(0, 0, 0),
        frequency_end: chrono::NaiveDate::from_ymd_opt(2026, 7, 2)
            .unwrap()
            .and_hms_opt(23, 59, 59),
        ..Default::default()
    };
    let (mut analyses, stats) = analyze_records(&session.records, &settings, None);
    analyses[0].summary.name = "替换姓名".into();
    store
        .replace_analysis(
            "session-1",
            CURRENT_SCHEMA_VERSION,
            &settings,
            &analyses,
            &stats,
        )
        .unwrap();

    assert_eq!(record_storage_snapshot(&store, "session-1"), before_storage);
    let loaded = store.load("session-1").unwrap();
    assert_eq!(
        serde_json::to_value(&loaded.settings).unwrap(),
        serde_json::to_value(&settings).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&loaded.stats).unwrap(),
        serde_json::to_value(&stats).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&loaded.analyses).unwrap(),
        serde_json::to_value(&analyses).unwrap()
    );
    assert_eq!(loaded.records.len(), 2);

    let mut old_name_query = query();
    old_name_query.search = "测试人员2".into();
    assert_eq!(
        store
            .query_people("session-1", &old_name_query)
            .unwrap()
            .total,
        0
    );
    let mut new_name_query = query();
    new_name_query.search = "替换姓名".into();
    assert_eq!(
        store
            .query_people("session-1", &new_name_query)
            .unwrap()
            .total,
        1
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_analysis_replacement_rolls_back_the_previous_analysis() {
    let (root, store) = test_store();
    let session = analyzed_session(vec![sample_record(1, Some(1)), sample_record(2, Some(2))]);
    store.save(&session).unwrap();
    let before = serde_json::to_value(store.load("session-1").unwrap()).unwrap();
    let before_storage = record_storage_snapshot(&store, "session-1");

    let mut invalid_analyses = session.analyses.clone();
    invalid_analyses.push(invalid_analyses[0].clone());
    let changed_settings = AnalysisSettings {
        week_threshold: 1,
        ..Default::default()
    };
    assert!(store
        .replace_analysis(
            "session-1",
            CURRENT_SCHEMA_VERSION,
            &changed_settings,
            &invalid_analyses,
            &session.stats,
        )
        .is_err());

    assert_eq!(
        serde_json::to_value(store.load("session-1").unwrap()).unwrap(),
        before
    );
    assert_eq!(record_storage_snapshot(&store, "session-1"), before_storage);
    let mut search = query();
    search.search = "测试人员1".into();
    assert_eq!(store.query_people("session-1", &search).unwrap().total, 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "synthetic full-save versus analysis-only reanalysis benchmark"]
fn benchmark_analysis_only_replacement() {
    let people_count = std::env::var("MAIYIN_REANALYSIS_BENCH_PEOPLE")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(20_000);
    let records_count = std::env::var("MAIYIN_REANALYSIS_BENCH_RECORDS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(25_000)
        .max(people_count);
    let session = reanalysis_benchmark_session(people_count, records_count);
    let (full_root, full_store) = test_store();
    let (partial_root, partial_store) = test_store();
    full_store.save(&session).unwrap();
    partial_store.save(&session).unwrap();
    let settings = AnalysisSettings {
        week_threshold: 2,
        month_threshold: 8,
        year_threshold: 100,
        ..Default::default()
    };

    let full_total_started = Instant::now();
    let full_load_started = Instant::now();
    let mut full_session = full_store.load(&session.session_id).unwrap();
    let full_load_elapsed = full_load_started.elapsed();
    let full_analysis_started = Instant::now();
    let (full_analyses, full_stats) = analyze_records(&full_session.records, &settings, None);
    let full_analysis_elapsed = full_analysis_started.elapsed();
    full_session.schema_version = CURRENT_SCHEMA_VERSION;
    full_session.settings = settings.clone();
    full_session.analyses = full_analyses;
    full_session.stats = full_stats;
    let full_persist_started = Instant::now();
    full_store.save(&full_session).unwrap();
    let full_persist_elapsed = full_persist_started.elapsed();
    let full_total_elapsed = full_total_started.elapsed();

    let partial_total_started = Instant::now();
    let partial_load_started = Instant::now();
    let (_, records) = partial_store.load_records(&session.session_id).unwrap();
    let partial_load_elapsed = partial_load_started.elapsed();
    let partial_analysis_started = Instant::now();
    let (partial_analyses, partial_stats) = analyze_records(&records, &settings, None);
    let partial_analysis_elapsed = partial_analysis_started.elapsed();
    let partial_persist_started = Instant::now();
    partial_store
        .replace_analysis(
            &session.session_id,
            CURRENT_SCHEMA_VERSION,
            &settings,
            &partial_analyses,
            &partial_stats,
        )
        .unwrap();
    let partial_persist_elapsed = partial_persist_started.elapsed();
    let partial_total_elapsed = partial_total_started.elapsed();

    let full_result = full_store.load(&session.session_id).unwrap();
    let partial_result = partial_store.load(&session.session_id).unwrap();
    assert_eq!(
        serde_json::to_value(&full_result.settings).unwrap(),
        serde_json::to_value(&partial_result.settings).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&full_result.stats).unwrap(),
        serde_json::to_value(&partial_result.stats).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&full_result.analyses).unwrap(),
        serde_json::to_value(&partial_result.analyses).unwrap()
    );
    let reduction = if full_total_elapsed.is_zero() {
        0.0
    } else {
        (1.0 - partial_total_elapsed.as_secs_f64() / full_total_elapsed.as_secs_f64()) * 100.0
    };
    println!(
        "reanalyze_benchmark records={} people={} full_load_ms={} full_analysis_ms={} full_persist_ms={} full_total_ms={} load_records_ms={} analysis_ms={} persist_analysis_ms={} total_ms={} reduction_percent={reduction:.1}",
        records_count,
        people_count,
        full_load_elapsed.as_millis(),
        full_analysis_elapsed.as_millis(),
        full_persist_elapsed.as_millis(),
        full_total_elapsed.as_millis(),
        partial_load_elapsed.as_millis(),
        partial_analysis_elapsed.as_millis(),
        partial_persist_elapsed.as_millis(),
        partial_total_elapsed.as_millis(),
    );
    fs::remove_dir_all(full_root).unwrap();
    fs::remove_dir_all(partial_root).unwrap();
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
    let imported = import_paths(&paths, None).unwrap();
    let parse_elapsed = parse_started.elapsed();
    let analysis_started = Instant::now();
    let (analyses, stats) = analyze_records(&imported.records, &AnalysisSettings::default(), None);
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
                frequency_window_count: 1,
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
    // split-column normalized contains, hotel jurisdiction normalized contains,
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
                    frequency_window_count: 1,
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

    // 2. people household_province contains.
    let mut q = query();
    q.household_province = "安徽".into();
    let started = Instant::now();
    let _page = reopened.query_people("filter-bench", &q).unwrap();
    let household_ms = started.elapsed().as_millis();

    // 3. imported-records household_province contains.
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

    // 4. imported-records hotel_jurisdiction contains.
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
        "people={} records={} save_ms={} fts5_search_ms={} household_contains_ms={} records_household_ms={} records_hotel_ms={} fuzzy_hotel_ms={}",
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
