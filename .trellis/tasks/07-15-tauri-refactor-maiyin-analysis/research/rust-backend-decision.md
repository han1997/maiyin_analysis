# Rust Backend Decision

## Decision summary

Use React/TypeScript for presentation and interaction state, with Rust as the authoritative backend for local file access, parsing, normalization, analysis, session persistence, and export.

Do not maintain a second production implementation of the risk rules in TypeScript. The browser-only Vite preview may use typed fixture data and a mock adapter, but it must consume the same DTO shape as Tauri commands.

## Why Rust fits this application

- Tauri already exposes Rust commands to the webview through serialized command arguments and return values.
- Sensitive identity and accommodation data remains inside the desktop process and does not require a local HTTP service.
- File traversal, parsing, deduplication, sliding-window analysis, persistence, and export are cohesive backend responsibilities.
- CPU-heavy work can run outside the UI thread and report coarse progress events without transferring every spreadsheet row repeatedly across the IPC boundary.
- A Rust backend avoids bundling a general Python runtime or a full Python sidecar for normal operation.
- The rule engine is deterministic and testable, which suits Rust unit and integration tests well.

## Recommended boundary

### Rust owns

- Native file and folder selection integration.
- Recursive file discovery and supported-extension validation.
- `.xlsx`, `.xls`, and multi-encoding CSV parsing.
- Header detection, fixed-template fallback, field inference, normalization, date parsing, deduplication, and short-stay exclusion.
- Jurisdiction/person filters, overlap analysis, rolling-window counts, scoring, and risk levels.
- Session history, storage-directory migration, merging, deletion, and durable schema versions.
- CSV/XLSX/template export and formula-injection hardening.
- Progress, warning, and structured error events.

### React/TypeScript owns

- Application shell, controls, table, inspector, keyboard interaction, and accessible status feedback.
- Query controls and view state; expensive filtering/pagination can be delegated to Rust when data volume requires it.
- Presentation formatting only, never a second copy of scoring rules.
- A browser preview adapter backed by fixtures so `npm run dev` remains useful without Rust.

## IPC design

- Prefer coarse commands such as `import_paths`, `reanalyze`, `list_sessions`, `load_session`, `merge_sessions`, and `export_result`.
- Return compact person summaries and page data instead of the full normalized record set on every action.
- Fetch evidence/detail records on demand for the selected person.
- Use typed, versioned DTOs shared with TypeScript generation or checked manually through contract tests.
- Run long commands asynchronously and emit progress keyed by an operation id.

## Main compatibility risk: legacy `.xls`

The original project intentionally uses `xlrd` for `.xls` because some hotel-industry exports reportedly produce corrupted text when read through Calamine. This means a pure-Rust parser cannot be considered equivalent until tested against representative real files.

Plan:

1. Implement the Rust parser behind a format adapter and port the existing sheet-selection tests.
2. Add sanitized real-world `.xls` fixtures that cover the known Chinese-text problem.
3. Accept the Rust path only when parsed cell text and detected core fields match the legacy implementation.
4. If parity fails, isolate a compatibility reader for `.xls` only, preferably a packaged converter/helper with a narrow JSON or temporary-file contract. Keep analysis, persistence, and export in Rust.

This compatibility escape hatch is narrower and more maintainable than preserving the entire Python application as a sidecar.

## Current ecosystem check

Checked on 2026-07-16:

- Tauri `2.11.5`
- Calamine `0.36.0`
- rust_xlsxwriter `0.96.0`
- csv `1.4.0`
- encoding_rs `0.8.35`
- chardetng `1.0.0`
- serde `1.0.228`

Version numbers are research inputs, not hard pins; implementation should select mutually compatible current releases and commit the lockfile.

## Consequences

- More initial work than a TypeScript-only port and Rust must be installed before native verification.
- Cleaner production architecture, stronger desktop boundary, and no duplicated business rules.
- Browser preview remains available, but real import/export/history operations require Tauri runtime.
- Legacy `.xls` compatibility is an explicit acceptance gate rather than an assumed property of the chosen crate.

