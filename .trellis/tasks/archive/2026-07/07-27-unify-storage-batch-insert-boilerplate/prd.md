# Unify storage batch insert boilerplate

## Goal

统一 `storage/write.rs` 中 8 个批量插入函数（5 个 `insert_*_batches` + 3 个 `execute_*_batch`）的重复骨架，消除审计指出的"同一骨架 + 两种不一致风格"问题，行为不变。

## What I already know

* 审计报告定位：`src-tauri/src/storage.rs:1750-2002`（行号已漂移，storage 已拆分为子模块，当前实际在 `src-tauri/src/storage/write.rs:219-471`），P3 结构。
* 审计原文："8 个函数（5 个 `insert_*_batches` + 3 个 `execute_*_batch`）同一骨架，且存在两种不一致风格：record/person 两个直接在循环里 push 值；alert/hotel/region 三个先 flatten 进 `rows` Vec 再委托 `execute_*_batch`。Rust 元组 arity 差异使干净的泛型较难，但可用宏统一骨架并统一两种风格。"
* 当前 8 个函数：
  * `insert_record_batches` (219-265) — direct-push，19 列，1:1 映射 `&[PreparedRecord]`。
  * `insert_person_batches` (267-313) — direct-push，18 列，1:1 映射 `&[PreparedPerson]`。
  * `insert_alert_batches` (315-337) — flatten+delegate，4 列，1:N（每人 N 个 alert）。
  * `execute_alert_batch` (339-362) — SQL 构造 + push 值 + execute。
  * `insert_person_hotel_batches` (364-387) — flatten+delegate，3 列，1:N。
  * `execute_person_hotel_batch` (389-412) — SQL 构造 + push 值 + execute。
  * `insert_person_hotel_region_batches` (414-443) — flatten+delegate，6 列，1:N。
  * `execute_person_hotel_region_batch` (445-471) — SQL 构造 + push 值 + execute。
* 已有共享骨架：`multi_row_insert_sql` (473-484) 已被全部 8 个函数复用。
* 3 个 `execute_*_batch` 函数近乎全等：仅 SQL 前缀、value_group 字符串、tuple arity（2/3/5 字段 + session_id = 3/4/6 列）、解构方式不同。
* 2 个 direct-push 函数（record/person）从 `PreparedRecord`/`PreparedPerson` struct 字段直接 push 19/18 个 `&dyn ToSql`，不走 tuple 中间层。
* `BULK_INSERT_VARIABLE_LIMIT = 900`；每函数 `max_rows = 900 / COLUMN_COUNT`。
* 相关审计记录：`.trellis/tasks/archive/2026-07/07-24-repo-code-audit/audit-report.md` 3.2 节第 5 条。

## Assumptions

* 纯行为保持 refactor：SQL 文本、列顺序、参数绑定顺序、chunk 大小、错误映射全部不变。
* 不引入新依赖。
* 不改变 `PreparedRecord`/`PreparedPerson` 结构体定义。
* 不改变 `multi_row_insert_sql` 签名。

## Open Questions

* None — 已确认采用 Approach A（仅统一 `execute_*_batch`）。

## Requirements

* 用 `bulk_insert_batch!` 宏生成 3 个 `execute_*_batch` 函数，消除 SQL 构造 + push 值 + execute 的重复骨架。
* 宏接收：函数名、SQL 前缀、value_group 字符串、列数、row tuple 类型、解构 + push 值的闭包/块。
* 5 个 `insert_*_batches` 函数保持现状（record/person 的 19/18 列 1:1 direct-push 不转 flatten+tuple）。
* 不改变任何对外行为：SQL 文本、列顺序、参数绑定、chunk 大小、错误映射。
* `cargo fmt --check`、`cargo test`、`cargo clippy --all-targets -- -D warnings` 全部通过。

## Acceptance Criteria (evolving)

* [ ] 3 个 `execute_*_batch` 的重复骨架收敛到单一宏或公共骨架。
* [ ] 行为不变：现有测试全绿，无新增失败。
* [ ] `cargo fmt --check`、`cargo test`、`cargo clippy --all-targets -- -D warnings` 全部通过。

## Definition of Done

* 重构范围限于 `src-tauri/src/storage/write.rs`。
* 质量门全绿（Rust 三项）。
* 不改变 SQL 文本、列顺序、参数绑定、chunk 大小、错误映射。

## Out of Scope

* 改变 SQL 文本、列顺序、参数绑定顺序或 chunk 大小。
* 改变 `PreparedRecord`/`PreparedPerson` 结构体或 `multi_row_insert_sql`。
* 引入新依赖。
* 重构 `write.rs` 中非批量插入函数（`insert_record_filter_counts`、`insert_people_search_index` 等）。

## Technical Approach

采用 Approach A（仅统一 `execute_*_batch`）：

1. 新增 `bulk_insert_batch!` 声明宏，生成 `execute_*_batch` 函数。宏接收：
   - 函数名（如 `execute_alert_batch`）
   - SQL 前缀（如 `"INSERT INTO alerts(session_id, person_key, alert_index, alert_json) VALUES "`）
   - value_group 字符串（如 `"(?, ?, ?, ?)"`）
   - row tuple 类型（如 `(&str, i64, &str)`）
   - 解构 + push 值的代码块（从 row tuple 解构并 push `session_id` + 各字段到 `values: &mut Vec<&dyn ToSql>`）
2. 宏展开后生成与原函数等价的 `pub(crate) fn execute_*_batch(transaction, session_id, rows) -> Result<(), AppError>`，内部调用 `multi_row_insert_sql` 构造 SQL，push 值，`prepare_cached` + `execute` + `map_err(sql_error)`。
3. 3 个 `execute_*_batch` 函数体替换为 3 个宏调用。
4. 5 个 `insert_*_batches` 函数保持现状不变。
5. SQL 文本、列顺序、参数绑定顺序、`BULK_INSERT_VARIABLE_LIMIT` chunk 大小、`map_err(sql_error)` 错误映射全部字节保持。

## Decision (ADR-lite)

**Context**: 3 个 `execute_*_batch` 函数近乎全等（仅 SQL/tuple arity/解构不同）；5 个 `insert_*_batches` 有两种风格（direct-push vs flatten+delegate）。审计建议宏统一骨架。

**Decision**: 采用 Approach A —— 仅用 `bulk_insert_batch!` 宏统一 3 个 `execute_*_batch`。record/person 的 19/18 列 1:1 direct-push 不转 flatten+tuple（大 tuple 难读，P3 风险收益不匹配）。

**Consequences**: 消除最明显的重复（3 个 `execute_*_batch`），骨架单点维护；`insert_*_batches` 的两种风格差异保留（各有适用场景）；行为保持，SQL/列/参数/chunk/错误映射零变化。

## Technical Notes

* 主要文件：`src-tauri/src/storage/write.rs`（8 个函数 219-471，`multi_row_insert_sql` 473-484）。
* 常量：`BULK_INSERT_VARIABLE_LIMIT = 900`、`SAVE_PREPARE_CHUNK_SIZE = 4_096`。
* 质量命令：`cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo test --manifest-path src-tauri/Cargo.toml`、`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`。
