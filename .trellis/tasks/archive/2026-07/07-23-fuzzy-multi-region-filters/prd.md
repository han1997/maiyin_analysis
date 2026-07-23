# 支持旅馆与户籍地模糊多选筛选

## Goal

在人员分析和导入入住记录两个结果视图中，扩展入住旅馆省/市/县与人员户籍地省/市/县筛选：每个字段支持模糊搜索、使用中英文逗号分隔多个候选值，并按“命中任一”处理；户籍地继续同时支持包含与排除条件。旅馆名称保留现有语义，避免改变已有查询习惯。

## What I already know

* 前端 `src/domain/types.ts` 中 `PersonQuery`、`ImportedRecordsQuery` 的省市县字段目前是单个字符串。
* `src/App.tsx` 在人员视图和导入记录视图各渲染一套省/市/县输入框；`src/lib/filter.ts` 提供浏览器演示筛选实现。
* Rust 查询契约位于 `src-tauri/src/model.rs`，SQL 构造位于 `src-tauri/src/storage.rs`，当前省市县主要使用结构化列的前缀范围查询。
* 现有旅馆名称支持逗号、中文逗号、顿号、分号和换行分隔，多个名称按全部命中（AND）处理，并使用有序子序列模糊匹配。
* 旅馆区域对人员结果通过 `person_hotel_regions` 做同一旅馆区域条目的组合匹配；导入记录直接匹配结构化旅馆省/市/县列。
* 户籍地当前包含条件在不同字段之间按 AND，排除条件命中任一字段即排除；这与“字段之间仍保持 AND、排除任一命中即排除”的目标方向一致，但每个字段尚未支持多值 OR。
* 浏览器演示数据、Tauri DTO、SQLite 查询与测试需要保持跨层一致；不得回退此前的 SQLite/导入性能优化。

## Confirmed Semantics

* “模糊搜索”对省、市、县统一采用不区分大小写、去空白后的任意子串匹配；输入 `安徽` 可命中 `安徽省`，输入 `徽省` 也可命中。
* 同一字段内逗号分隔的候选值按 OR；不同字段（省、市、县）之间按 AND。
* 旅馆省/市/县在人员视图中沿用既有正确性约束，必须来自同一条 `hotelRegions` 记录，避免把不同旅馆的区域拼接命中。
* 户籍地包含条件为所有已填写字段各自“命中任一”（字段之间 AND）；排除条件为任一候选值命中即排除，且包含与排除整体按 AND 组合。
* 继续兼容英文逗号、中文逗号、顿号、中英文分号、换行和回车作为分隔符。
* 前端继续使用文本输入框，通过占位提示明确逗号多选，不引入新依赖或复杂 token 组件。

## Open Questions

* 无（MVP 边界已确认）。

## Requirements (evolving)

* [x] 旅馆省份、城市、县区字段支持去空白、忽略大小写的任意子串模糊搜索。
* [x] 户籍地包含/排除的省份、城市、县区字段支持同样的模糊搜索。
* [x] 每个字段支持中英文逗号分隔多个候选值，候选值按 OR；兼容顿号、分号、换行和回车，空项与重复项忽略。
* [x] 不同字段（省/市/县）之间按 AND；人员旅馆区域要求同一条 `hotelRegions` 记录满足组合条件。
* [x] 人员视图与导入记录视图使用一致的筛选语义。
* [x] 旅馆名称既有“多项需全部命中”的语义保持不变。
* [x] 更新前端演示过滤、Rust SQL 查询、DTO 类型和相关测试。
* [x] 对旧客户端缺失字段保持反序列化兼容。

## Acceptance Criteria (evolving)

* [x] `安徽,浙江` 的旅馆省份筛选可命中省份为安徽或浙江的记录，不要求同时存在两省。
* [x] `黄山,杭州` 的旅馆城市筛选可命中任一城市；省、市、县同时填写时仍按字段间 AND。
* [x] 人员拥有多个入住旅馆时，旅馆省/市/县组合不会跨不同旅馆区域条目拼接命中。
* [x] 户籍包含候选值命中任一即可；排除候选值命中任一则记录被排除。
* [x] 中英文逗号、顿号、分号、换行和回车行为一致，空项与重复项不会改变结果。
* [x] 浏览器与 Tauri 查询结果、分页总数和筛选计数一致。
* [x] 旅馆名称多项仍需全部命中，且既有有序模糊语义不回归。
* [x] 前端单元测试、Rust 测试、lint/type-check 通过。

## Definition of Done (team quality bar)

* Tests added/updated (unit/integration where appropriate)
* Lint / type-check / CI green
* Docs/spec notes updated if behavior changes
* Rollout/rollback considered for query-contract changes

## Out of Scope (explicit)

* 不改变旅馆名称的既有有序模糊与多项 AND 语义。
* 不新增行政区级联选择器、远程行政区字典或第三方搜索依赖。
* 不修改导入、分析、数据库迁移及此前已完成的性能优化逻辑，除非为支持筛选查询所必需。
* 不做行政区后缀、简称、别名或拼音归一化；不改变既有旅馆名称筛选语义。

## Decision (ADR-lite)

**Context**：原有省市县与户籍地字段只能输入单值，并以结构化前缀查询；用户需要在不改变旅馆名称筛选习惯的前提下进行模糊、多选区域筛选。

**Decision**：在前后端统一解析兼容分隔符为候选数组；字段内候选按 OR，字段间按 AND；区域值做规范化后的任意子串匹配；人员旅馆区域组合继续绑定同一条区域记录；户籍排除候选任一命中即排除。查询层使用参数化 SQL，并保留可利用索引的候选预筛选后进行精确确认（如需要）。

**User confirmation**：用户已确认人员视图采用同一入住旅馆区域记录满足省、市、县组合条件（选项 1）。

**Consequences**：查询表达能力与两种运行时一致性提高；多候选可能扩大扫描范围，需要通过 SQL 计划、批量参数和现有索引控制性能。行政区别名不在本次范围内，后续可独立引入字典/归一化策略。

## Final Confirmation

**Goal**：为旅馆区域与人员户籍地省市县提供一致的模糊多选筛选能力。

**Implementation Plan**：

1. 更新共享 TypeScript/Rust 查询契约与分隔符解析辅助函数。
2. 更新浏览器演示筛选和 Tauri SQLite 查询，覆盖人员与导入记录两条路径。
3. 更新双视图 UI 提示、筛选计数与测试；运行 Trellis 质量检查并记录规范变更。

## Technical Notes

* 重点文件：`src/domain/types.ts`、`src/lib/filter.ts`、`src/App.tsx`、`src/data/demo.ts`、`src/api/browserApi.ts`、`src-tauri/src/model.rs`、`src-tauri/src/storage.rs` 及对应测试。
* 规范待实现前按 `trellis-before-dev` 读取 backend/database、backend/tauri-contract、frontend/state-management、frontend/type-safety、frontend/quality-guidelines。
* 当前任务目录：`.trellis/tasks/07-23-fuzzy-multi-region-filters`，状态为 `in_progress`，待提交与归档。
