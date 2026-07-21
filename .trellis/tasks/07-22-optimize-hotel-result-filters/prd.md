# 优化入住旅馆及结果筛选

## Goal

将入住旅馆辖区、入住人户籍地、排除户籍地和人员条件从分析前置条件改为人员结果的后置筛选条件，避免每次调整这些条件都重新执行风险分析；同时增强入住旅馆筛选，使多个旅馆条件采用“同时入住过”的交集语义。

## What I already know

- 当前四组条件位于 `SettingsPanel`，随 `reanalyze(settings)` 传入 Rust。
- Rust 的 `within_analysis_scope` 会先过滤明细，再进行人员分组、频次统计、预警和风险评分，因此改变筛选条件会改变分析结果并触发完整重算。
- 结果列表已在 `src/lib/filter.ts` 中进行本地筛选和分页，已有单个旅馆名称的模糊匹配。
- `PersonSummary` 已包含户籍地、年龄、性别和去重后的旅馆名称，但尚未包含人员入住过的旅馆辖区集合。
- 风险规则和评分继续由 Rust 负责；前端后置筛选不得重新计算分数。

## Confirmed Behavior

- 时间范围和频次阈值仍是分析参数，继续触发重新分析。
- 入住旅馆辖区按人员任一入住记录命中所选省/市/县区即保留该人员。
- 户籍地包含/排除、年龄和性别针对人员汇总字段筛选。
- 旅馆多条件采用 AND：每个输入条件都必须能模糊匹配该人员入住过的至少一家旅馆。
- 后置筛选只影响人员结果列表；导出保持现有全量分析结果行为。

## Requirements (evolving)

- 入住旅馆辖区、入住人户籍地、排除户籍地、最小/最大年龄和性别不再参与 Rust 风险分析。
- 上述条件放入结果页的次级筛选区域，应用筛选时不调用 `reanalyze`。
- 人员风险分数、预警、统计频次仍基于时间范围内的完整有效明细计算。
- 人员汇总数据提供后置筛选所需的入住旅馆辖区集合。
- 入住旅馆沿用文本输入，支持逗号、中文逗号、顿号等分隔符输入多个条件；每项保留模糊匹配，并仅保留同时满足全部旅馆条件的人员。
- 清除筛选、活动筛选计数和分页重置行为覆盖新增条件。
- 导出接口和导出内容不跟随后置筛选变化。

## Acceptance Criteria (evolving)

- [x] 修改任一后置筛选条件并应用时不调用重新分析接口。
- [x] 相同底层分析结果在不同后置筛选条件下风险分数和预警内容不变。
- [x] 入住旅馆辖区、户籍包含、户籍排除、年龄范围和性别均能正确筛选人员。
- [x] 输入旅馆 A 和 B 时，仅返回旅馆名称集合同时命中 A、B 的人员。
- [x] 单旅馆模糊搜索能力保持兼容。
- [x] 逗号、中文逗号、顿号分隔的旅馆条件具有一致的 AND 语义。
- [x] 新增 DTO 字段在旧会话缺失时具有安全默认值。
- [x] 使用后置筛选后，现有导出调用与导出数据范围保持不变。
- [x] 前端测试、lint、build 和 Rust 测试通过。

## Definition of Done

- Tests added/updated for result filters, hotel AND matching, DTO compatibility, and analysis scope behavior.
- `npm test`, `npm run lint`, `npm run build`, and relevant `cargo test` checks pass.
- Cross-layer contract/spec is reviewed and updated if the DTO or ownership boundary changes.
- Existing user Excel files and unrelated working-tree changes remain untouched.

## Out of Scope (explicit)

- 不修改现有风险评分公式和预警阈值逻辑。
- 不新增服务端或数据库搜索系统；筛选针对已加载的本地人员汇总结果。
- 不改变导出行为；导出仍基于当前会话的全量分析结果。

## Technical Approach

- Rust 分析仅使用时间范围和频次阈值；辖区、户籍、年龄、性别不再裁剪分析明细。
- `PersonSummary` 增加带 serde 默认值的入住辖区汇总数据，为前端后置筛选提供稳定 DTO。
- React 将原分析设置中的四组筛选条件迁移到 `PersonQuery` 和“更多筛选”区域，本地应用并分页。
- 旅馆搜索按逗号、中文逗号、顿号等分隔符拆分；每项使用现有有序模糊匹配，条件之间采用 AND。
- 旧会话缺失新增汇总字段时安全加载；必要时从会话记录补足筛选元数据，避免旧历史筛选失效。

## Decision (ADR-lite)

**Context**: 前置范围条件导致每次搜索都重新执行完整风险分析，而且会改变评分所依据的记录集合。

**Decision**: 将非时间、非阈值条件改为人员汇总结果的前端后置筛选；Rust 继续独占风险分析和评分，DTO 只补充筛选所需元数据。

**Consequences**: 筛选响应更快且不会改变风险分数；快照会增加少量辖区汇总数据；导出范围暂不跟随列表筛选。

## Technical Notes

- Likely frontend files: `src/App.tsx`, `src/domain/types.ts`, `src/lib/filter.ts`, related tests/styles.
- Likely backend files: `src-tauri/src/model.rs`, `src-tauri/src/analysis.rs`, commands/export compatibility points.
- Relevant specs: `.trellis/spec/frontend/quality-guidelines.md`, `.trellis/spec/frontend/type-safety.md`, `.trellis/spec/backend/tauri-contract.md`, `.trellis/spec/guides/cross-layer-thinking-guide.md`.
