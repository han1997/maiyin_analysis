# Import and history performance research

## Observed workload

- Real stored session: `556,023,959` bytes.
- Session index: 15 files, 453,506 valid records, 352,948 people.
- Memory-mapped section estimate:
  - records: about 365.5 MB
  - analyses: about 190.5 MB
- Sequential disk read of the session file took about 293 ms on the current machine, so storage bandwidth is not the dominant cost.

## Current bottlenecks

### Import

1. `importer::import_paths` parses supported files one after another.
2. Each workbook is materialized as `Vec<Vec<String>>`, then transformed into `Record` values.
3. `commands::import_paths` offloads parsing with `spawn_blocking`, but performs `analyze_records` synchronously after the await.
4. Session persistence serializes the full records and analyses into one large JSON allocation before writing.

### History open

1. `SessionStore::load` reads and deserializes the entire 556 MB JSON file.
2. Records and analyses are loaded together even though the initial history view needs only metadata and the first result page.
3. `WorkspaceSnapshot.people` clones every `PersonSummary` and sends all 352,948 summaries through Tauri IPC.
4. React filters and paginates only after receiving the entire collection.

## Comparable patterns

### Indexed SQLite storage

- Persist records and person analyses in indexed tables.
- Insert imports in a transaction with prepared/batched statements.
- Query only the requested result page and total count.
- Load evidence/raw records only when detail or export needs them.
- Strengths: bounded memory, native pagination/filtering, durable migration story.
- Costs: largest refactor; query schema and migration tests are required.

### Split binary session files

- Store small metadata separately from records and analyses.
- Use a compact serde binary format; load analyses first and raw records lazily.
- Add a backend query command so IPC transfers one page rather than all summaries.
- Strengths: smaller change than relational storage, fast sequential decode.
- Costs: custom indexing/migration; filtering still scans in-memory analyses; binary compatibility needs explicit versioning.

### Minimal concurrency-only changes

- Parse independent files in parallel and move analysis/serialization to blocking workers.
- Strengths: lowest implementation risk.
- Costs: does not address the 556 MB history decode or 352,948-person IPC transfer, so history opening remains structurally slow.

## Feasible approaches

### A. SQLite + backend pagination (recommended)

- Introduce a versioned local SQLite database under `MaiyinAnalysisData`.
- Keep history index metadata lightweight.
- Store records, person summaries, alerts, hotel names/regions, and evidence mappings in indexed tables.
- Replace `WorkspaceSnapshot.people` with a paginated people response and add a backend query command.
- Parallelize independent file parsing, then batch persist and analyze off the UI command thread.
- Auto-import legacy JSON sessions on first open, retaining the original until migration succeeds.

This removes both dominant history costs and provides the best path for million-row data.

### B. Split binary files + backend pagination

- Retain session-file ownership but split metadata, analyses, and records.
- Load analyses with a compact binary decoder; load records lazily.
- Query/filter pages in Rust and transfer only page results.
- Parallelize file parsing and offload analysis/storage.

This should be substantially faster, but it creates a custom storage/index layer with fewer query capabilities than SQLite.

### C. Concurrency-only patch

- Parallel file parsing.
- Move analysis and JSON save/load to blocking workers.
- Keep current snapshot and JSON schema.

This improves import responsiveness but cannot adequately optimize large-history opening.

## Recommendation

Choose approach A when large sessions like the observed 453k-record data are a normal workload. It is the only option that avoids loading and transferring the entire session during ordinary browsing. Use approach B only if adding SQLite is unacceptable. Do not select C as the final solution for the stated history-opening requirement.

## Implemented benchmark results

Release-mode measurements on the development machine after the SQLite and Rayon implementation:

- Synthetic history with 453,506 people: SQLite save took 7,050 ms; reopening the database, reading session metadata, counting results, and returning the first 50-person page took 58 ms.
- Fifteen parses of the available 4.7 MB legacy `.xls` source: sequential parsing took 138 ms; bounded Rayon parsing took 35 ms, a 74.4% reduction.
- The available local `.xls` fixture contained only one recognized record, so it validates the end-to-end adapter path but is not a substitute for rerunning the benchmark with the user's full 15-file source set after re-import.

Reproducible ignored Rust benchmarks:

- `storage::tests::benchmark_large_history_first_page`
- `importer::discovery_tests::benchmark_parallel_file_parsing`
- `storage::tests::benchmark_real_import_pipeline`
