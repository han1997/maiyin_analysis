# 为导入记录增加结果筛选功能

## Goal

让"导入记录"视图拥有与"人员研判"一致的结果筛选能力，便于在原始入住记录中按旅馆、辖区、户籍、年龄、性别、关键词等条件快速定位记录。

## What I already know

- 人员研判的 `PersonQuery`（`src/domain/types.ts:141`）现有筛选：`search`、`hotelSearch`、`hotelProvince/City/County`、`householdProvince/City/County` + `excludeHousehold*`、`minAge/maxAge`、`gender`、`level`（风险等级）、`alertState`（预警状态）、`page/pageSize`。
- 导入记录的 `ImportedRecordsQuery`（`src/domain/types.ts:92`）只有 `page/pageSize`，无任何筛选字段；后端 `query_imported_records`（`storage.rs:385`）只按 `session_id` + `check_in IS NOT NULL` + 选定模式时间窗过滤。
- `records` 表 schema（`storage.rs:634`）只有 `session_id, uid, person_key, check_in, record_json`，无结构化筛选列；而 `people` 表 + `person_hotels` + `person_hotel_regions` 子表有完整结构化列。
- **利好**：`Record`（`model.rs:103`）已携带结构化字段：旅馆辖区 `province/city/county`、户籍 `household_province/city/county`、`age: Option<u8>`、`gender`、`name/id_no/phone/hotel_name`，导入时已填充，只是未提取成 `records` 表列。
- 人员研判的辖区筛选用 `person_hotel_regions` 子表 EXISTS；户籍用 `household_region_norm` LIKE；搜索用 `search_text` LIKE（`storage.rs:790-878`）。
- 导入记录是**单条入住记录**视角，本身无风险等级/预警状态（那是人员聚合属性）。
- SQLite schema 当前 `user_version = 2`；spec 约定版本 1 清空重建为 2，其他未支持版本拒绝。
- 性能契约：453k 记录的首页打开要快，筛选必须在 SQLite 层做，不能全量反序列化。

## Assumptions (temporary)

- "相同的结果筛选"指字段集合与交互形态对齐人员研判的"更多筛选"弹窗，而非逐字复刻（导入记录无等级/预警）。
- 筛选在 SQLite 层执行，需给 `records` 表增加结构化列并提升 schema 版本。

## Open Questions

- （已收敛）MVP 不纳入"按所属人员风险等级/预警状态"筛选——导入记录是单条记录视角，无等级属性。

## Requirements

- 导入记录视图新增结果筛选，字段集合与人员研判对齐（适用部分）：`search`（姓名/证件/电话/旅馆等文本）、`hotelSearch`（旅馆名模糊）、`hotelProvince/City/County`（旅馆辖区）、`householdProvince/City/County` + `excludeHousehold*`（户籍包含/排除）、`minAge/maxAge`、`gender`。**不纳入** `level`/`alertState`（导入记录无这些属性）。
- 筛选在 SQLite 层执行，保持 453k 记录性能：给 `records` 表增加结构化列并加索引，不在前端全量过滤。
- `ImportedRecordsQuery` 扩展筛选字段，`#[serde(default)]` 兼容旧调用方。
- 前端导入记录 tab 增加筛选 UI，复用人员研判的"更多筛选"弹窗形态（filter-popover）与"草稿→应用"模式；筛选与应用结果分离，应用后回到第 1 页重新请求。
- schema 从 `user_version=2` 迁移到 `3`：ALTER TABLE 加列 + 从 `record_json` 回填现有行，保留用户已导入数据（不清空重建）。

## Acceptance Criteria

- [ ] 导入记录可按旅馆名/辖区/户籍(含排除)/年龄/性别/关键词筛选，字段集合与人员研判一致（除等级/预警外）。
- [ ] 筛选在后端 SQLite 执行，前端只请求一页；453k 记录筛选响应不退化。
- [ ] 旧 `ImportedRecordsQuery`（无筛选字段）安全兼容（serde default）。
- [ ] schema v2→v3 迁移保留现有数据，ALTER 后列从 `record_json` 回填。
- [ ] 筛选 UI 复用 filter-popover 形态，草稿→应用→回第 1 页；弹窗外部点击/Escape/互斥关闭。
- [ ] lint/build/test/fmt/clippy 全绿。
- [ ] 不改变评分公式与导出格式。

## Definition of Done

- 后端 schema 升级(v2→v3) + 导入记录筛选查询 + 回归测试。
- 前端筛选 UI + 跨层 DTO 同步 + 交互测试。
- 更新 Trellis 跨层契约与前端状态规范（导入记录筛选态、schema 版本）。
- 全部门禁绿色。

## Out of Scope (explicit)

- 不纳入风险等级/预警状态筛选（导入记录无此属性）。
- 不纳入"按来源文件(sourceFile)筛选"（人员研判没有，属未来演进）。
- 不改变重合入住/同日多次入住评分公式。
- 不改变导出文件格式。
- 不重新设计导入记录表本身。

## Technical Approach

- **Schema**：`records` 表增加结构化列 `name_norm, id_no_norm, phone_norm, hotel_name_norm, hotel_province_norm, hotel_city_norm, hotel_county_norm, household_region_norm, household_province_norm, household_city_norm, household_county_norm, age, gender, search_text`；加索引 `(session_id, hotel_name_norm)`、`(session_id, household_region_norm)` 等按需。`user_version` 2→3，打开 v2 库时 `ALTER TABLE` 加列 + `UPDATE records SET ... = json_extract(record_json, ...)` 回填。
- **DTO**：`ImportedRecordsQuery` 增加 `search, hotel_search, hotel_province/city/county, household_province/city/county, exclude_household_province/city/county, min_age, max_age, gender`，全部 `#[serde(default)]`。
- **查询**：`query_imported_records` 增加筛选子句，复用 `query_people` 的 `fuzzy_pattern/contains_pattern/normalize/split_hotel_terms` 工具；旅馆名用 `hotel_name_norm LIKE`、辖区用 `hotel_province_norm/city_norm/county_norm LIKE`、户籍用 `household_*_norm LIKE` + `NOT(...)` 排除、年龄/gender 用列、search 用 `search_text LIKE`。
- **前端**：导入记录 tab 增加工具栏（搜索框 + 更多筛选按钮 + filter-popover），`recordsFilterDraft`/`recordsQuery` 状态分离，应用筛选→回第 1 页→`appApi.getImportedRecords(recordsQuery)`；弹窗复用人员研判的受控 open/外部点击/Escape/互斥逻辑。
- **保存路径**：`save` 写 `records` 时填充新增结构化列（从 `Record` 字段直接取，`*_norm` 用 `normalize`，`search_text` 拼接）。

## Decision (ADR-lite)

**Context**: 导入记录需与人员研判一致的结果筛选，但 records 表无结构化列、Record 已有结构化字段。
**Decision**: schema v2→v3 ALTER 加列回填（不清空）；筛选字段集合对齐人员研判但剔除等级/预警；UI 复用 filter-popover 形态。
**Consequences**: 保留用户数据；一次 schema 升级；导入记录与人员研判筛选交互一致；后续若要 sourceFile 筛选可再加列。

## Technical Notes

- 主要文件：`src-tauri/src/model.rs`（`ImportedRecordsQuery` 扩展）、`src-tauri/src/storage.rs`（schema 升级 + `query_imported_records` 筛选）、`src-tauri/src/commands.rs`、`src/domain/types.ts`、`src/App.tsx`、`src/styles.css`、`src/App.test.tsx`。
- `Record` 已有 province/city/county/household_*/age/gender，schema 升级时提取成列。
- 跨层契约：`.trellis/spec/backend/tauri-contract.md`、`.trellis/spec/backend/database-guidelines.md`。
