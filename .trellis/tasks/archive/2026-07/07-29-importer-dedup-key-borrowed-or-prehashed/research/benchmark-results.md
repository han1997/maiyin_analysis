# Benchmark Results: importer dedup key (owned structured key)

> Phase 1 of task `07-29-importer-dedup-key-borrowed-or-prehashed`.
> Goal: establish whether the current structured owned `DeduplicationKey`
> (8 `String` clones + 2 `DateKey` per record) is a bottleneck at ~453k-record
> scale, **before** deciding whether to prototype a borrowed-key variant.

## Setup

- **Benchmark function**: `benchmark_synthetic_multi_file_import_merge`
  (`src-tauri/src/importer.rs:1058-1123`), `#[ignore]`-gated.
- **What it measures**:
  - `parse_ms` — Rayon parallel `parse_file` over the synthetic CSVs.
  - `old_merge_ms` — `merge_parsed_files_baseline` (joined-string key via
    `fields.join("\u{1f}")`, the spec-listed "Wrong" pattern).
  - `new_merge_ms` — `merge_parsed_files` (current production structured owned
    `DeduplicationKey`, the spec-listed "Correct" pattern).
  - `merge_reduction_percent` — `1 - new/old` for the merge phase.
  - Asserts `records` / `duplicates` / `(uid, source_file, id_no)` identity
    match between the two merge paths.
- **Environment variables used**:
  - `MAIYIN_BENCH_FILES=46`
  - `MAIYIN_BENCH_ROWS_PER_FILE=10000`
  - → 46 × 10_000 = 460_000 generated rows.
- **Build**: `cargo test --manifest-path src-tauri/Cargo.toml --release`
  (release profile, optimized). Release build: ~1m 10s (first run, cached
  afterwards).
- **Run command** (Windows PowerShell):
  ```powershell
  $env:MAIYIN_BENCH_FILES=46; $env:MAIYIN_BENCH_ROWS_PER_FILE=10000
  cargo test --manifest-path src-tauri/Cargo.toml --release -- `
    --ignored benchmark_synthetic_multi_file_import_merge --nocapture
  ```
- **Toolchain**: cargo/rustc 1.96.0 (stable).
- **Machine**: developer workstation (Windows).

## Raw output per run

All four runs produced identical record/duplicate counts:
`records=455500`, `duplicates=4500`.
(460_000 generated rows − 4_500 cross-file duplicates = 455_500 retained
records. Duplicates come from `row % 100 == 0` rows whose `duplicate_bucket`
collapses to `0` across all files — see `write_synthetic_import_csv`.)

### Run 1 (includes cold release build, ~1m 10s)

```
files=46 rows_per_file=10000 records=455500 duplicates=4500
parse_ms=516 old_merge_ms=1092 new_merge_ms=424 merge_reduction_percent=61.2
```

### Run 2 (warm build cache)

```
files=46 rows_per_file=10000 records=455500 duplicates=4500
parse_ms=459 old_merge_ms=1101 new_merge_ms=410 merge_reduction_percent=62.7
```

### Run 3 (warm build cache)

```
files=46 rows_per_file=10000 records=455500 duplicates=4500
parse_ms=469 old_merge_ms=1044 new_merge_ms=411 merge_reduction_percent=60.6
```

### Run 4 (warm build cache)

```
files=46 rows_per_file=10000 records=455500 duplicates=4500
parse_ms=465 old_merge_ms=1055 new_merge_ms=403 merge_reduction_percent=61.8
```

## Summary table

| Run | parse_ms | old_merge_ms | new_merge_ms | reduction% |
|-----|----------|--------------|--------------|------------|
| 1   | 516      | 1092         | 424          | 61.2       |
| 2   | 459      | 1101         | 410          | 62.7       |
| 3   | 469      | 1044         | 411          | 60.6       |
| 4   | 465      | 1055         | 403          | 61.8       |

## Averages (across 4 runs)

| Metric                   | Average | Min | Max | Spread (max−min) |
|--------------------------|---------|-----|-----|------------------|
| `parse_ms`               | 477     | 459 | 516 | 57               |
| `old_merge_ms`           | 1073    | 1044| 1101| 57               |
| `new_merge_ms`           | 412     | 403 | 424 | 21               |
| `merge_reduction_percent`| 61.6    | 60.6| 62.7| 2.1              |

The key metric — `new_merge_ms` (current structured owned-key merge time) —
is **extremely stable**: 403–424 ms across runs, a 21 ms spread (~5% of the
mean). The structured key already cuts merge time ~62% versus the joined-string
baseline at this scale.

## Notes on the produced record count

- Target was ~453k records. Achieved **455_500 retained records** after
  deduplication of 460_000 generated rows (4_500 duplicates).
- This is ~0.5% above the target and well within the "large-scale ~453k"
  intent. The benchmark clearly exercises the dedup hot path at the intended
  scale.
- Per the `findings.md` audit, 455_500 records × 8 `String` fields = ~3.64 M
  `String::clone` calls during `new_merge` (plus 2 `DateKey` constructions
  per record).

## Observations relevant to the bottleneck decision

1. **`new_merge_ms` is small in absolute terms**: ~412 ms for 455.5 k records
   of full merge+dedup work (8 clones + 2 DateKeys + HashSet insert + Vec push
   + uid assignment per record).
2. **Synthetic `parse_ms` is artificially fast** (~477 ms): the benchmark
   generates plain CSVs in memory, writes them to disk, then parses them back.
   Real-world imports parse `.xlsx`/`.xls` via Calamine (and the BIFF fallback),
   which is orders of magnitude slower (tens of seconds for 453 k rows). So
   the synthetic `new_merge / (parse + new_merge) ≈ 46%` ratio OVERstates
   merge's share of real import time.
3. **Most clone overhead is free in the synthetic data**: the synthetic CSV
   has only 5 columns (姓名, 身份证号码, 旅馆名称, 入住时间, 退房时间), so 6 of
   the 8 `String` fields (province, city, county, region, address, room_no)
   are empty strings. Cloning an empty `String` in Rust allocates nothing (it
   copies the dangling-sentinel pointer triple). Only `id_no` (18 bytes) and
   `hotel_name` ("旅馆N", 4–8 bytes) are non-empty, and the two dates parse to
   `DateKey::Parsed` (no string allocation). So the synthetic benchmark
   UNDERestimates clone overhead relative to fully-populated real data — but
   even with that caveat, the absolute `new_merge_ms` is small.
4. **The structured key already beats the joined-string baseline by ~62%**
   at this scale, confirming the spec "Correct" pattern is delivering.

See `bottleneck-decision.md` for the documented go/no-go decision on Phase 2.
