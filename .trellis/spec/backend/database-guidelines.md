# Database Guidelines

## Scenario: versioned SQLite history storage

### 1. Scope / Trigger

This contract applies whenever local history persistence, person-result queries,
session deletion, storage-root movement, reanalysis, merge, or export changes.
The application supports hundreds of thousands of stays and people, so ordinary
history browsing must never deserialize one full session object.

### 2. Signatures

```rust
SessionStore::open(storage_root: PathBuf) -> Result<SessionStore, AppError>
SessionStore::save(session: &StoredSession) -> Result<SessionMetadata, AppError>
SessionStore::metadata(session_id: &str) -> Result<SessionMetadata, AppError>
SessionStore::query_people(session_id: &str, query: &PersonQuery) -> Result<PersonPage, AppError>
SessionStore::query_imported_records(session_id: &str, query: &ImportedRecordsQuery) -> Result<ImportedRecordsPage, AppError>
SessionStore::person_detail(session_id: &str, person_key: &str) -> Result<PersonDetail, AppError>
SessionStore::load(session_id: &str) -> Result<StoredSession, AppError>
SessionStore::move_to(destination_root: PathBuf) -> Result<SessionStore, AppError>
```

The database is `<storageRoot>/MaiyinAnalysisData/history-v1.sqlite3` and uses
`PRAGMA user_version = 2`. The file name remains stable while `user_version`
owns schema compatibility.

### 3. Contracts

- `sessions` stores lightweight metadata, settings, statistics, and source-session IDs.
- `records` stores one normalized imported record per row as a JSON payload keyed by
  `(session_id, uid)`. It also stores nullable `check_in` text in
  `%Y-%m-%d %H:%M:%S` format and indexes `(session_id, check_in, uid)` so the raw view
  can count, time-filter, sort, and page without decoding the full session. Records
  are loaded in full only for reanalysis, merge, or export.
- `people` stores query columns plus one `PersonSummary` JSON payload. Normalized hotel
  names, shared-stay hotel regions, and alerts live in child tables.
- Saves use one SQLite transaction and prepared statements. Replacing a session first
  deletes its prior rows inside the same transaction, so a later failure rolls back to
  the previous complete session.
- Ordinary result browsing performs filter, count, deterministic sort, and pagination
  in SQLite. The sort key is `score DESC, total_records DESC, name ASC, person_key ASC`.
- Multiple hotel terms are split on comma, Chinese comma, enumeration comma, semicolon,
  or newline. Each term becomes an ordered fuzzy `LIKE` pattern and every term must
  match one normalized hotel row.
- Province/city/county hotel filters are evaluated inside one correlated region row.
- Database version `1` is explicitly cleared and rebuilt as version `2`; the user chose
  re-import over migration. Any other nonzero unsupported `user_version` is rejected.
  Legacy JSON session files and `index.json` are not read or migrated.
- Hidden combined sessions are persisted only to support paginated queries and are
  replaced by the next save, preventing unbounded transient-session accumulation.
- Storage-root changes checkpoint WAL, copy the database through a temporary file, and
  refuse to overwrite an existing destination database.

### 4. Validation & Error Matrix

| Condition | Result |
| --- | --- |
| Missing session or person | `session_not_found` or `validation_error` |
| Unsupported nonzero database version other than `1` | `storage_error` naming both versions |
| Duplicate row or serialization failure during save | Transaction rolls back; prior session remains readable |
| Page size below 1 or above 500 | Clamp to `1..=500` |
| Missing record check-in | Exclude it from imported-record pages and counts |
| Database `user_version = 1` | Drop the old application tables, create schema version `2`, and return an empty history list |
| Destination already contains `history-v1.sqlite3` | `storage_error`; never overwrite it |
| Legacy JSON files exist beside the database | Ignore them; do not import or delete them automatically |

### 5. Good / Base / Bad Cases

- Good: loading a 453k-record history reads metadata, then returns only the requested
  50-person page and total count.
- Good: opening imported records returns one 50-row `ImportedRecordsPage`; start/end
  boundaries are evaluated against indexed `check_in` values in SQLite.
- Good: `A，B` creates two correlated hotel `EXISTS` clauses and requires both.
- Base: export calls `load` in a blocking worker and reconstructs the full session only
  because the export format needs all rows.
- Bad: adding `Vec<PersonSummary>` back to `WorkspaceSnapshot` or decoding every record
  during `load_session`.
- Bad: implementing the raw-record view with `store.records(session_id)` followed by a
  Rust iterator filter, because JSON decode and IPC again scale with the whole history.
- Bad: copying the live database without a WAL checkpoint or overwriting a destination
  database selected by the user.

### 6. Tests Required

- Round-trip session metadata, people, alerts, and records through SQLite.
- Assert person count, page size, stable ordering, multi-hotel AND fuzzy matching, and
  same-row jurisdiction matching.
- Assert imported-record total, `1..=500` page-size clamping, stable
  `check_in ASC, uid ASC` ordering, time boundaries, and missing-check-in exclusion.
- Set a populated database to `user_version = 1`, reopen it, and assert history is empty
  and `user_version = 2`.
- Assert household include/exclude, age, gender, risk, alert-state, and search behavior.
- Inject a duplicate-key save failure and assert the previous session remains intact.
- Delete the active session and assert the next listed session becomes active.
- Move storage and assert the copied database can list and fully load the session.
- Keep ignored release benchmarks for 453,506-person first-page opening and 15-file
  parallel parsing; record measured output in the active task research artifact.

### 7. Wrong vs Correct

#### Wrong

```rust
let session: StoredSession = serde_json::from_slice(&fs::read(path)?)?;
let people = session.analyses.into_iter().map(|item| item.summary).collect();
```

#### Correct

```rust
let metadata = store.activate(&session_id)?;
let page = store.query_people(&session_id, &query)?;
let records = store.query_imported_records(&session_id, &records_query)?;
```

The first path scales work with the entire history before the UI can render. The second
keeps startup work bounded by session metadata and the requested page.
