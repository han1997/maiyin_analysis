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
SessionStore::delete(session_id: &str) -> Result<Option<SessionMetadata>, AppError>
SessionStore::move_to(destination_root: PathBuf) -> Result<SessionStore, AppError>
```

The database is `<storageRoot>/MaiyinAnalysisData/history-v1.sqlite3` and uses
`PRAGMA user_version = 4`. The file name remains stable while `user_version`
owns schema compatibility.

### 3. Contracts

- `sessions` stores lightweight metadata, settings, statistics, and source-session IDs.
- `records` stores one normalized imported record per row as a JSON payload keyed by
  `(session_id, uid)`. It also stores nullable `check_in` text in
  `%Y-%m-%d %H:%M:%S` format and indexes `(session_id, check_in, uid)` so the raw view
  can count, time-filter, sort, and page without decoding the full session. Structured
  filter columns (`name_norm`, `id_no_norm`, `phone_norm`, `hotel_name_norm`,
  `hotel_province_norm`, `hotel_city_norm`, `hotel_county_norm`,
  `household_region_norm`, `household_province_norm`, `household_city_norm`,
  `household_county_norm`, `age`, `gender`, `search_text`) are populated from the
  `Record` fields at save time so the imported-record view can filter in SQLite
  without decoding `record_json`. Records are loaded in full only for reanalysis,
  merge, or export.
- `people` stores query columns plus one `PersonSummary` JSON payload. Normalized hotel
  names, household split columns, shared-stay hotel regions, and alerts live in child
  tables.
- Free-text search uses contentless FTS5 trigram tables (`records_search_fts` and
  `people_search_fts`) for normalized queries of three or more characters. One- and
  two-character queries keep the `LIKE '%x%'` fallback for correctness. The FTS rowid
  must mirror the real SQLite table `rowid`, not business `uid` or `person_key`;
  business IDs are session-local and are not valid FTS join keys.
- Contentless FTS rows must be deleted by their mirrored source-table `rowid` before
  deleting the corresponding `records`, `people`, or hotel rows. Their UNINDEXED
  `session_id` value is not a reliable read/delete predicate, so
  `DELETE FROM <fts> WHERE session_id = ?` can silently leave documents behind.
- Hotel and household jurisdiction filters use normalized split columns with prefix
  range semantics (`column >= x AND column < x || max_unicode`) so B-tree indexes can
  serve them reliably. Do not reintroduce
  `household_region_norm LIKE '%x%'`, `search_text LIKE '%x%'` for long queries, or
  OR conditions against concatenated region text on ordinary paginated paths.
- `record_filter_counts` stores per-session counts for non-empty, non-null-check-in
  imported records by `filter_kind` and normalized value. It may answer exact totals
  only for safe single-field imported-record filters without selected time windows or
  other narrowing filters; all combined filters fall back to normal SQLite `COUNT(*)`.
  Replacing a session must replace these counts in the same save transaction.
- Saves use one SQLite transaction and prepared statements. Replacing a session first
  deletes its prior rows inside the same transaction, so a later failure rolls back to
  the previous complete session.
- Ordinary result browsing performs filter, count, deterministic sort, and pagination
  in SQLite. The sort key is `score DESC, total_records DESC, name ASC, person_key ASC`.
- Multiple hotel terms are split on comma, Chinese comma, enumeration comma, semicolon,
  or newline. Each term becomes an ordered fuzzy `LIKE` pattern and every term must
  match one normalized hotel row.
- Province/city/county hotel filters are evaluated inside one correlated region row.
- Database versions `1`, `2`, and `3` are cleared and rebuilt as version `4` — the user
  chose re-import over migration. `initialize_schema` calls `reset_legacy_database` for
  any legacy version, which drops all application tables and FTS tables, resets
  `user_version = 0`, then recreates the v4 schema, rather than backfilling columns from
  `record_json`. Any other nonzero unsupported `user_version` is rejected. Legacy JSON
  session files and `index.json` are not read or migrated.
- Schema changes prefer "clear old data + re-import" over writing migration/backfill
  code. A backfill that scans `record_json` for hundreds of thousands of rows holds a
  write transaction and blocks the Tauri main thread (synchronous `bootstrap_workspace`),
  producing a white screen; clearing is instantaneous. `reset_legacy_database` is the
  single clear-and-rebuild routine reused by every legacy `user_version` branch.
- Hidden combined sessions are persisted only to support paginated queries and are
  replaced by the next save, preventing unbounded transient-session accumulation. Their
  FTS rows must be removed before the session cascade runs.
- Storage access uses one shared read/write lock across `SessionStore` clones. Queries
  take a read guard; save, activate, delete, and storage movement take a write guard so a
  database-file reset cannot race an open query connection.
- Deleting the final listed session checkpoints WAL, removes the database plus exact
  `-wal`, `-shm`, and rollback-journal sidecars, then recreates an empty version-4 schema.
  Transient unlisted combined sessions are discarded with it. When other listed sessions
  remain, delete only the target session: remove its FTS rows by source rowid, explicitly
  clear current child tables, preserve other sessions, and select the next active session.
- Opening an empty database larger than 8 MiB best-effort rebuilds it. This repairs old
  logical deletes that left a multi-gigabyte file, freelist pages, or orphaned FTS data;
  cleanup failure must not turn an otherwise readable startup into a fatal error.
- Storage-root changes checkpoint WAL, copy the database through a temporary file, and
  refuse to overwrite an existing destination database.

### 4. Validation & Error Matrix

| Condition | Result |
| --- | --- |
| Missing session or person | `session_not_found` or `validation_error` |
| Unsupported nonzero database version other than `1`, `2`, or `3` | `storage_error` naming both versions |
| Duplicate row or serialization failure during save | Transaction rolls back; prior session remains readable |
| Page size below 1 or above 500 | Clamp to `1..=500` |
| Missing record check-in | Exclude it from imported-record pages and counts |
| Database `user_version = 1` | Drop the old application tables, create schema version `4`, and return an empty history list |
| Database `user_version = 2` | Drop the old application tables, create schema version `4`, and return an empty history list |
| Database `user_version = 3` | Drop the old application and FTS tables, create schema version `4`, and return an empty history list |
| Destination already contains `history-v1.sqlite3` | `storage_error`; never overwrite it |
| Legacy JSON files exist beside the database | Ignore them; do not import or delete them automatically |
| Delete target does not exist | `session_not_found`; preserve all rows and files |
| Delete target is the final listed session | Recreate a small empty version-4 database and return no active metadata |
| File-level reset cannot acquire/checkpoint/remove the file | Fall back to transactional row deletion when the original database remains usable |

### 5. Good / Base / Bad Cases

- Good: loading a 453k-record history reads metadata, then returns only the requested
  50-person page and total count.
- Good: opening imported records returns one 50-row `ImportedRecordsPage`; start/end
  boundaries are evaluated against indexed `check_in` values in SQLite.
- Good: `A，B` creates two correlated hotel `EXISTS` clauses and requires both.
- Good: `query.search = "祁门县"` uses FTS5 trigram and joins back by SQLite `rowid`;
  `query.search = "祁"` uses the short-query `LIKE` fallback.
- Good: `householdProvince = "安徽"` matches `安徽省`; `householdProvince = "省"` does
  not match under prefix semantics.
- Good: a single `ImportedRecordsQuery.householdProvince = "安徽"` can get `total`
  from `record_filter_counts`, then fetch rows from `records` ordered by `check_in, uid`.
- Good: deleting the only visible 1.6 GB session replaces the database file instead of
  writing hundreds of thousands of cascade tombstones; the next import saves normally.
- Base: deleting one of several listed sessions runs on the blocking worker, removes its
  base/child/FTS rows, and keeps the other sessions queryable.
- Base: export calls `load` in a blocking worker and reconstructs the full session only
  because the export format needs all rows.
- Bad: adding `Vec<PersonSummary>` back to `WorkspaceSnapshot` or decoding every record
  during `load_session`.
- Bad: implementing the raw-record view with `store.records(session_id)` followed by a
  Rust iterator filter, because JSON decode and IPC again scale with the whole history.
- Bad: copying the live database without a WAL checkpoint or overwriting a destination
  database selected by the user.
- Bad: using `uid` as `records_search_fts.rowid`; uid is only a session-local business
  value and can diverge from `records.rowid` or collide across sessions.
- Bad: using `record_filter_counts` for combined filters such as province + age, selected
  time windows, or exclude-household filters; those need exact row-level predicates.
- Bad: writing a v3→v4 backfill that scans every `record_json` row inside one startup
  transaction, because it blocks the Tauri main thread for a 453k-row history and white
  screens; clear the database instead.

- Bad: deleting contentless FTS rows with `WHERE session_id = ?`, or deleting the shared
  database file while another listed session must be retained.

### 6. Tests Required

- Round-trip session metadata, people, alerts, and records through SQLite.
- Assert person count, page size, stable ordering, multi-hotel AND fuzzy matching, and
  same-row jurisdiction matching.
- Assert imported-record total, `1..=500` page-size clamping, stable
  `check_in ASC, uid ASC` ordering, time boundaries, and missing-check-in exclusion.
- Set a populated database to `user_version = 1`, reopen it, and assert history is empty
  and `user_version = 4`.
- Set a populated database to `user_version = 2`, reopen it, and assert history is
  empty and `user_version = 4`.
- Set a populated database to `user_version = 3`, reopen it, and assert history is
  empty and `user_version = 4`.
- Assert imported-record result filters (hotel name, hotel jurisdiction, household
  include/exclude, age range, gender, keyword search) are applied in SQLite.
- Assert household include/exclude, age, gender, risk, alert-state, and search behavior.
- Assert FTS search works when a record's business `uid` does not equal its SQLite
  rowid.
- Assert replacing a session also replaces `record_filter_counts`; stale aggregate
  counts must not survive.
- Inject a duplicate-key save failure and assert the previous session remains intact.
- Delete the active session and assert the next listed session becomes active.
- Delete one of multiple sessions and assert its FTS rowids are absent while the other
  session's FTS rowids remain.
- Delete the final listed session with a transient combined session present; assert the
  store reopens empty at version 4 and accepts a new save.
- Reopen an oversized empty database and assert stale tables/data are removed and the
  physical database file shrinks.
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

#### Wrong: contentless FTS cleanup

```sql
DELETE FROM records_search_fts WHERE session_id = ?;
```

#### Correct: delete through the mirrored SQLite rowid

```sql
DELETE FROM records_search_fts
WHERE rowid IN (SELECT rowid FROM records WHERE session_id = ?);
```
