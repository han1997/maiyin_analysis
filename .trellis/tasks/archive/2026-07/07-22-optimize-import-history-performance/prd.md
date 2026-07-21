# 优化数据导入与历史打开性能

## Goal

针对数十万条旅馆入住记录，显著缩短多文件导入、分析、持久化和历史会话首次打开时间，同时降低峰值内存与前端 IPC 数据量。

## What I already know

- 当前多文件逐个串行解析，每个文件先完整物化为字符串二维数组。
- 导入解析通过 `spawn_blocking` 执行，但风险分析和 JSON 持久化仍会在后续命令链路中同步处理。
- 历史会话将明细与分析结果存放在单个 JSON 文件中，打开时整体读取和反序列化。
- `WorkspaceSnapshot` 会把全部人员汇总发送到前端，再由 React 本地筛选分页。
- 真实历史样本为 556 MB，包含 453,506 条记录和 352,948 人；其中明细约 365.5 MB，分析约 190.5 MB。
- 同一文件的磁盘读取约 293 ms，主要瓶颈不是磁盘带宽，而是完整 JSON 解码、对象构建和全量 IPC。

## Confirmed Behavior

- 数十万至百万条记录是需要长期支持的正常数据规模。
- 优化必须保留现有分析公式、证据追溯、导出内容和敏感数据本地存储原则。
- 允许引入本地嵌入式存储依赖；不实现旧 JSON 历史迁移。
- 历史打开优先展示元数据和首屏人员结果，详情与原始明细可以按需加载。
- 采用 SQLite 作为新的本地历史存储与查询层，不采用自定义拆分二进制格式。
- 当前旧历史经用户明确确认删除，用户将在新版本中重新导入原始数据。
- 性能目标：453k 记录 SQLite 历史首次打开到首屏不超过 2 秒，再次打开不超过 1 秒；15 文件导入总耗时至少降低 50%；耗时操作期间界面保持响应。

## Requirements (evolving)

- 多个独立输入文件可并行解析，最终去重、UID 和错误统计保持确定性。
- 解析、分析和大型持久化操作不得阻塞 Tauri 命令/UI 线程。
- 历史打开不再读取并向前端传输全部人员与全部明细。
- 人员结果采用后端过滤与分页，首屏只传输当前页和总数。
- 人员详情、导入明细和导出继续按需取得完整证据。
- 新存储中的数据与现有导入、分析和导出语义保持一致。
- SQLite 写入使用事务与批处理；常用人员筛选、人员键和会话键建立索引。
- 新 SQLite 存储从空库开始；应用不承担旧 JSON 会话兼容或自动迁移。

## Technical Approach

- 在 `MaiyinAnalysisData` 下建立版本化 SQLite 数据库，存储会话元数据、明细、人员汇总、预警、旅馆名称/辖区和证据映射。
- `WorkspaceSnapshot` 只携带工作区元数据、统计、历史摘要和当前查询首屏，不再携带全量人员集合。
- 新增后端人员查询命令，接收现有 `PersonQuery`，在 SQLite 中完成筛选、排序、总数统计和分页。
- 人员详情、导入明细和导出从 SQLite 按需读取；导出继续保持完整分析口径。
- 多文件解析采用有界并行，每个文件产生独立解析结果，随后按确定顺序统一去重、分配 UID、分析并批量写入事务。
- 导入、重新分析和大型导出在 blocking worker 中运行，锁只覆盖必要的状态切换。
- SQLite schema 直接从空库初始化；用户通过原始 Excel 重新建立历史数据。

## Decision (ADR-lite)

**Context**: 真实会话达到 556 MB，打开时需要解码全部明细和分析，并通过 IPC 发送 352,948 人员汇总；仅增加线程或更换 JSON 解析方式无法消除主要瓶颈。

**Decision**: 使用 SQLite 的事务、索引和分页查询替代单文件全量会话加载；同时并行解析独立输入文件并将大型 CPU/IO 工作移出命令线程。

**Consequences**: 历史首屏与筛选的数据量变为固定页大小，峰值内存显著下降；代价是新增数据库 schema、查询一致性与导出适配工作，且旧 JSON 历史不会被新版本读取。

## Acceptance Criteria (evolving)

- [ ] 453k 记录历史打开时不再产生 556 MB 整体 JSON 解码和 352k 人员 IPC。
- [ ] 453k 记录的 SQLite 历史首次打开到首屏不超过 2 秒，再次打开不超过 1 秒。
- [ ] 15 文件导入总耗时相对基线至少降低 50%。
- [ ] 导入、分析和持久化期间界面保持响应，并提供明确状态反馈。
- [ ] 历史首屏仅返回固定页大小的人员结果及总数。
- [ ] 多文件导入使用有界并行，结果顺序、去重数和异常统计可重复。
- [ ] 分析与持久化在 blocking worker 中执行，UI 保持可响应。
- [ ] 现有筛选、详情、导出、合并和重新分析行为保持正确。
- [ ] 增加性能基准或可重复计时测试，记录真实样本前后结果。
- [ ] 前端与 Rust 质量门全部通过。

## Definition of Done

- Unit/integration tests cover pagination, filters, transaction/atomicity, and deterministic parallel import.
- Performance measurements use the existing 453k-record history and untouched source workbook(s).
- `npm test`, `npm run lint`, `npm run build`, Rust tests, formatting, and Clippy pass.
- Storage and Tauri contracts are documented with rollback behavior.
- Existing user Excel files remain untouched；旧历史按用户确认删除，不要求兼容恢复。

## Out of Scope (explicit)

- 不修改风险评分公式或预警语义。
- 不引入远程服务、云数据库或上传行为。
- 不为性能删除证据、原始明细或导出字段。
- 不以仅增加加载动画代替实际耗时优化。
- 不实现旧 JSON 历史到 SQLite 的迁移或兼容读取。

## Research References

- [`research/performance-bottlenecks-and-options.md`](research/performance-bottlenecks-and-options.md) — 真实数据规模、瓶颈分解及三种架构方案。

## Technical Notes

- Import path: `src-tauri/src/importer.rs` → `commands::import_paths` → `analysis::analyze_records` → `SessionStore::save`.
- History path: `SessionStore::load` → `commands::load_session` → `snapshot` → Tauri IPC → React `filterPeople`.
- Likely contract changes: paginated people query, lightweight workspace snapshot, lazy detail/raw-record access, versioned SQLite schema.
- Relevant specs: `.trellis/spec/backend/tauri-contract.md`, `.trellis/spec/backend/database-guidelines.md`, `.trellis/spec/guides/cross-layer-thinking-guide.md`.
