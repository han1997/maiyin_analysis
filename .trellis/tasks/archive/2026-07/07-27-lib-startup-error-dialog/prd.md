# Lib startup error dialog

## Goal

把 `src-tauri/src/lib.rs:39` 的 `tauri::Builder::...run(...).expect("failed to run maiyin analysis")` 改为结构化错误处理，避免启动失败时 raw panic（Windows 上可能触发"程序已停止工作"对话框），改善首启失败体验。

## What I already know

* 审计报告定位：`src-tauri/src/lib.rs:39`，P3 类型安全。审计原文："启动失败直接 panic 且无用户可见信息。属 Tauri 入口惯用写法，但转成日志化错误提示能改善首启失败体验。"
* 当前实现：`pub fn run()` 是 Tauri 入口（`#[cfg_attr(mobile, tauri::mobile_entry_point)]`），`tauri::Builder::default()...run(tauri::generate_context!()).expect("failed to run maiyin analysis")`。
* `.run()` 返回 `Result<(), tauri::Error>`；失败原因可能包括：`generate_context!` 编译期上下文错误、事件循环启动失败、`setup` 闭包返回 `Err`（如 `AppState::open` 失败即 storage 初始化失败）。
* `setup` 闭包内的 `AppState::open(storage_root)` 已用 `.map_err(|error| std::io::Error::other(error.to_string()))?` 把 `AppError` 转为 `io::Error` 向上传播——启动失败的根因信息已在 error 链里。
* 项目已有 `tauri-plugin-dialog = "2.7.1"`，但显示运行时对话框需要 `AppHandle`；`.run()` 返回 `Err` 时事件循环未启动，无 `AppHandle`，所以 dialog plugin 不可用于此处。
* `Cargo.toml` 无原生 messagebox 依赖（无 `windows`/`msgbox`/`native-dialog`）。
* `tauri::Error` 实现 `Display`（用 `thiserror`），可直接 `eprintln!("...: {error}")`。
* 相关审计记录：`.trellis/tasks/archive/2026-07/07-24-repo-code-audit/audit-report.md` 3.4 节第 3 条。

## Assumptions

* 不新增 GUI 对话框依赖（P3 风险收益不匹配）。
* 改善目标：避免 raw panic + 提供结构化错误输出，便于从终端启动时调试。
* 不改变 `run()` 签名（`pub fn run()` 无返回值）与 `mobile_entry_point` 属性。
* 不改变 `setup` 闭包内的错误传播逻辑。

## Open Questions

* None — 已确认采用 Approach A（stderr 日志 + exit(1)）。

## Requirements

* 移除 `.expect("failed to run maiyin analysis")`，改为结构化错误处理。
* 启动失败时输出完整错误信息（含 `tauri::Error` 的 Display），不 panic。
* 进程以非零退出码退出（`std::process::exit(1)`）。
* 不新增依赖。

## Acceptance Criteria (evolving)

* [ ] `lib.rs:39` 不再出现 `.expect(...)`。
* [ ] 启动失败时输出完整错误信息到 stderr，进程以退出码 1 退出（而非 panic）。
* [ ] `cargo fmt --check`、`cargo test`、`cargo clippy --all-targets -- -D warnings` 全部通过。

## Definition of Done

* 重构范围限于 `src-tauri/src/lib.rs`。
* 质量门全绿（Rust 三项）。
* 不改变 `run()` 签名、`setup` 闭包、`invoke_handler` 列表。

## Out of Scope

* 新增 GUI 对话框/原生 messagebox 依赖。
* 改变 `run()` 签名或 `mobile_entry_point` 属性。
* 改变 `setup` 闭包内的 `AppState::open` 错误传播。
* 引入日志框架（如 `tauri-plugin-log`）。

## Technical Approach

采用 Approach A（stderr 日志 + exit(1)）：

1. 把 `.run(tauri::generate_context!()).expect("failed to run maiyin analysis")` 改为：
   ```rust
   .run(tauri::generate_context!())
       .unwrap_or_else(|error| {
           eprintln!("failed to run maiyin analysis: {error}");
           std::process::exit(1);
       });
   ```
2. `eprintln!` 输出完整 `tauri::Error` Display（含 setup 闭包传播的 `AppState::open` 失败根因），终端启动时可调试。
3. `std::process::exit(1)` 以非零退出码退出，避免 panic unwinding（Windows 上避免触发"程序已停止工作"系统对话框）。
4. 不改 `run()` 签名、`setup` 闭包、`invoke_handler` 列表。

## Decision (ADR-lite)

**Context**: `lib.rs:39` 的 `.expect()` 在启动失败时 raw panic，无结构化错误输出；GUI 对话框在 `.run()` 失败点不可用（无 `AppHandle`）。

**Decision**: 采用 Approach A —— `.unwrap_or_else()` 输出完整错误到 stderr 后 `std::process::exit(1)`。不新增依赖。

**Consequences**: 避免 panic unwinding 与系统崩溃对话框；从终端启动时开发者可见完整错误链；GUI 模式下 stderr 对终端用户不可见（P3 限制，后续若引入日志框架可再增强）。改动局限于 `lib.rs` 一行。

## Technical Notes

* 主要文件：`src-tauri/src/lib.rs`（`run()` 13-40，`.expect` 在 39）。
* `.run()` 返回 `Result<(), tauri::Error>`，`tauri::Error: Display`。
* 质量命令：`cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo test --manifest-path src-tauri/Cargo.toml`、`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`。
