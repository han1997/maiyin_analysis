# Filter Benchmark Results

## Environment

- Date: 2026-07-22
- Command: `cargo test --release --manifest-path src-tauri/Cargo.toml benchmark_filter_latency_on_large_session -- --ignored --nocapture`
- Dataset: synthetic session, `MAIYIN_BENCH_PEOPLE=100000`, `MAIYIN_BENCH_RECORDS=1000000`
- Build: Rust release test profile

## Results

After adding `record_filter_counts` aggregate counts for safe single-field imported-record
filters:

```text
people=100000 records=1000000 save_ms=60671 fts5_search_ms=33 household_prefix_ms=169 records_household_ms=5 records_hotel_ms=4 fuzzy_hotel_ms=4
```

After FTS5 rowid fix, prefix range predicates, imported-record page index hint, and
count index hints, before aggregate counts:

```text
people=100000 records=1000000 save_ms=72306 fts5_search_ms=35 household_prefix_ms=198 records_household_ms=2298 records_hotel_ms=2307 fuzzy_hotel_ms=2558
```

Earlier release run before imported-record count hints:

```text
people=100000 records=1000000 save_ms=57522 fts5_search_ms=34 household_prefix_ms=181 records_household_ms=1952 records_hotel_ms=2000 fuzzy_hotel_ms=2691
```

Earlier release run before forcing imported-record paging through
`idx_records_check_in`:

```text
people=100000 records=1000000 save_ms=57357 fts5_search_ms=27 household_prefix_ms=166 records_household_ms=4008 records_hotel_ms=4021 fuzzy_hotel_ms=2658
```

Initial release run with LIKE-prefix predicates:

```text
people=100000 records=1000000 save_ms=57484 fts5_search_ms=28 household_prefix_ms=69 records_household_ms=2612 records_hotel_ms=2622 fuzzy_hotel_ms=2665
```

## Interpretation

- People free-text FTS5 path passes the 500ms target.
- People household prefix path passes the 500ms target.
- Imported-record household, hotel jurisdiction, and fuzzy hotel-name paths pass the
  500ms target on the synthetic 1M-record session after aggregate counts.
- The pre-aggregate bottleneck was exact `COUNT(*)` under broad 25%-selectivity filters.
  Safe single-field imported-record filters now answer total from `record_filter_counts`
  and still fetch page rows from SQLite in `check_in ASC, uid ASC` order.
