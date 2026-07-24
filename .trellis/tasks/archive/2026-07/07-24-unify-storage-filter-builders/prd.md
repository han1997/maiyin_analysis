# 统一 storage filter 构建器

## Goal

抽取 `src-tauri/src/storage/filter.rs` 中 `build_person_filter` 与 `build_records_filter` 共享的 5 个子块为 helper 函数，消除 ~60% 重复代码，降低后续筛选语义调整时的双处同步风险。**行为不变，生成的 SQL 与绑定值完全一致。**

## What I already know

- `filter.rs`（462 行）已在上一个任务中从 storage.rs 拆出为独立模块。
- 两个构建器（`build_person_filter` 6-135、`build_records_filter` 137-259）共享 7 个子块，其中 5 个结构相同（仅列名前缀/FTS 表名不同），2 个结构不同。
- 结构相同的 5 个块（可抽取）：
  1. **search/FTS**：normalize → ≥3 字符走 FTS trigram + LIKE，≤2 走 LIKE only。差异：FTS 表名（people_search_fts vs records_search_fts）、search 列名前缀（p.search_text vs search_text）。
  2. **age**：min_age → `age >= ?`，max_age → `age <= ?`。差异：列名前缀（p.age vs age）。
  3. **gender**：`gender = ?`。差异：列名前缀（p.gender vs gender）。
  4. **household include**：province/city/county split → contains_any_clause 逐列 push。差异：列名前缀（p.household_* vs household_*）。
  5. **household exclude**：province/city/county exclude → contains_any_clause → NOT (...) join。差异：列名前缀。
- 结构不同的 2 个块（不抽取）：
  6. **hotel name**：person 用 `EXISTS (SELECT 1 FROM person_hotels ph WHERE ...)`；records 用直接列 `hotel_name_norm LIKE ?`。结构本质不同（子查询 vs 直接列）。
  7. **hotel jurisdiction**：person 用 `EXISTS (SELECT 1 FROM person_hotel_regions phr WHERE ... AND {region_clauses})` 包裹多列为一个 EXISTS；records 逐列直接 push 各自的 contains_any_clause。结构本质不同。
- 现有工具函数已共享：`normalize`、`contains_pattern`、`fts_trigram_query`、`escape_like`、`fuzzy_pattern`、`contains_any_clause`、`split_filter_terms`、`split_hotel_terms`、`has_filter_terms`。
- 质量门：`cargo test`（45 passed / 8 ignored）、`cargo fmt --check`、`cargo clippy -D warnings` 必须保持全绿。
- spec（database-guidelines）对筛选语义有严格约定（per-field OR / cross-field AND / EXISTS 包裹 / FTS rowid 映射等），抽取不得改变任何语义。

## Decisions (locked)

- **范围**：仅抽取 5 个结构相同的子块为 helper 函数；hotel name 与 hotel jurisdiction 因结构本质不同而不抽取。
- **行为不变**：生成的 SQL 字符串与绑定值 Vec 必须与抽取前完全一致（字节级）。
- **helper 签名**：每个 helper 接收 `&mut Vec<String>` clauses、`&mut Vec<Value>` values，加上必要的参数（列名前缀/FTS 表名等）。
- **不引入 enum/trait 策略**：保持简单函数签名，避免过度抽象。
- **质量门**：`cargo test` 45 passed / 8 ignored 不变、`cargo fmt --check` 无 diff、`cargo clippy -D warnings` 零告警。

## Requirements

- 抽取 5 个共享子块为 `pub(super)` helper 函数：
  1. `push_search_filter(clauses, values, search, fts_tables, search_column)`
  2. `push_age_filter(clauses, values, min_age, max_age, age_column)`
  3. `push_gender_filter(clauses, values, gender, gender_column)`
  4. `push_household_include_filter(clauses, values, household_province, household_city, household_county, column_prefix)`
  5. `push_household_exclude_filter(clauses, values, exclude_household_province, exclude_household_city, exclude_household_county, column_prefix)`
- 两个构建器调用这些 helper 替换内联块。
- hotel name 与 hotel jurisdiction 块保留在各自构建器内不动。
- 生成的 SQL 与绑定值与抽取前完全一致。

## Acceptance Criteria

- [ ] 5 个 helper 函数抽取完成，两个构建器调用它们。
- [ ] `cargo test` 通过数仍为 45 passed / 8 ignored。
- [ ] `cargo fmt --check` 无 diff。
- [ ] `cargo clippy --all-targets -- -D warnings` 零告警。
- [ ] `git diff` 仅 `src-tauri/src/storage/filter.rs` 有改动。

## Definition of Done

- 上述 AC 全部满足。
- 生成的 SQL 与绑定值完全一致（现有测试守卫语义）。

## Out of Scope

- 强行统一 hotel name / hotel jurisdiction 的 EXISTS 与直接列两种结构。
- 改变任何筛选语义（per-field OR / cross-field AND / FTS 策略等）。
- 新增测试（现有 45 个测试守卫行为）。

## Technical Notes

- FTS 表名参数化方案：`push_search_filter` 接收 FTS 表名对（如 `[("people_search_fts", "people_search_fts_v2")]`），用 format! 拼入 SQL。
- 列名前缀参数化方案：`push_age_filter` 接收 `age_column: &str`（如 `"p.age"` 或 `"age"`），用 format! 拼入。
- household 前缀参数化：`push_household_include_filter` 接收 `column_prefix: &str`（如 `"p."` 或 `""`），内部拼 `format!("{prefix}household_province_norm")`。
- 审计报告 P2 #5 原文：抽取一个接收"列名前缀映射 + 是否走 EXISTS"参数的共享构建器。本方案更保守——只抽结构相同的 5 块，不强行统一 EXISTS 与直接列。
