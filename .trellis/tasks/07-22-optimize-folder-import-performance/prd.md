# 优化文件夹批量导入性能

## Goal

定位“选择文件夹”导入链路仍然缓慢的真实瓶颈，并在不改变导入结果、会话语义和本地数据安全性的前提下，显著缩短大批量 Excel/CSV 文件从目录扫描到可用分析结果的总耗时。

## What I already know

- 前端通过 `importFolder()` 选择单个目录，再调用 Tauri `import_folders`。
- Rust 后端使用 `walkdir` 扫描目录，导入逻辑集中在 `src-tauri/src/importer.rs`，持久化集中在 `src-tauri/src/storage.rs`。
- 仓库刚完成过一次“多文件导入合并性能”优化（提交 `f12a11b`），因此本次需要用基准拆分阶段耗时，确认剩余瓶颈，而不是重复已有优化。
- 用户明确反馈：实际“选择文件夹”导入仍然慢，要求充分调研并提高性能。

## Assumptions (temporary)

- 性能问题主要发生在 Tauri 桌面端，而非浏览器演示模式。
- 必须保持当前支持的文件格式、字段兼容、去重规则、分析结果和会话历史行为不变。
- 应优先优化真实热路径和算法/IO，而不是仅增加 UI 动画或模糊进度提示。

## Open Questions

- 无（用户已确认采用保留数据可靠性的批量 FTS 方案）。

## Requirements (evolving)

- 对目录发现、文件解析、记录归一化/合并、分析计算和 SQLite 持久化分别计时或建立可复现基准。
- 识别并消除不必要的串行处理、重复读取/克隆、全量排序/哈希、JSON 编解码或逐行数据库写入。
- 保证错误文件处理、空目录、嵌套目录、重复文件/记录和多格式混合目录行为稳定。
- 导入执行不得冻结 Tauri 主线程，且不得破坏现有并发与数据库一致性保护。
- 保存阶段优先采用同一事务内“普通表批量写入 + FTS `INSERT ... SELECT`”路径，避免每条记录/每个人单独写入 FTS 和查询 rowid。
- 在代表性 352,948 people / 453,506 records 负载上，相对当前约 48.6 秒保存基线至少降低 30%（以同机同构建的前后比例为准）。

## Acceptance Criteria (evolving)

- [x] 建立代表性文件夹导入基准，并记录优化前后各阶段耗时与总耗时。
- [x] 对基准工作负载实现至少 30% 的可重复、可量化总导入性能提升，或以进一步基准证据解释未达标原因。
- [x] 相同输入的导入条数、重复数、风险统计、人员聚合与持久化结果保持一致。
- [x] FTS 搜索、删除会话、重启恢复和事务回滚结果与优化前一致。
- [x] Rust 测试、Clippy、前端测试、lint 和 build 通过。
- [x] 关键性能路径具备回归测试或基准保护。

## Definition of Done

- Tests added/updated (unit/integration/benchmark where appropriate)
- Lint / typecheck / build green
- Performance evidence persisted under `research/`
- Relevant backend/frontend contracts or guidelines updated when behavior or conventions change
- Rollback risk and resource-usage trade-offs documented

## Out of Scope (explicit)

- 改变业务分析规则、字段映射或去重定义。
- 将本地数据上传到云端处理。
- 为性能而静默跳过损坏文件或部分记录。
- UI 进度条、取消导入和跨进程增量进度协议。
- 更换 SQLite/自定义二进制存储架构，或删除其他历史会话的共享索引。

## Technical Notes

- Relevant files discovered: `src/api/tauriApi.ts`, `src/App.tsx`, `src-tauri/src/commands.rs`, `src-tauri/src/importer.rs`, `src-tauri/src/storage.rs`.
- Previous related task: `.trellis/tasks/archive/2026-07/07-22-optimize-import-performance/`.
- Relevant specs: `.trellis/spec/backend/database-guidelines.md`, `.trellis/spec/backend/tauri-contract.md`, `.trellis/spec/frontend/state-management.md`, `.trellis/spec/frontend/quality-guidelines.md`, `.trellis/spec/frontend/type-safety.md`.

## Expansion Sweep

- **Future evolution**: 为后续进度/取消能力保留阶段边界和可测量计时点，但本次不改变 Tauri 命令返回契约。
- **Related scenarios**: 单文件导入、文件夹递归导入、重复导入、会话删除、重新打开和 FTS 搜索必须继续使用同一 rowid/事务语义。
- **Failure/edge cases**: 混合 `.xls/.xlsx/.csv`、空目录、重复文件、事务中断、FTS 写入失败和旧会话删除都必须原子失败或保持原状态。

## User Confirmation

用户确认按方案 A 继续（“ok”）：优先提升吞吐，同时保留 SQLite 的本地数据可靠性设置。

## Research References

- [`research/baseline-and-bulk-write.md`](research/baseline-and-bulk-write.md) — 代表性负载基线、SQLite/FTS5 官方资料与方案比较。

## Research Notes

- 文件解析与合并在当前基准中约 0.8 秒，而 SQLite 保存约 48.6 秒；因此本次 MVP 以落盘热路径为主。
- 推荐方案保留当前 schema 和真实 SQLite rowid，避免影响已有 FTS 删除契约；不采用 `synchronous=OFF` 或全库索引重建。

## Decision (ADR-lite)

**Context**: 选择文件夹导入的用户感知耗时由大批量 SQLite 保存主导；当前每条 record/person 都单独维护 FTS，且 people 路径包含逐行 rowid 查询。

**Decision**: 在同一事务内使用受 SQLite 变量上限保护的多行普通表写入与 FTS `INSERT ... SELECT`，移除 people 的逐行 rowid 查询；以容量为 1 的有界流水线重叠分块准备和 SQLite 写入，并用安全 LZ4 BLOB、16 KiB 新库页面降低 WAL/检查点字节量。

**Consequences**: 数据原子性、真实 rowid 删除语义、搜索和 Tauri DTO 契约保持不变；SQLite 数据库版本无损升级到 5，而 StoredSession 模型 schema 仍为 4。读取端必须兼容旧 TEXT 与新 `MYL4` BLOB；已有数据库不为 page size 执行启动期重写。

## Implementation Plan

1. 为保存路径补充分段 benchmark/测试，记录普通表、FTS 和提交阶段的基线。
2. 将 records/people 的 FTS 镜像改为事务内批量 `INSERT ... SELECT`，删除逐人 rowid 查询与逐行 FTS statement。
3. 根据基准继续消除重复 normalization/serialization，并补充低风险 importer 热路径优化。
4. 验证导入结果、FTS 查询、会话删除、事务回滚和 SQLite 重开一致性。
5. 运行 Rust/前端完整质量门，记录同机前后性能数据并更新项目规范。
