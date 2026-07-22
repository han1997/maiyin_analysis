# Import Performance Benchmark

## Environment

- Date: 2026-07-22
- Command: `cargo test --release --manifest-path src-tauri/Cargo.toml benchmark_synthetic_multi_file_import_merge -- --ignored --nocapture`
- Dataset: synthetic CSV import workload, 15 files x 20,000 rows per file
- Env:
  - `MAIYIN_BENCH_FILES=15`
  - `MAIYIN_BENCH_ROWS_PER_FILE=20000`
- Build: Rust release test profile

## Result

```text
files=15 rows_per_file=20000 records=297200 duplicates=2800 parse_ms=530 old_merge_ms=948 new_merge_ms=289 merge_reduction_percent=69.4
```

## Interpretation

- Parse is already parallel and was not the dominant bottleneck in this workload.
- The largest bottleneck was the post-parse merge/dedup/UID assignment pass.
- Replacing the old joined-string dedup key with a structured hash key and preallocating
  merge containers reduced merge/dedup time from 948ms to 289ms.
- Deterministic output is checked in the benchmark by comparing record count,
  duplicate count, and `(uid, source_file, id_no)` identity against the old merge path.
