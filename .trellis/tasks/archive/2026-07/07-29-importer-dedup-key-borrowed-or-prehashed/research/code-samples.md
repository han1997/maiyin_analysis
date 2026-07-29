# Code Samples: importer dedup key

> 采集自 2026-07-29 源码。所有行号以当日源码为准（审计 2026-07-24 引用的行号已漂移）。

## 1. DeduplicationKey 结构体与 DateKey 枚举

`src-tauri/src/importer.rs:55-73`

```rust
#[derive(Debug, Hash, PartialEq, Eq)]
pub(crate) struct DeduplicationKey {
    id_no: String,
    hotel_name: String,
    province: String,
    city: String,
    county: String,
    region: String,
    address: String,
    room_no: String,
    check_in: DateKey,
    check_out: DateKey,
}

#[derive(Debug, Hash, PartialEq, Eq)]
pub(crate) enum DateKey {
    Parsed(NaiveDateTime),
    Raw(String),
}
```

要点：
- 8 个 `String` 字段 + 2 个 `DateKey`，全部 owned。
- `DateKey::Raw(String)` 在时间未解析时持有一份原始字符串副本。

## 2. deduplication_key 构造器（热路径核心）

`src-tauri/src/importer.rs:841-860`

```rust
pub(crate) fn deduplication_key(record: &Record) -> DeduplicationKey {
    DeduplicationKey {
        id_no: record.id_no.clone(),
        hotel_name: record.hotel_name.clone(),
        province: record.province.clone(),
        city: record.city.clone(),
        county: record.county.clone(),
        region: record.region.clone(),
        address: record.address.clone(),
        room_no: record.room_no.clone(),
        check_in: date_key(record.check_in, &record.check_in_text),
        check_out: date_key(record.check_out, &record.check_out_text),
    }
}

pub(crate) fn date_key(parsed: Option<NaiveDateTime>, raw: &str) -> DateKey {
    parsed
        .map(DateKey::Parsed)
        .unwrap_or_else(|| DateKey::Raw(raw.trim().to_string()))
}
```

要点：
- 对 record 的 8 个 String 字段逐个 `clone()`。
- `date_key` 在 raw 分支额外 `raw.trim().to_string()`（一次小字符串分配）。
- 每条 record 调用一次 → 453k 记录约 453 万次 String clone（8 字段 × 453k）。

## 3. 生产热路径：merge_parsed_files

`src-tauri/src/importer.rs:151-177`

```rust
fn merge_parsed_files(
    files: &[PathBuf],
    parsed: Vec<ParsedFile>,
) -> Result<ImportedData, AppError> {
    let total_records = parsed.iter().map(|file| file.records.len()).sum();
    let mut stats = ImportStats::default();
    let mut records = Vec::with_capacity(total_records);
    let mut seen = HashSet::with_capacity(total_records);
    let mut uid = 1_u64;
    let mut reasons = Vec::new();
    for parsed_file in parsed {
        stats.short_stay_count += parsed_file.stats.short_stay_count;
        stats.missing_id_count += parsed_file.stats.missing_id_count;
        if let Some(reason) = parsed_file.reason {
            reasons.push(reason);
        }
        for mut record in parsed_file.records {
            let key = deduplication_key(&record);
            if !seen.insert(key) {
                stats.duplicate_count += 1;
                continue;
            }
            record.uid = uid;
            records.push(record);
            uid += 1;
        }
    }
    // ...
}
```

要点：
- `seen: HashSet<DeduplicationKey>`，按 `total_records` 预分配容量。
- 内层循环对每条 record 构造键并 `insert`——dedup key 的主热路径。

## 4. 合并会话热路径：commands.rs merge_sessions

`src-tauri/src/commands.rs:222-243`

```rust
let metadata = tauri::async_runtime::spawn_blocking(move || {
    let mut combined = Vec::new();
    let mut seen = HashSet::new();
    let mut duplicate_count = 0;
    let mut short_stay_count = 0;
    let mut missing_id_count = 0;
    let mut file_count = 0;
    for session_id in &session_ids {
        let (metadata, records) = store.load_records(session_id)?;
        file_count += metadata.file_count;
        short_stay_count += metadata.import_stats.short_stay_count;
        missing_id_count += metadata.import_stats.missing_id_count;
        for mut record in records {
            let key = importer::deduplication_key(&record);
            if !seen.insert(key) {
                duplicate_count += 1;
                continue;
            }
            record.uid = combined.len() as u64 + 1;
            combined.push(record);
        }
    }
    // ...
});
```

要点：
- 复用 `importer::deduplication_key`，跨会话去重。
- 合并多个 453k 级会话时，键构造开销随合并规模线性放大。

## 5. 对比：analyze_records 不使用 DeduplicationKey

`src-tauri/src/analysis.rs:74-89`

```rust
pub fn analyze_records(
    records: &[Record],
    settings: &AnalysisSettings,
    on_progress: Option<&(dyn Fn(usize, usize) + Send + Sync)>,
) -> (Vec<PersonAnalysis>, AnalysisStats) {
    let mut grouped: HashMap<&str, Vec<&Record>> = HashMap::new();
    let mut scoped_records = 0;
    let mut issues = 0;
    for record in records {
        if !within_analysis_time_window(record, settings) {
            continue;
        }
        scoped_records += 1;
        issues += usize::from(!record.issues.is_empty());
        grouped.entry(&record.person_key).or_default().push(record);
    }
    // ...
}
```

要点：
- 分组键是 `&record.person_key`（`&str`，**借用，零分配**），与 DeduplicationKey 无关。
- 证明 dedup key 不在分析路径。

## 6. 对比：detect_dense_day_overlaps 的键

`src-tauri/src/analysis.rs:139-215`（节选键定义部分）

```rust
fn detect_dense_day_overlaps(
    records: &[&Record],
    day_index: usize,
    days: &mut [DayAnalysis],
    location_cache: &mut HashMap<u64, (String, String)>,
) {
    // ...
    let mut active_groups: HashMap<(String, String), usize> = HashMap::new();
    let mut active_by_end: BTreeMap<NaiveDateTime, Vec<usize>> = BTreeMap::new();
    let mut involved: HashSet<u64> = HashSet::new();
    // ...
    let group_key = location_cache
        .entry(record.uid)
        .or_insert_with(|| (compact(&record.hotel_name), compact(&record.room_no)))
        .clone();
    // ...
}
```

要点：
- 同住宿分组键 `HashMap<(String, String), usize>`，key = `(compact(hotel_name), compact(room_no))`。
- 涉及人员集合 `HashSet<u64>`（uid），扫描线 `BTreeMap<NaiveDateTime, Vec<usize>>`。
- 全程不构造 DeduplicationKey。

## 7. importer 基准：benchmark_synthetic_multi_file_import_merge

`src-tauri/src/importer.rs:1058-1123`

```rust
#[test]
#[ignore = "synthetic multi-file import benchmark"]
fn benchmark_synthetic_multi_file_import_merge() {
    let root = temp_root();
    fs::create_dir_all(&root).unwrap();
    let files = std::env::var("MAIYIN_BENCH_FILES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(15);
    let rows_per_file = std::env::var("MAIYIN_BENCH_ROWS_PER_FILE")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(10_000);
    let paths = (0..files)
        .map(|file_index| {
            let path = root.join(format!("import-{file_index:02}.csv"));
            write_synthetic_import_csv(&path, file_index, rows_per_file);
            path
        })
        .collect::<Vec<_>>();

    let parse_started = std::time::Instant::now();
    let parsed = paths
        .par_iter()
        .map(|path| parse_file(path))
        .collect::<Vec<_>>()
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let parse_elapsed = parse_started.elapsed();

    let old_started = std::time::Instant::now();
    let old = merge_parsed_files_baseline(&paths, parsed.clone()).unwrap();
    let old_merge_elapsed = old_started.elapsed();

    let new_started = std::time::Instant::now();
    let new = merge_parsed_files(&paths, parsed).unwrap();
    let new_merge_elapsed = new_started.elapsed();

    assert_eq!(old.records.len(), new.records.len());
    assert_eq!(old.stats.duplicate_count, new.stats.duplicate_count);
    assert_eq!(
        old.records
            .iter()
            .map(|record| (record.uid, record.source_file.clone(), record.id_no.clone()))
            .collect::<Vec<_>>(),
        new.records
            .iter()
            .map(|record| (record.uid, record.source_file.clone(), record.id_no.clone()))
            .collect::<Vec<_>>()
    );
    let reduction = 1.0
        - new_merge_elapsed.as_secs_f64() / old_merge_elapsed.as_secs_f64().max(f64::EPSILON);
    println!(
        "files={} rows_per_file={} records={} duplicates={} parse_ms={} old_merge_ms={} new_merge_ms={} merge_reduction_percent={:.1}",
        files,
        rows_per_file,
        new.records.len(),
        new.stats.duplicate_count,
        parse_elapsed.as_millis(),
        old_merge_elapsed.as_millis(),
        new_merge_elapsed.as_millis(),
        reduction * 100.0,
    );
    fs::remove_dir_all(root).unwrap();
}
```

## 8. importer 基准的 baseline：旧拼接字符串键

`src-tauri/src/importer.rs:1192-1212`

```rust
fn baseline_deduplication_key(record: &Record) -> String {
    [
        record.id_no.clone(),
        record.hotel_name.clone(),
        record.province.clone(),
        record.city.clone(),
        record.county.clone(),
        record.region.clone(),
        record.address.clone(),
        record.room_no.clone(),
        baseline_date_key(record.check_in, &record.check_in_text),
        baseline_date_key(record.check_out, &record.check_out_text),
    ]
    .join("\u{1f}")
}

fn baseline_date_key(parsed: Option<chrono::NaiveDateTime>, raw: &str) -> String {
    parsed
        .map(|value| format!("dt:{}", value.format("%Y-%m-%dT%H:%M:%S")))
        .unwrap_or_else(|| format!("raw:{}", raw.trim()))
}
```

要点：
- baseline 仍是"逐字段 clone + `\u{1f}` join"，即 spec 明确列为 Wrong 的范式。
- 现有基准对比的是"拼接键 vs 结构化键"，**不是**"owned 结构化键 vs 借用/预哈希键"。
- 因此要度量审计建议的"借用键/预哈希键"收益，需在此基准新增第三个分支或新建微基准。

## 9. importer 基准：benchmark_parallel_file_parsing（仅 parse，不涉 dedup）

`src-tauri/src/importer.rs:1021-1056`

```rust
#[test]
#[ignore = "requires MAIYIN_BENCH_FILE"]
fn benchmark_parallel_file_parsing() {
    let path = PathBuf::from(
        std::env::var("MAIYIN_BENCH_FILE").expect("set MAIYIN_BENCH_FILE to a source file"),
    );
    let copies = std::env::var("MAIYIN_BENCH_COPIES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(15);
    let files = vec![path; copies];
    let sequential_started = std::time::Instant::now();
    let sequential = files
        .iter()
        .map(|path| parse_file(path))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let sequential_elapsed = sequential_started.elapsed();
    let parallel_started = std::time::Instant::now();
    let parallel = files
        .par_iter()
        .map(|path| parse_file(path))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let parallel_elapsed = parallel_started.elapsed();
    let reduction = 1.0
        - parallel_elapsed.as_secs_f64() / sequential_elapsed.as_secs_f64().max(f64::EPSILON);
    println!(
        "files={} sequential_ms={} parallel_ms={} reduction_percent={:.1}",
        copies,
        sequential_elapsed.as_millis(),
        parallel_elapsed.as_millis(),
        reduction * 100.0,
    );
    assert_eq!(sequential.len(), parallel.len());
}
```

要点：仅测顺序 vs 并行 `parse_file`，不进入 merge/dedup 路径，与 dedup key 优化无关。

## 10. spec 范式：tauri-contract.md 的 Wrong / Correct

`.trellis/spec/backend/tauri-contract.md:288-303`

```rust
// Wrong
let key = fields.join("\u{1f}");
// This clones fields and allocates one large separator string per imported row.

// Correct
let key = DeduplicationKey { id_no, hotel_name, check_in, check_out, /* ... */ };
// Structured keys preserve equality semantics while avoiding the extra joined-string
// allocation in the merge/dedup hot path.
```

要点：spec 的 Correct 范式仍用 owned 字段；当前实现已满足。审计 #10 是在此之上的进一步探讨，spec 未要求也未禁止。
