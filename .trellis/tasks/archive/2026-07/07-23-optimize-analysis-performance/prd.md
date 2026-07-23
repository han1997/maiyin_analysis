# 优化数据分析性能

## Goal

在不改变任何分析规则、证据、排序、统计和前后端契约的前提下，缩短大数据量下首次导入后的分析、参数重新分析与历史会话合并的等待时间，并降低分析阶段的临时内存与无效 SQLite 读写。

## What I already know

- 三条桌面端路径都在阻塞工作线程中调用同一个 Rust 核心：`import_paths`、`merge_sessions`、`reanalyze` 最终都会执行 `analysis::analyze_records`。
- `analyze_records` 已使用 Rayon 按人员并行，不能把“再加并行”当作主要方案。
- 当前代表性历史负载为 453,506 条有效记录、352,948 人，平均每人约 1.285 条记录；即使其余人员都只有两条记录，至少也有 252,390 人（约 71.5%）只有一条记录。
- 单记录人员当前仍会创建每日 `BTreeMap`、重叠结果映射、三个滚动窗口 `Vec` 和去重容器，存在明显的固定开销。
- 每个人的 7/30/365 天最大窗口分别过滤、扫描和复制记录；滚动窗口未触发预警时，证据 `Vec` 仍会提前分配。
- 重叠入住先完整保存所有配对，再按天二次统计；证据 UID 使用 `Vec::contains` 去重，密集重叠时可能退化为高阶开销。
- 酒店名和酒店区域也使用 `Vec::contains` 保序去重，高频人员记录多时会重复线性扫描。
- 全局分组前不先排除时间范围外或缺少入住时间的记录；统计问题记录时又单独扫描和收集一次全部有效记录。
- 最终人员结果使用稳定串行排序，但比较键最后包含唯一 `person_key`，可以在保持确定顺序的情况下评估无额外缓冲的并行不稳定排序。
- `reanalyze` 当前通过 `SessionStore::load` 读取原始记录、旧人员摘要和旧预警，随后丢弃旧分析；再调用完整 `save`，重写未变化的 records、记录 FTS 和记录筛选计数。
- `merge_sessions` 同样加载每个会话的旧人员摘要和预警，但合并只使用原始 records 与少量元数据。
- 上一项文件夹导入性能任务已将代表性完整保存从约 48.6 秒降至约 32.4 秒；其中 records 基础表与 records FTS 仍约占 12.9 秒。重新分析若继续重写 records，会掩盖核心算法优化的体感收益。

## Assumptions

- 用户所说的“数据分析”主要指 Tauri 桌面端的大数据量计算等待，而非浏览器演示筛选或普通结果分页。
- 同时优化共享分析核心和重新分析/合并的无效存储工作；首次导入仍需完整持久化原始记录。
- 不增加新的持久化缓存格式，不改变 SQLite schema version，不改变 Tauri/TypeScript DTO。

## Open Questions

- 无。

## Requirements (evolving)

- 相同 records 与 settings 必须产生序列化后完全一致的 `Vec<PersonAnalysis>` 与 `AnalysisStats`，包括人员顺序、预警顺序、标题/详情、分数、风险等级、证据 UID 顺序和去重顺序。
- 保持 selected/rolling 模式、时间边界包含性、缺少入住时间排除、7/30/365 天窗口定义及所有阈值/封顶规则不变。
- 保持同一入住日的重叠与非重叠多次入住规则不变；长住、无效退房时间、相同入住时间和重复酒店/区域值都必须兼容。
- 优化 `analyze_records` 的公共热路径：提前过滤并一次完成分组/问题计数，减少单记录人员固定开销，合并滚动窗口扫描，延迟证据分配，使用保序哈希去重，并避免完整物化重叠配对后再统计。
- 保持按人员并行分析；只有基准证明有效且顺序确定时才调整全局排序策略。
- `reanalyze` 只加载重新计算所需的元数据与 records，不加载即将被替换的 people/alerts。
- 重新分析在单一事务中只更新 session 的 settings/stats/counts 以及 people/alerts/person hotel 索引，不重写 records、records FTS 或 `record_filter_counts`。
- `merge_sessions` 只加载 records 与合并所需元数据，不反序列化旧 people/alerts。
- 所有重活继续位于 Tauri `spawn_blocking` 内，不移动到 React/WebView 主线程。
- 增加可复现的 ignored release benchmark，至少覆盖代表性稀疏人员分布与高频/密集重叠人员，并记录优化前后相同工作负载的阶段耗时。

## Acceptance Criteria (evolving)

- [x] 固定和生成式测试证明优化前后分析 JSON 与统计完全一致。
- [x] 代表性稀疏负载基准覆盖约 352,948 人 / 453,506 条记录的数量级或等比例可配置负载，并单独输出 `analysis_ms`。
- [x] 高频人员基准覆盖大量滚动窗口记录与密集重叠证据，证明时间和临时内存行为不恶化。
- [x] 同机 release 基准连续运行至少三次；共享 `analyze_records` 的中位耗时相对基线至少降低 20%，若未达到则以分阶段证据说明剩余主导成本并继续优化。
- [x] 重新分析基准输出 `load_records_ms`、`analysis_ms`、`persist_analysis_ms`、`total_ms`，并证明 records 行、records FTS rowid 和记录筛选计数未被删除或重建。
- [x] 重新分析事务失败时旧 settings、stats、people、alerts 与搜索结果完整保留；成功后详情证据、人员查询和导出结果与完整保存路径一致。
- [x] 合并后的去重、UID、文件数、导入统计、来源会话和分析结果保持一致。
- [x] Rust tests、`cargo fmt --check`、Clippy、前端 tests/lint/build 全部通过。

## Definition of Done

- Tests added/updated (unit/integration/ignored release benchmark where appropriate)
- Lint / typecheck / build green
- Performance evidence persisted under `research/`
- Relevant backend contracts/specs updated with analysis determinism and partial reanalysis persistence rules
- Transaction rollback and resource-usage trade-offs documented

## Expansion Sweep

- **Future evolution**：基准保留清晰的 load / analyze / persist 阶段边界，便于以后增加进度、取消或增量计算；本次不新增进度 IPC。
- **Related scenarios**：首次导入、重新分析、合并会话、人员详情和导出必须继续共享同一分析结果；浏览器演示不承担生产分析性能职责。
- **Failure/edge cases**：空时间窗、全部缺少入住时间、单人超多记录、密集重叠、相同入住时间、无效退房、重复 UID/酒店值及 SQLite 事务中断均需验证确定性和回滚。

## Research References

- [`research/analysis-hotspots-and-baseline.md`](research/analysis-hotspots-and-baseline.md) — 当前复杂度、端到端无效工作、候选方案和基准设计。
- [`../archive/2026-07/07-22-optimize-folder-import-performance/research/baseline-and-bulk-write.md`](../archive/2026-07/07-22-optimize-folder-import-performance/research/baseline-and-bulk-write.md) — 代表性导入/保存负载与上一轮 SQLite 保存基线。

## Research Notes

### Feasible approaches

**Approach A: 仅优化共享分析核心**

- 融合扫描与窗口计算，增加单记录快路径，流式汇总重叠证据并使用保序哈希去重。
- 风险最低，三条路径都会受益；但重新分析仍会反序列化旧分析并重写约 453k 条 records，用户体感可能仍由 SQLite 主导。

**Approach B: 核心算法 + 端到端读写裁剪（推荐）**

- 包含 Approach A。
- 为重新分析和合并提供 records-only 加载；重新分析使用 analysis-only 原子替换，保留原始 records 与其索引。
- 能同时降低 CPU、分配、解压/JSON 解析和 WAL/FTS 写入，最符合“优化数据分析等待时间”的用户目标。
- 需要新增存储事务测试，确认 partial replace 与完整保存结果一致且失败可回滚。

**Approach C: 持久化中间分析缓存 / 增量重算**

- 持久化按人排序、窗口和重叠中间状态，只重算受设置影响的部分。
- 理论上对频繁改参数更快，但会引入新 schema、缓存失效和版本兼容复杂度；当前不推荐作为 MVP。

## Decision (ADR-lite)

**Context**: 当前共享算法有大量可消除固定分配，但重新分析/合并还存在更大的无效反序列化和持久化工作。只做局部微优化可能无法改善用户感知的总等待。

**Decision**: 用户选择三条路径全部优化，采用 Approach B：先建立分阶段基线，再落地共享核心算法优化、records-only 加载与 analysis-only 原子替换。

**Consequences**: 分析与 DTO 语义保持不变；实现范围可能从单一 `analysis.rs` 扩展到 `storage.rs` 与 `commands.rs`，但无需前端或数据库 schema 变更。

## User Confirmation

用户于 2026-07-23 选择方案 1：首次导入后的分析、参数重新分析与历史会话合并三条路径全部优化。

## Out of Scope (explicit)

- 修改风险公式、预警文字、证据定义、排序或业务阈值。
- 将分析迁移到 React、GPU、云端或独立服务。
- 新增 SQLite schema version、持久化中间缓存或后台增量索引协议。
- UI 进度条、取消按钮、并发执行多个重新分析任务。
- 再次重做上一任务已完成的通用 records/people 批量保存、压缩格式或 FTS 架构。

## Technical Notes

- Core: `src-tauri/src/analysis.rs`.
- Entry points: `src-tauri/src/commands.rs::{import_paths, merge_sessions, reanalyze}`.
- Persistence: `src-tauri/src/storage.rs::{load, save}` currently loads and rewrites more data than reanalysis/merge consume.
- Relevant contract: `.trellis/spec/backend/tauri-contract.md`, especially “analysis ownership and result filtering”.
- Existing benchmark: `storage::tests::benchmark_real_import_pipeline` already prints parse/analysis/save but needs source files; the new analysis benchmark should be synthetic and independently reproducible.
