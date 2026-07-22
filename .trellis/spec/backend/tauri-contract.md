# Tauri and frontend contract

## Scenario: local analysis workspace

### 1. Scope / Trigger

This contract applies to changes crossing the React WebView, Tauri commands, Rust analysis modules, local session storage, or export adapters. The current application crosses all five boundaries, so this contract is mandatory for future changes.

### 2. Signatures

Rust commands are coarse-grained and return serializable DTOs:

```rust
bootstrap_workspace() -> Result<WorkspaceSnapshot, CommandError>
import_paths(paths: Vec<String>) -> Result<WorkspaceSnapshot, CommandError>
import_folders(paths: Vec<String>) -> Result<WorkspaceSnapshot, CommandError>
load_session(session_id: String) -> Result<WorkspaceSnapshot, CommandError>
merge_sessions(session_ids: Vec<String>) -> Result<WorkspaceSnapshot, CommandError>
reanalyze(settings: AnalysisSettings) -> Result<WorkspaceSnapshot, CommandError>
query_people(query: PersonQuery) -> Result<PersonPage, CommandError>
get_person_detail(person_key: String) -> Result<PersonDetail, CommandError>
get_imported_records(query: ImportedRecordsQuery) -> Result<ImportedRecordsPage, CommandError>
export_result(kind: String, path: String) -> Result<OperationResult, CommandError>
```

The TypeScript `AppApi` mirrors these operations. Browser mode implements the same interface with fixture data and never claims that fixture data was parsed from a local file.

### 3. Contracts

Request rules:

- `paths` contains absolute user-selected files or folders. Supported extensions are `.xls`, `.xlsx`, and `.csv`.
- `session_ids` must contain at least two values for merge.
- `AnalysisSettings.monthThreshold` is an integer in `1..9999`; `yearThreshold` is an integer in `1..99999`.
- `export_result.path` is selected by the native save dialog. The backend creates only the selected file and its parent directory.

Response rules:

- `WorkspaceSnapshot` contains `mode`, `stats`, `sessions`, `settings`, `importStats`, `sourceSessionIds`, and `generatedAt`; it never contains the full people collection.
- `PersonPage` contains only `items`, `total`, `page`, and `pageSize` for the applied backend query.
- `PersonDetail` contains one `person`, its rule `alerts`, and on-demand `evidence` rows.
- `ImportedRecordsPage` contains only `items`, `total`, `page`, and `pageSize`; each item
  is an `ImportedStayRecord` inside the current analysis check-in boundary.
- `CommandError` always serializes `{ code: string, message: string }`; the UI displays `message` and does not expose Rust internals.
- Dates crossing the boundary are strings. Rust owns parsing; TypeScript only formats valid display strings.
- Risk `level` and alert `severity` are explicit text values. Color is presentation only.

Storage rules:

- Sessions use the versioned SQLite database documented in [`database-guidelines.md`](./database-guidelines.md).
- `storage.json` under the Tauri app-data directory remembers a user-selected storage root.
- Startup does not automatically show the last session. SQLite metadata is read for history, and the user explicitly loads a session.
- Legacy JSON history is not migrated or read; users re-import the original source files.

### 4. Validation & Error Matrix

| Condition | Rust result | UI behavior |
| --- | --- | --- |
| Unsupported extension or empty file selection | `validation_error` | Keep workspace unchanged and show an inline toast |
| No recognizable id/time columns | `empty_import` | Explain which files were skipped |
| All rows are duplicates or under 10 minutes | `empty_import` | Explain the exclusion reason |
| Fewer than two merge sessions | `validation_error` | Disable merge and reject direct command |
| Invalid time range or threshold | `validation_error` | Keep parameter panel open and identify the field |
| Result-filter minimum age exceeds maximum age | No Rust call | Keep the current list and show a validation toast |
| Missing session/person | `session_not_found` or `validation_error` | Show a retryable error, never crash the shell |
| Export path canceled | No Rust call | Show a cancellation message without error styling |
| Legacy `.xls` text differs from reference | Compatibility failure | Do not claim parity; route only `.xls` through a future narrow adapter |

### 5. Good / Base / Bad cases

- Good: the UI calls `reanalyze(settings)` once, Rust recomputes all four alert types, persists the non-combined session, and returns one fresh snapshot.
- Base: browser preview calls the same `AppApi` method and returns clearly labeled demo data without reading file bytes.
- Bad: React filters a subset and invents a new risk score that differs from Rust, or transfers each spreadsheet row through IPC.
- Good: `get_person_detail` returns evidence only for the selected person and the UI opens a right-side inspector.
- Bad: logs or toasts include full identity numbers, phone numbers, or raw workbook contents.

### 6. Tests Required

- Rust unit tests for overlap requiring different hotel/room, same-day non-overlap count, rolling 30/365-day thresholds, score caps, and risk level boundaries.
- Rust importer tests for title rows before headers, fixed template positions, decorated headers, inferred id/time columns, compact/Excel/text dates, short-stay and duplicate exclusion, and CSV BOM/GBK decoding.
- Rust storage tests for SQLite round-trip, paginated filters, transaction rollback, active-session deletion, and storage-root copying.
- Export tests for UTF-8 BOM, full identity values, formula-injection prefixing, and risk workbook rows.
- TypeScript tests for search across identity/household/alert text, level/alert filters, and first render of browser preview.
- Cross-layer assertions must verify camelCase DTO fields and structured `{ code, message }` errors.

### 7. Wrong vs Correct

#### Wrong

```ts
const score = rows.length > 6 ? 30 : 0;
```

This duplicates a partial business rule in the WebView and will drift from Rust.

#### Correct

```ts
const snapshot = await appApi.reanalyze(settings);
setSnapshot(snapshot);
```

Rust owns normalization, grouping, scoring, persistence, and the returned explanation. React only renders the contract.

## Design Decisions

- Rust is the authoritative backend because local file access, sensitive-data boundaries, batch analysis, persistence, and export belong in the desktop process.
- A browser fixture adapter is retained for fast visual development but cannot perform native operations or claim file parsing.
- Calamine is the first Rust workbook reader. Legacy `.xls` Chinese-text compatibility is an explicit fixture gate because the original application used `xlrd` for known problematic exports.

## Scenario: recursive file discovery

### 1. Scope / Trigger

Applies whenever a command accepts user-selected files or directories before workbook parsing.

### 2. Signatures

```rust
discover_supported_files(paths: &[String]) -> Result<Vec<String>, AppError>
import_folders(paths: Vec<String>) -> Result<WorkspaceSnapshot, CommandError>
```

### 3. Contracts

- Each input may be a supported file or a directory.
- Directories are recursively scanned without following directory links.
- `.xls`, `.xlsx`, and `.csv` matching is case-insensitive.
- Results are canonicalized where possible, sorted deterministically, and de-duplicated case-insensitively.
- Unsupported files are ignored. Missing paths and traversal errors are not ignored.

### 4. Validation & Error Matrix

| Condition | Result |
| --- | --- |
| Supported file path | Return that normalized file |
| Directory with nested supported files | Return all supported descendants |
| Directory with only unsupported files | `empty_import` with supported-extension guidance |
| Missing or inaccessible root | `read_error` with the affected path |
| WalkDir entry error | `read_error`; never silently discard it |

### 5. Good / Base / Bad cases

- Good: one root contains `a.CSV` and `nested/b.XLSX`; both reach the shared parser.
- Base: unsupported PDFs are skipped while valid spreadsheets continue.
- Bad: `filter_map(Result::ok)` hides access failures and produces an unexplained empty import.

### 6. Tests Required

- Temporary multi-level directory with mixed-case supported extensions.
- Direct supported file passed without directory walking.
- Unsupported files excluded.
- Missing path and traversal failures produce structured errors.

### 7. Wrong vs Correct

#### Wrong

```rust
WalkDir::new(path).into_iter().filter_map(Result::ok)
```

#### Correct

```rust
for entry in WalkDir::new(path).follow_links(false) {
    match entry {
        Ok(entry) => { /* filter supported files */ }
        Err(error) => failures.push(error.to_string()),
    }
}
```

## Scenario: analysis ownership and result filtering

### 1. Scope / Trigger

Applies whenever analysis settings, alert formulas, result summary fields,
detail evidence, result filters, imported-record views, history JSON, or exports change.

### 2. Signatures

```rust
reanalyze(settings: AnalysisSettings) -> Result<WorkspaceSnapshot, CommandError>
query_people(query: PersonQuery) -> Result<PersonPage, CommandError>
get_imported_records(query: ImportedRecordsQuery) -> Result<ImportedRecordsPage, CommandError>
within_analysis_time_window(record: &Record, settings: &AnalysisSettings) -> bool
```

```ts
appApi.queryPeople(query: PersonQuery): Promise<PersonPage>
appApi.getImportedRecords(query: ImportedRecordsQuery): Promise<ImportedRecordsPage>
```

### 3. Contracts

- `AnalysisSettings` contains only nullable `frequencyStart`/`frequencyEnd`
  plus thresholds for selected-window, 7-day, 30-day, and 365-day frequency.
- Hotel jurisdiction, household include/exclude, age, and gender belong to
  `PersonQuery`; changing them calls `query_people`, not `reanalyze`, and never alters scores.
- Threshold defaults are `3`, `3`, `12`, and `144` respectively.
- `get_imported_records` accepts `page` and `pageSize`, performs the analysis check-in
  boundary in SQLite, and returns only one page plus total count. Snapshots and commands
  never transfer every raw row through IPC for ordinary browsing.
- `PersonSummary` includes `maxWeekCount`, `maxMonthCount`, `maxYearCount`,
  `hotelNames`, and `hotelRegions`. Each hotel-region entry is
  `{ province, city, county, region }`; persisted additions use serde defaults.
- Hotel-name input is split on comma, Chinese comma, enumeration comma,
  semicolon, or newline. Every non-empty term must fuzzy-match at least one
  hotel name (AND across terms).
- Populated province/city/county filters must match one shared `hotelRegions`
  entry; never combine components from different stays.
- Stored session payloads use schema version `3` inside SQLite database version `2`.
  This release starts from an empty database and provides no legacy JSON upgrade path.
- React never computes scores. Selected-window and rolling frequency scoring
  remain mutually exclusive in Rust.
- Scores are: overlap `min(35, 20 + P*2 + D*5)`, same-day-many
  `min(45, 25 + (N-4)*5)`, frequency `min(80, 45 + (C-T)*6)`.

### 4. Validation & Error Matrix

| Condition | Result |
| --- | --- |
| Any threshold outside `1..=99999` | `validation_error` naming the period |
| Start boundary after end boundary | `validation_error` and keep settings UI open |
| Result-filter minimum age exceeds maximum age | Frontend toast; do not update the applied query or call Rust |
| Missing check-in | Exclude from time-window analysis |
| Old summary lacks `hotelRegions` | Deserialize to an empty list via serde default |
| SQLite `user_version = 1` | Clear application history and initialize version `2`; the user re-imports source files |
| Other nonzero unsupported SQLite version | `storage_error`; do not attempt an implicit migration |

### 5. Good / Base / Bad Cases

- Good: one selected boundary is set, all totals/evidence/exports use that
  boundary, and no rolling frequency alert scores.
- Good: `A，B` returns only people whose hotel set fuzzy-matches both terms,
  while their Rust score and alerts remain unchanged.
- Base: no result filter is active, so SQLite counts the session and returns only the requested page.
- Good: a 453k-row imported-record view returns 50 JSON payloads and one count instead
  of deserializing and sending all 453k rows.
- Bad: adding province, household, age, or gender back to `AnalysisSettings`,
  because this changes the evidence set and reintroduces slow searches.
- Bad: matching province from one stay and county from another; all populated
  jurisdiction components must match one structured hotel-region entry.

### 6. Tests Required

- Same-room overlap alerts at the base score; different room scores higher.
- Selected-window count greater than threshold produces only `window_frequency`.
- Narrow boundaries remove outside records from totals and evidence ids.
- Fuzzy hotel-name filtering matches ordered non-contiguous characters and
  multiple separators use AND semantics.
- Hotel jurisdiction tests assert same-entry province/city/county matching.
- Household include/exclude, age, gender, alert-state, and search behavior have SQLite query tests; browser fixtures retain matching TypeScript tests.
- Imported-record tests cover paging, stable time order, inclusive start/end boundaries,
  missing check-ins, and the camelCase page DTO.
- Legacy settings ignore removed analysis fields, and missing `hotelRegions` defaults safely.
- Frontend build asserts all camelCase DTO fields.

### 7. Wrong vs Correct

#### Wrong

```ts
await appApi.reanalyze({ ...settings, province: "安徽省" });
```

#### Correct

```ts
setQuery((current) => ({ ...current, hotelProvince: "安徽省", page: 1 }));
const records = await appApi.getImportedRecords({ page: 1, pageSize: 50 });
```

Result filters narrow the rendered `PersonSummary` collection. Only time and
frequency settings cross the Tauri command boundary and trigger recalculation.
