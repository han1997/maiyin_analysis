# 修复选择文件夹未递归导入数据文件

## Goal

修复 Tauri 桌面模式下选择文件夹后没有稳定遍历并导入其中 `.xls`、`.xlsx`、`.csv` 文件的问题，让文件选择和文件夹选择最终共享同一套导入、清洗、去重和风险分析流程。

## What I Already Know

- 前端 `src/api/tauriApi.ts` 使用 Tauri dialog 的 `open({ directory: true })`，随后调用 `import_folders`。
- Rust `src-tauri/src/commands.rs` 中 `import_folders` 调用 `importer::expand_folders`，再把返回路径传给文件导入流程。
- Rust `src-tauri/src/importer.rs` 当前使用 `WalkDir`，通过 `Path::extension().to_lowercase()` 筛选扩展名，但遍历错误会被 `filter_map(Result::ok)` 静默丢弃，空结果也没有给出扫描统计。
- 当前没有针对文件夹遍历的 Rust 测试；现有 Rust 测试覆盖风险分析规则，前端测试覆盖浏览器演示首屏。
- 工作树在本任务开始前干净，Rust stable 已可用。

## Assumptions (temporary)

- 文件夹导入应递归扫描所有子目录，扩展名大小写不敏感。
- 不支持的文件类型跳过，不应阻断有效数据文件导入。
- 选中的路径可能是文件或目录；两者都应被规范化为待导入文件列表。
- 如果目录中没有支持文件，应显示明确错误，而不是静默停留在导入状态。

## Open Questions

- None. User confirmed recursive, case-insensitive discovery with explicit empty/error feedback.

## Requirements

- 规范化 Tauri 选择结果，兼容单目录、目录列表、文件路径和 Windows 路径分隔符。
- 递归遍历目录，稳定排序候选文件，并识别 `.xls`、`.xlsx`、`.csv` 的任意大小写形式。
- 遍历过程中不静默吞掉权限或路径错误；至少返回可定位的扫描错误信息。
- 将找到的候选文件交给现有统一导入流程，保留原有表头识别、字段推断、去重、短入住过滤和风险分析逻辑。
- 在 UI 中显示扫描/导入结果，包含候选文件数；没有候选文件时给出下一步提示。
- 保持浏览器演示模式的行为明确，不把浏览器 `webkitdirectory` 伪装成真实本地解析。

## Acceptance Criteria

- [x] 选择包含根目录文件和多层子目录的临时目录，能找到并导入所有支持扩展名文件。
- [x] `.XLS`、`.XLSX`、`.CSV` 等大写扩展名能被识别。
- [x] 混有图片、PDF、临时文件时，支持文件仍能导入，非支持文件被跳过。
- [x] 空目录或无支持文件目录显示明确错误，不进入无限导入状态。
- [x] 遍历权限/路径错误能传递到前端结构化错误提示。
- [x] Rust 单元测试、Clippy、前端 lint、测试和构建通过。

## Definition of Done

- Tests added/updated for recursive discovery and edge cases.
- Rust and frontend quality checks green.
- Cross-layer contract and user-facing documentation updated if behavior changes.
- No remote API, shell command, or Python sidecar introduced.

## Technical Approach

- Add a Rust `discover_supported_files` function that accepts files or directories and returns a structured discovery result.
- Use `WalkDir` with explicit error handling, `follow_links(false)`, case-insensitive extension matching, canonicalized paths where possible, and deterministic sorting.
- Keep `import_paths` as the single parser/analyzer entry point.
- Return discovery metadata through the import command so the UI can state how many files were scanned/imported.

## Decision (ADR-lite)

**Context**: The folder button reaches a Rust command, but the current discovery helper returns only a silent `Vec<String>`, hides traversal errors, and has no test or user-visible scan result.

**Decision**: Make discovery explicit and testable in Rust, normalize both file and folder selections before parsing, and preserve one downstream import path.

**Consequences**: Directory imports become observable and diagnosable. The command payload gains discovery metadata, and permission errors are no longer silently ignored.

## Out of Scope

- Changing workbook parsing rules or risk scoring.
- Watching directories for future file changes.
- Importing unsupported formats such as PDF, JSON, or image files.
- Browser-mode parsing of local file bytes.

## Technical Notes

- Relevant files: `src/api/tauriApi.ts`, `src/api/contract.ts`, `src-tauri/src/commands.rs`, `src-tauri/src/importer.rs`, `src-tauri/src/error.rs`.
- Existing cross-layer contract: `.trellis/spec/backend/tauri-contract.md`.
