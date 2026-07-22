# 修复会话删除失败或耗时过长

## Goal

让删除历史会话在大规模 SQLite 数据（数十万到百万条记录）下可完成且不阻塞应用界面；删除单一/最后一个会话时应尽量采用文件级清理，同时保留其他历史会话，避免把“删除当前会话”误实现为清空全部历史。

## What I already know

* 用户反馈：会话无法删除，或者删除很慢；认为只需删除 SQLite 文件即可。
* 当前历史数据集中存放在 `<storageRoot>/MaiyinAnalysisData/history-v1.sqlite3`，不是“一会话一文件”。直接删除数据库文件会同时删除所有历史会话。
* `SessionStore::delete` 使用 `DELETE FROM sessions WHERE session_id = ?1`，依赖多层 `ON DELETE CASCADE` 清理大批 `records/people` 子行；该调用目前在同步 Tauri 命令 `delete_session` 中执行。
* 数据库启用 WAL；FTS5 contentless 表没有外键级联，删除路径目前也没有显式清理对应 FTS 行。
* 本机现存数据库约 1.65 GB，但 `sessions/records/people` 均为 0；`page_count = 402798`、`freelist_count = 367337`，证明旧删除只完成逻辑行删除，没有回收数据库文件。
* contentless FTS5 的 UNINDEXED `session_id` 不能作为可靠的回读/删除键；测试证实旧 `DELETE ... WHERE session_id = ?` 会留下文档。清理必须使用对应主表的真实 SQLite `rowid`。
* 导入、合并、重分析、查询等重操作已使用 `spawn_blocking`，删除是明显例外。
* 前端已有 `busy === "delete"` 状态，但后端同步阻塞会让窗口无响应，用户可能误判为失败。

## Assumptions (temporary)

* 删除一个会话必须保留其他历史会话。
* 当目标是最后一个历史会话时，可以关闭连接、删除数据库及其 WAL/SHM 文件，再按现有 schema 重新创建空库。
* 不改变原始 Excel 文件，也不需要兼容旧 JSON 历史。

## Open Questions

* 多会话场景是否接受“逻辑删除立即完成、物理空间回收稍后进行”，还是必须在命令返回前完成物理清理？（暂按同步完成逻辑删除、最后会话文件级清理处理）

## Requirements (evolving)

* 将删除操作移到 blocking worker，保持 Tauri 主线程和 WebView 响应。
* 删除目标会话的所有关系数据（包括 FTS5 contentless 索引行），不能留下可见历史或搜索脏数据。
* 删除最后一个 listed 会话时走安全的数据库文件级快速路径；删除后自动重建空 schema，后续导入仍可用。
* 启动时若发现历史为空但数据库文件仍异常膨胀，应尽力重建空库，自动处理旧版本已经留下的 1GB+ 空文件。
* 多会话场景继续只删除目标会话，不得误删其他会话；必要时提供可控的 WAL/空间清理策略。
* 保持 active session 替换规则、`WorkspaceSnapshot` 和前端状态行为不变。
* 增加大数据/最后会话/FTS 清理/失败回滚相关测试或可重复验证。

## Acceptance Criteria (evolving)

* [x] 删除大数据会话期间 UI 不冻结，`delete` busy 状态可见，命令最终返回成功或结构化错误。
* [x] 删除最后一个会话后，历史列表为空、active session 为空，数据库文件可再次打开并成功导入。
* [x] 删除一个会话后其他会话仍可加载、查询和搜索；目标会话的 FTS 行不再存在。
* [x] 删除不存在的会话仍返回 `session_not_found`，不会破坏数据库。
* [x] Rust 测试、前端类型检查/测试和项目质量检查通过。

## Definition of Done (team quality bar)

* Tests added/updated (unit/integration where appropriate)
* Lint / typecheck / CI green
* Database/command contract notes updated if behavior changes
* Rollout/rollback considered for file replacement paths

## Out of Scope

* 将现有共享数据库全面重构为“一会话一 SQLite 文件”。
* 删除用户原始 Excel/CSV 文件。
* 改变会话列表、合并或导出产品功能。

## Technical Notes

* 相关代码：`src-tauri/src/storage.rs` (`SessionStore::delete`, schema/connection)、`src-tauri/src/commands.rs` (`delete_session`)、`src/App.tsx` (`deleteCurrentSession`/`runSnapshotAction`)。
* 相关规范：`.trellis/spec/backend/database-guidelines.md`、`.trellis/spec/backend/tauri-contract.md`、`.trellis/spec/frontend/state-management.md`。
* 跨层数据流：React 删除确认 → `AppApi.deleteSession` → Tauri `delete_session` → SQLite 删除/文件清理 → 新 `WorkspaceSnapshot`。

## Research References

* [`research/sqlite-delete-options.md`](research/sqlite-delete-options.md) — 比较后台级联删除、空库文件重置、重建数据库和一会话一文件方案。

## Expansion Sweep

* Future evolution: 如果未来历史会话长期并存，可独立评估一会话一文件或后台 compaction；本次保留共享数据库契约。
* Related scenarios: 存储目录迁移、导入替换会话和合并生成的隐藏会话不能被删除路径误伤；FTS 和 WAL sidecar 必须与主库一致处理。
* Failure/edge cases: 文件被其他连接占用、删除中途 I/O 失败、删除不存在会话、目标是 active/hidden session，以及数据库已空但文件仍很大。

## Decision (ADR-lite)

**Context**: 共享 SQLite 中的大会话级联删除会产生大量写放大，并且同步 Tauri 命令会冻结窗口；逻辑删除后文件也不会自动缩小。

**Decision**: 采用“后台删除 + 最后会话文件级重置 + 多会话显式关系/FTS 清理”的方案。直接删除数据库文件仅在确认目标之外没有任何会话时执行。

**Consequences**: 单会话大库删除接近文件系统操作速度；多会话仍受 SQLite 行删除成本影响但不阻塞 UI，且不会误删其他历史。若未来需要任意会话恒定 O(1) 删除，需另立任务改为每会话独立文件。
