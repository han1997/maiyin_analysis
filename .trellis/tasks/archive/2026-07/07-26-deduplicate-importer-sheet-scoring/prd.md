# Deduplicate importer sheet scoring

## Goal

抽取 `read_workbook`（calamine）与 `read_legacy_xls`（rxls）中重复的"逐 sheet 打分择优"逻辑为共享 helper，消除两条路径的结构重复，行为不变。

## What I already know

* 审计报告定位：`src-tauri/src/importer.rs:313-361`（`read_workbook`）与 `363-406`（`read_legacy_xls`），P3 代码质量。
* 两个函数都以同样结构遍历 sheet、调用 `detect_template_data_start` / `detect_header_row` / `infer_core_fields`，并用 `best_score` / `best_rows` 兜底。
* 重复的核心是单 sheet 的 4 步判定 + best-score 兜底追踪：
  1. `detect_template_data_start(&rows).is_some()` → 立即接受
  2. `detect_header_row(&rows)` → id_no 与 check_in 列非空 → 立即接受
  3. `infer_core_fields(&rows, indexes).is_some()` → 立即接受
  4. 否则按 `score` 追踪 `best_rows`，循环结束兜底返回
* 两者差异仅在：返回类型（`Result<Vec<...>>` vs `Result<Option<Vec<...>>>`）、sheet→rows 的转换源（calamine `worksheet_range` vs rxls `cells` + `legacy_cells_to_rows`）、以及 `read_workbook` 末尾的 `.xls` legacy 回退分支。
* `read_workbook` 的 per-sheet `worksheet_range` 可能出错并 `map_err` 短路；提前 `return Ok(rows)` 会停止读取剩余 sheet（惰性 + 早退）。
* 测试覆盖：`discovery_tests` 模块覆盖 `legacy_cells_to_rows`、`parse_file`、文件发现与合并；但 `read_workbook` / `read_legacy_xls` 的择优逻辑无直接单测，行为靠集成路径间接保证。
* 相关审计记录：`.trellis/tasks/archive/2026-07/07-24-repo-code-audit/audit-report.md` 3.3 节。

## Assumptions

* 重构为纯行为保持 refactor，不改变任何 sheet 选取/兜底语义、错误消息或返回顺序。
* 早退（找到合格 sheet 即停止读剩余 sheet）与 per-sheet 错误短路语义需保留。
* 不引入新依赖，不动 calamine / rxls 调用边界。

## Open Questions

* None — 已确认采用 Approach A（共享编排器）。

## Requirements

* 抽取共享择优 helper，`read_workbook` 与 `read_legacy_xls` 各自只负责把 sheet 转成行后调用它。
* 保留早退、per-sheet 错误传播、best-score 兜底三类语义。
* 不改变对外行为：同一输入文件产生相同 rows、相同错误、相同 sheet 选择。

## Acceptance Criteria (evolving)

* [ ] `read_workbook` 与 `read_legacy_xls` 的 4 步判定 + best-score 兜底逻辑收敛到单一 helper。
* [ ] 行为不变：现有测试全绿，无新增失败。
* [ ] `cargo fmt --check`、`cargo test`、`cargo clippy --all-targets -- -D warnings` 全部通过。

## Definition of Done

* 重构范围限于 `src-tauri/src/importer.rs`。
* 质量门全绿（Rust 三项）。
* 不改变 calamine / rxls 调用边界与错误消息文本。

## Out of Scope

* 改变 sheet 选取规则或打分权重。
* 改变 calamine / rxls 读取方式或新增解析后端。
* 重构 importer 中 `parse_file` / `resolve_data_start_and_indexes` / `build_record` 等其他部分。
* 新增对其它文件格式的支持。

## Technical Approach

采用 Approach A（共享编排器）：

1. 新增 `score_and_pick_sheet(sheets: impl Iterator<Item = Result<Vec<Vec<String>>, AppError>>) -> Result<Option<Vec<Vec<String>>>, AppError>`，封装单 sheet 的 4 步判定 + best-score 兜底 + 早退 + per-sheet 错误短路。
2. `read_workbook`：把 calamine `sheet_names()` → `worksheet_range` → rows 的转换包成 `Result<rows, AppError>` 迭代器，调 `score_and_pick_sheet`；返回 `Some` 即用，返回 `None` 再走 `.xls` 的 `read_legacy_xls` 回退，最后 `AppError::Empty`。
3. `read_legacy_xls`：把 rxls `sheets` → `cells` → `legacy_cells_to_rows` 的转换包成 `Result<rows, AppError>` 迭代器（`legacy_cells_to_rows` 返回 `None` 时产出空 Vec，由 helper 的空行跳过等价处理），直接返回 `score_and_pick_sheet` 的 `Ok(Option<...>)`。
4. 不改 `legacy_cells_to_rows`、`detect_*`、`infer_core_fields` 等被调函数；sheet 迭代顺序、错误消息文本、早退语义全部保持。

## Decision (ADR-lite)

**Context**: `read_workbook` 与 `read_legacy_xls` 的 4 步单 sheet 判定 + best-score 兜底循环结构近乎全等，任一择优语义调整需在两处同步，易漂移。

**Decision**: 采用 Approach A —— 抽一个接收 `Iterator<Item = Result<rows, AppError>>` 的共享编排器 `score_and_pick_sheet`，把判定 + 兜底 + 早退 + 错误短路全部收敛；两个 caller 只负责把各自 sheet 源转成该迭代器。

**Consequences**: 去重最大化，择优语义单点维护；helper 签名带 `Result` + `Option` 略复杂但语义清晰；行为保持，calamine / rxls 调用边界与错误消息不变。

## Technical Notes

* 主要文件：`src-tauri/src/importer.rs`（`read_workbook` 342-390、`read_legacy_xls` 392-435、`legacy_cells_to_rows` 437-450）。
* 被复用判定函数：`detect_template_data_start`、`detect_header_row`、`infer_core_fields`。
* 质量命令：`cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo test --manifest-path src-tauri/Cargo.toml`、`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`。
