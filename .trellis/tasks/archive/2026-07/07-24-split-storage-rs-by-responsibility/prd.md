# 拆分 storage.rs 按职责分模块

## Goal

将 `src-tauri/src/storage.rs`（~3913 行）按职责拆分为模块树，降低单文件体积与阅读/定位成本，为后续 storage 相关子任务（unify-storage-filter-builders、unify-storage-batch-insert-boilerplate 等）降低改动门槛。**纯机械重构，零行为变更，零 API 变更。**

## What I already know

- `storage.rs` 当前是 `lib.rs` 中 `mod storage;` 指向的单文件。
- Rust 2021 edition，可用现代模块风格：`storage.rs`（根）+ `storage/`（子模块目录）。
- 文件内职责分布（按行号）：
  - 常量与导入：`1-42`
  - `SessionStore` struct：`44-50`
  - `SessionMetadata` struct：`51-63`
  - `PreparedRecord`/`PreparedPerson` struct：`64-103`
  - `impl SessionStore`（核心：open/list/save/replace_analysis/load/query_people/query_imported_records/person_detail/delete/move_to）：`104-833`
  - FTS 删除辅助：`delete_session_fts_rows` `835`、`delete_analysis_fts_rows` `846`、`delete_fts_rows_for_sources` `853-885`
  - 文件辅助：`remove_file_if_exists` `886-892`
  - schema 建表与版本迁移：`initialize_schema` `894-1056`、`reset_legacy_database` `1058-1078`
  - 会话/元数据查询辅助：`metadata_from` `1080`、`active_id_from` `1120`、`ensure_session_exists` `1131`、`settings_for_session` `1148-1164`
  - SQL 过滤构建器：`build_person_filter` `1166-1295`、`build_records_filter` `1297-1419`、`records_count_source` `1421`
  - 过滤计数：`fast_record_filter_count` `1442`、`increment_record_filter_count` `1525-1537`
  - 记录加载辅助：`load_records_for_person` `1538`、`load_session_records` `1559`、`load_json_column` `1570-1583`
  - 预处理：`prepare_record_chunk` `1585`、`prepare_person_chunk` `1633-1699`
  - 批量插入（8 个函数）：`insert_analysis_rows` `1700`、`insert_people_search_index` `1733`、`insert_record_batches` `1750`、`insert_person_batches` `1798`、`insert_alert_batches` `1846`+`execute_alert_batch` `1870`、`insert_person_hotel_batches` `1895`+`execute_person_hotel_batch` `1920`、`insert_person_hotel_region_batches` `1945`+`execute_person_hotel_region_batch` `1976`、`multi_row_insert_sql` `2004`
  - 过滤词条工具：`split_hotel_terms` `2017`、`split_filter_terms` `2021`、`has_filter_terms` `2036`、`contains_any_clause` `2042`、`normalize` `2054`、`contains_pattern` `2060`、`fuzzy_pattern` `2064`、`fts_trigram_query` `2077`、`escape_like` `2092`
  - JSON 压缩：`JsonCompressor` `2099`、`compressed_json` `2111`、`from_stored_json` `2132`、`json` `2148`、`from_json` `2152`
  - 数值转换小工具：`i64_from_usize` `2156`、`i64_from_u64` `2160`、`usize_from_i64` `2164`
  - 错误映射：`storage_error` `2168`、`sql_error` `2172`
  - 测试：`mod tests` `2177-3913`（约 1736 行）

## Decisions (locked)

- **交付形态**：纯机械重构，零行为变更、零公共 API 变更；`SessionStore` 的公开方法签名与 `lib.rs` 的 `mod storage;` 声明保持不变。
- **模块风格**：Rust 2021 现代风格——`storage.rs`（根）+ `storage/` 子模块目录。
- **质量门**：`cargo fmt --check`、`cargo test`、`cargo clippy -D warnings` 必须保持全绿；现有 45 passed / 8 ignored 测试数量不变。
- **验证策略**：拆分后每一步都跑 `cargo check`，最终跑全部门。

## Module layout (proposed)

```
src-tauri/src/
  storage.rs          根：常量 + SessionStore/SessionMetadata/Prepared* structs
                      + impl SessionStore（核心方法，104-833）
                      + 会话级私有辅助（FTS 删除、metadata_from、active_id_from、
                        ensure_session_exists、settings_for_session、
                        load_records_for_person、load_session_records、load_json_column、
                        remove_file_if_exists、数值转换、storage_error/sql_error）
                      + pub(crate) use 子模块再导出
  storage/
    schema.rs         initialize_schema + reset_legacy_database + SESSION_FTS_TABLES + DB 常量
    filter.rs         build_person_filter + build_records_filter + records_count_source
                      + fast_record_filter_count + increment_record_filter_count
                      + 全部过滤词条工具（split_*/has_filter_terms/contains_any_clause/
                        normalize/contains_pattern/fuzzy_pattern/fts_trigram_query/escape_like）
    write.rs          prepare_record_chunk + prepare_person_chunk
                      + 全部 insert_* / execute_*_batch + multi_row_insert_sql
    compress.rs       JsonCompressor + compressed_json + from_stored_json + json + from_json
    tests.rs          mod tests（原 2177-3913 整体迁出）
```

拆分后 `storage.rs` 根预计 ~1000 行（impl 核心 + 会话级辅助），最大子模块 `filter.rs` ~450 行、`write.rs` ~500 行。

## Requirements

- 将 `storage.rs` 拆为上述 1 根 + 5 子模块，按职责迁移函数/struct/常量。
- 子模块内函数对根降为 `pub(super)` 或 `pub(crate)` 可见（仅 storage 内部调用，不对外）。
- 保持 `SessionStore` 公共方法签名与可见性不变。
- 保持所有测试通过且数量不变。
- 保持 `cargo fmt --check` / `cargo clippy -D warnings` 零告警。

## Acceptance Criteria

- [ ] `storage.rs` 根文件行数显著下降（目标 < 1200 行）。
- [ ] 5 个子模块各自职责单一，无跨职责混杂。
- [ ] `cargo test` 通过数仍为 45 passed / 8 ignored。
- [ ] `cargo fmt --check` 无 diff。
- [ ] `cargo clippy --all-targets -- -D warnings` 零告警。
- [ ] `git diff` 仅新增 `storage/` 目录文件与 `storage.rs` 的搬移修改，无其他文件改动。

## Definition of Done

- 上述 AC 全部满足。
- 纯搬移，无逻辑改动（diff 可逐函数对照）。
- 工作树仅 `src-tauri/src/` 下有改动。

## Out of Scope

- 统一两个 filter 构建器的重复逻辑（子任务 #5 unify-storage-filter-builders）。
- 用宏统一批量插入样板（子任务 #11 unify-storage-batch-insert-boilerplate）。
- 移除 save 内 cfg(test) 计时（子任务 #16 storage-save-remove-inline-timing）。
- 任何函数逻辑/算法修改。

## Technical Notes

- 迁移策略：逐个子模块抽取——先抽 `compress.rs`（最内聚、依赖最少），再抽 `schema.rs`、`filter.rs`、`write.rs`，最后抽 `tests.rs`；每抽一个跑 `cargo check` 确认编译。
- 可见性：子模块函数对根用 `pub(super)`；若跨子模块互调则 `pub(crate)`。
- 常量归属：DB 常量（DATABASE_VERSION 等）随 `schema.rs`；压缩常量随 `compress.rs`；过滤常量随 `filter.rs`；其余留根。
- `lib.rs` 的 `mod storage;` 无需改动（指向 `storage.rs` 根不变）。
- 测试迁出注意：`mod tests` 内的 `use super::*` 需调整为引用正确的模块路径。
