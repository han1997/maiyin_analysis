# 抽取 export_error helper 收敛重复 map_err

## Goal

`exporter.rs` 中重复 `.map_err(|error| AppError::Export(error.to_string()))` 约 14 处。storage 模块已用 `sql_error`/`storage_error` 收敛同类映射，exporter 仍逐处内联同一闭包。抽一个泛型 `export_error(e) -> AppError` helper，收敛 14 处重复。**行为不变。**

## What I already know

### 14 处重复定位

每处都是完全相同的闭包：`.map_err(|error| AppError::Export(error.to_string()))`

| # | 行 | 上下文 | 错误类型 |
|---|-----|--------|----------|
| 1 | 23 | `fs::create_dir_all(parent)` | `io::Error` |
| 2 | 39 | `fs::File::create(path)` | `io::Error` |
| 3 | 41 | `file.write_all(...)` | `io::Error` |
| 4 | 65 | `writer.write_record(header)` | `csv::Error` |
| 5 | 86 | `writer.write_record(row)` | `csv::Error` |
| 6 | 90 | `writer.flush()` | `csv::Error` |
| 7 | 114 | `writer.write_record(header)` | `csv::Error` |
| 8 | 139 | `writer.write_record(row)` | `csv::Error` |
| 9 | 142 | `writer.flush()` | `csv::Error` |
| 10 | 151 | `worksheet.set_name(...)` | `XlsxError` |
| 11 | 174 | `worksheet.write_string_with_format(...)` | `XlsxError` |
| 12 | 204 | `worksheet.write_string(...)` | `XlsxError` |
| 13 | 212 | `workbook.save(path)` | `XlsxError` |
| 14 | 217 | `fs::write(path, bytes)` | `io::Error` |

### 已有模式参考（storage.rs 998-1004）

storage 模块用具体类型 helper：
```rust
pub(crate) fn storage_error(error: std::io::Error) -> AppError {
    AppError::Storage(error.to_string())
}
pub(crate) fn sql_error(error: rusqlite::Error) -> AppError {
    AppError::Storage(error.to_string())
}
```

exporter 涉及 3 种错误类型（`io::Error`, `csv::Error`, `XlsxError`），用泛型 helper 更合适——避免为每种类型写一个 helper。

## Decisions (locked)

- **方案**：抽一个泛型 `export_error<E: std::fmt::Display>(error: E) -> AppError`，14 处 `.map_err(|error| AppError::Export(error.to_string()))` 替换为 `.map_err(export_error)`。
- **泛型而非具体类型**：3 种错误类型共享同一 `Display` 语义，泛型 helper 比写 3 个具体 helper 更简洁。
- **行为不变**：生成的 `AppError::Export` 值完全一致（都是 `error.to_string()`）。
- **质量门**：`cargo test` 45 passed / 8 ignored、`cargo fmt --check`、`cargo clippy -D warnings` 全绿。

## Requirements

- 新增 `fn export_error<E: std::fmt::Display>(error: E) -> AppError`（模块私有，无需 `pub`）。
- 14 处 `.map_err(|error| AppError::Export(error.to_string()))` 替换为 `.map_err(export_error)`。
- 不改动任何其他逻辑。

## Acceptance Criteria

- [ ] `export_error` helper 函数定义完成。
- [ ] 14 处 `.map_err(|error| AppError::Export(...))` 全部替换为 `.map_err(export_error)`。
- [ ] `cargo test` 通过数仍为 45 passed / 8 ignored。
- [ ] `cargo fmt --check` 无 diff。
- [ ] `cargo clippy --all-targets -- -D warnings` 零告警。
- [ ] `git diff` 仅 `src-tauri/src/exporter.rs` 有改动。

## Definition of Done

- 上述 AC 全部满足。
- 生成的 `AppError::Export` 值与改动前完全一致。

## Out of Scope

- 改变 `AppError::Export` 的语义或错误消息格式。
- 改变导出逻辑。
- 新增测试。

## Technical Notes

- `.map_err(export_error)` 传递函数指针给 `map_err`，Rust 从 `Result` 的错误类型推断 `E`——所有 3 种错误类型都实现了 `Display`，推断成功。
- helper 为模块私有 `fn`，不需要 `pub(crate)`，仅 exporter.rs 内部使用。
- 审计报告 P3 #13 原文：抽一个 `export_error(e) -> AppError` 即可。
