# Tauri Migration Research

## Sources

- Tauri prerequisites: https://v2.tauri.app/start/prerequisites/
- Tauri project creation: https://v2.tauri.app/start/create-project/
- Tauri dialog plugin: https://v2.tauri.app/plugin/dialog/
- Tauri file system plugin: https://v2.tauri.app/plugin/file-system/
- Tauri shell plugin: https://v2.tauri.app/plugin/shell/

## Current Environment

- Node.js is installed: `v24.14.0`.
- npm is installed: `11.9.0`.
- Rust is not currently installed: `rustc` and `cargo` are not found.
- This means Vite/React code can be scaffolded and run, but `tauri dev` and `tauri build` cannot complete until Rust is installed.

### Verification update (2026-07-16)

- Rust was installed after the initial research: `rustc 1.96.0`, `cargo 1.96.0`, stable `x86_64-pc-windows-msvc`.
- `cargo check --all-targets`, Rust tests, and Clippy with warnings denied pass.
- `tauri build --no-bundle` produces `src-tauri/target/release/maiyin-analysis.exe`.
- MSI and NSIS bundling currently fail only while downloading WiX/NSIS from GitHub due a network timeout.

## Tauri Constraints

- Official Tauri Windows prerequisites include Microsoft C++ Build Tools and Microsoft Edge WebView2 for development.
- Official Rust setup recommends `rustup`, with `winget install --id Rustlang.Rustup` available on Windows.
- Tauri project creation can use `npm create tauri-app@latest`.
- The dialog plugin provides native file open/save dialogs.
- The file-system plugin can access files from the frontend, but filesystem permissions and scopes must be explicit.
- The shell plugin can spawn child processes and is relevant if using a Python sidecar.

## Architecture Options

### Option A: Tauri + React/TypeScript, Business Logic Ported to TypeScript

How it works:
- Build a Tauri shell with a React/TypeScript frontend.
- Port the current Python import, analysis, filtering, history, and export logic into TypeScript modules.
- Use `xlsx` for `.xls`/`.xlsx`/CSV parsing and Excel/CSV export.
- Use native Tauri dialog/file-system plugins for desktop file access once Rust is installed.
- Provide browser-compatible fallback for development where possible.

Pros:
- No Python runtime or sidecar packaging.
- Cleaner long-term Tauri app shape.
- UI can be substantially improved with standard web layout, responsive panels, table density, and accessible controls.

Cons:
- Higher initial porting risk because the business logic must be reimplemented.
- Need focused parity tests for date parsing, header detection, deduplication, risk scoring, and export safety.

### Option B: Tauri + Python Sidecar

How it works:
- Keep Python analysis/export code.
- Package it as a sidecar executable and call it from Tauri through the shell plugin.
- Build a modern frontend for controls/results, using JSON IPC between frontend/Rust and Python.

Pros:
- Preserves the tested Python business logic.
- Lower business-rule regression risk in the first migration.

Cons:
- More packaging complexity.
- Needs PyInstaller sidecar management plus Tauri shell permissions.
- Harder to make the app feel like one coherent Tauri codebase.

### Option C: Tauri + Rust Backend Port

How it works:
- Port import, analysis, storage, and export to Rust.
- Expose Tauri commands to the frontend.

Pros:
- Strong desktop-native implementation.
- Good performance and clean separation between UI and local processing.

Cons:
- Highest initial cost.
- Cannot be compiled in the current environment until Rust is installed.
- Excel `.xls` compatibility and export parity require careful crate selection and tests.

## Recommendation

Proceed with Option A for the MVP:
- It produces a visible Tauri-style application fastest.
- It avoids Python sidecar packaging.
- It can run as a Vite app immediately for UI verification even before Rust is installed.
- It keeps local-only data handling as a product requirement.

Follow-up hardening can move sensitive file operations or heavy parsing behind Rust commands after the UI and parity tests stabilize.
