# 优化导入性能

## Goal

Optimize the data import path so large multi-file imports finish faster without changing analysis results, imported-record semantics, or the Tauri command contract.

## What I already know

- User request: "优化导入性能".
- Previous completed task optimized history/result filtering, not import performance.
- `import_paths` in `src-tauri/src/importer.rs` already parses files in parallel with Rayon:
  `files.par_iter().map(parse_file)`.
- After parallel parse, `import_paths` still performs a single-thread merge pass for stats,
  global duplicate detection, deterministic UID assignment, and final `Vec<Record>` construction.
- Tauri `commands::import_paths` runs import, analysis, and SQLite save inside one
  `spawn_blocking` closure.
- Existing importer benchmark: ignored test `benchmark_parallel_file_parsing` compares
  sequential vs parallel `parse_file` over repeated file copies.
- Existing storage benchmark covers `import_paths` + `analyze_records` + `store.save` +
  first page query when `MAIYIN_BENCH_FILES` is provided.

## Assumptions (temporary)

- The target bottleneck is the import pipeline after file selection: workbook/CSV parsing,
  cross-file duplicate removal, UID assignment, analysis handoff, and initial save.
- Result correctness and deterministic ordering are more important than maximizing parallelism.
- This task should not change result-filter performance; that was handled separately.

## Open Questions

- None.

## Requirements (evolving)

- Preserve deterministic imported record ordering and UID assignment.
- Preserve duplicate/short-stay/missing-id statistics.
- Preserve existing Tauri command signatures and `WorkspaceSnapshot` response shape.
- Add or update benchmark artifacts that compare before/after import performance.
- MVP benchmark target: multi-file import throughput, around 15 real workbook/CSV files.
- MVP scope includes both file parsing throughput and the post-parse merge/dedup/UID
  assignment pass.
- MVP includes internal stage timing/benchmark observability for parse and merge/dedup,
  but does not add UI progress or cancellation.

## Acceptance Criteria (evolving)

- [ ] Existing importer, analysis, storage, frontend tests remain green.
- [ ] A representative multi-file import benchmark is recorded under `research/`.
- [ ] Import optimization keeps record ordering, duplicate count, and UID assignment deterministic.
- [ ] Benchmark identifies the largest parse/merge bottleneck and the implementation
  removes or materially reduces it without adding disproportionate complexity.
- [ ] The merge/dedup pass remains deterministic across repeated imports of the same file list.
- [ ] Benchmark output separates parse time from merge/dedup time.

## Definition of Done (team quality bar)

- Tests added/updated for changed import behavior.
- `cargo test`, `npm run lint`, `npm test`, and `npm run build` pass.
- Specs updated if importer/storage contracts change.
- Benchmark notes record dataset, command, before/after numbers, and interpretation.

## Out of Scope (explicit)

- Single huge-file-specific parser redesign unless discovered as the dominant blocker for
  the multi-file benchmark.
- Analysis scoring and SQLite save optimization unless benchmark evidence shows they
  dominate the selected multi-file import scenario.
- UI progress indicators and cancellation controls.
- Changing analysis scoring semantics.
- Changing `WorkspaceSnapshot` shape or frontend pagination behavior.
- Reworking result filter/query performance.
- Migrating legacy history formats.

## Technical Notes

- Likely files:
  - `src-tauri/src/importer.rs`
  - `src-tauri/src/commands.rs`
  - `src-tauri/src/analysis.rs` if import/analysis handoff is involved
  - `src-tauri/src/storage.rs` if initial save dominates the import wall clock
- Relevant specs:
  - `.trellis/spec/backend/tauri-contract.md`
  - `.trellis/spec/backend/database-guidelines.md`
- Existing benchmark hooks:
  - `cargo test --manifest-path src-tauri/Cargo.toml benchmark_parallel_file_parsing -- --ignored --nocapture`
  - `cargo test --manifest-path src-tauri/Cargo.toml benchmark_real_import_pipeline -- --ignored --nocapture`
- Benchmark artifact:
  - [`research/benchmark.md`](research/benchmark.md) records the synthetic 15-file import
    benchmark and old/new merge comparison.

## Technical Approach

- Establish a baseline on the selected multi-file import workload.
- Instrument benchmark output to separate parse time and merge/dedup time.
- Optimize the largest observed importer bottleneck first. The selected implementation
  splits merge/dedup into `merge_parsed_files`, preallocates merge containers, and uses a
  structured `DeduplicationKey` instead of joining cloned fields into one separator
  string.
- Keep analysis and SQLite save changes out of scope unless the benchmark proves they
  dominate the chosen multi-file scenario.

## Decision (ADR-lite)

**Context**: The previous performance task optimized history/filter browsing and explicitly
left import-path performance for a separate task.

**Decision**: Optimize multi-file import throughput, including parsing and the post-parse
merge/dedup/UID assignment pass. Add internal timing observability in benchmarks, but do
not add UI progress/cancellation in this MVP.

**Consequences**: The implementation can touch importer data structures and benchmark
tests, but must preserve deterministic record ordering, duplicate counts, and UID
assignment. Success is based on finding and materially reducing the largest benchmarked
bottleneck rather than a fixed percentage target.
