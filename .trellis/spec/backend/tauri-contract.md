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
get_person_detail(person_key: String) -> Result<PersonDetail, CommandError>
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

- `WorkspaceSnapshot` contains `mode`, `stats`, `people`, `sessions`, `settings`, `importStats`, `sourceSessionIds`, and `generatedAt`.
- `PersonDetail` contains one `person`, its rule `alerts`, and on-demand `evidence` rows.
- `CommandError` always serializes `{ code: string, message: string }`; the UI displays `message` and does not expose Rust internals.
- Dates crossing the boundary are strings. Rust owns parsing; TypeScript only formats valid display strings.
- Risk `level` and alert `severity` are explicit text values. Color is presentation only.

Storage rules:

- Sessions are versioned JSON records under `<storageRoot>/MaiyinAnalysisData/sessions`.
- `storage.json` under the Tauri app-data directory remembers a user-selected storage root.
- Startup does not automatically show the last session. The index is read for history, and the user explicitly loads a session.

### 4. Validation & Error Matrix

| Condition | Rust result | UI behavior |
| --- | --- | --- |
| Unsupported extension or empty file selection | `validation_error` | Keep workspace unchanged and show an inline toast |
| No recognizable id/time columns | `empty_import` | Explain which files were skipped |
| All rows are duplicates or under 10 minutes | `empty_import` | Explain the exclusion reason |
| Fewer than two merge sessions | `validation_error` | Disable merge and reject direct command |
| Invalid age range or threshold | `validation_error` | Keep parameter panel open and identify the field |
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
- Rust storage tests for round-trip session JSON, missing-file cleanup, explicit startup loading, merge de-duplication, and storage-root preference persistence.
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

## Scenario: upstream scoring parity

### 1. Scope / Trigger

Applies whenever analysis settings, alert formulas, result summary fields,
detail evidence, imported-record views, history JSON, or exports change.

### 2. Signatures

```rust
reanalyze(settings: AnalysisSettings) -> Result<WorkspaceSnapshot, CommandError>
get_imported_records() -> Result<Vec<ImportedStayRecord>, CommandError>
within_analysis_time_window(record: &Record, settings: &AnalysisSettings) -> bool
```

### 3. Contracts

- `AnalysisSettings` includes nullable `frequencyStart`/`frequencyEnd` plus
  thresholds for selected-window, 7-day, 30-day, and 365-day frequency.
- Defaults are `3`, `3`, `12`, and `144` respectively.
- `get_imported_records` is loaded only when the records tab opens and returns
  only records inside the current scope and check-in boundary; snapshots do
  not transfer every raw row through IPC.
- `PersonSummary` includes `maxWeekCount`, `maxMonthCount`, `maxYearCount`, and
  `hotelNames`; newly added persisted summary fields use serde defaults.
- React never computes scores. It submits settings once and renders Rust DTOs.
- Selected-window frequency and rolling frequency are mutually exclusive.
- Scores are: overlap `min(35, 20 + P*2 + D*5)`, same-day-many
  `min(45, 25 + (N-4)*5)`, frequency `min(80, 45 + (C-T)*6)`.

### 4. Validation & Error Matrix

| Condition | Result |
| --- | --- |
| Any threshold outside `1..=99999` | `validation_error` naming the period |
| Start boundary after end boundary | `validation_error` and keep settings UI open |
| Missing check-in | Exclude from time-window analysis |
| Old history lacks new settings/summary fields | Load serde defaults and reanalyze normally |

### 5. Good / Base / Bad Cases

- Good: one selected boundary is set, all totals/evidence/exports use that
  boundary, and no rolling frequency alert scores.
- Base: no boundary is set, rolling 7/30/365-day alerts may independently score.
- Bad: React filters imported records by time while Rust detail/export retains
  out-of-window records.

### 6. Tests Required

- Same-room overlap alerts at the base score; different room scores higher.
- Selected-window count greater than threshold produces only
  `window_frequency`.
- Narrow boundaries remove outside records from totals and evidence ids.
- Fuzzy hotel-name filtering matches ordered non-contiguous characters.
- Frontend build asserts all new camelCase DTO fields.

### 7. Wrong vs Correct

#### Wrong

```ts
const score = selectedRows.length > threshold ? 45 : 0;
```

#### Correct

```ts
const snapshot = await appApi.reanalyze(draftSettings);
setSnapshot(snapshot);
```
