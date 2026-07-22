# Fix white screen after last commit

## Goal

Restore application startup responsiveness after the most recent code submission introduced a white screen / unresponsive runtime.

## What I already know

* User reports the program has been unresponsive and blank since the last code commit.
* Recent feature commit `e6360f1` added result filtering for imported records and touched both frontend filtering UI and Tauri storage.
* The repository is a Vite/React frontend with a Tauri backend.

## Assumptions

* The desired fix is a bug fix only; no new filtering behavior should be added beyond restoring the intended committed feature.
* The root cause should be verified through local build/test/runtime diagnostics where possible.

## Requirements

* The app must render instead of showing a white screen.
* Existing result filtering behavior from the last feature should continue to work.
* The fix should be scoped to the failing runtime path.

## Acceptance Criteria

* [x] Frontend test suite passes.
* [x] Type check / build succeeds.
* [x] Startup migration no longer reads the full legacy records table into memory at once.
* [x] No unrelated files are changed.

## Definition of Done

* Tests and build checks run or any inability to run them is documented.
* Trellis quality check is performed.
* Spec update is considered if the bug reveals a durable convention.

## Out of Scope

* Redesigning the filter UI.
* Adding new import/filter features.
* Broad refactors unrelated to startup rendering.

## Technical Notes

* Recent commit inspected: `e6360f1 feat: 为导入记录增加结果筛选功能`.
* Likely impacted files include `src/App.tsx`, `src/lib/filter.ts`, `src/api/browserApi.ts`, `src/domain/types.ts`, and `src-tauri/src/storage.rs`.
* 初次尝试将根因定位在 v2-to-v3 SQLite 迁移批量改写上，已提交修复；但运行时仍白屏未响应，说明修复不充分。

## Real Root Cause (re-diagnosed 2026-07-22)

* 用户本地数据库 `C:\Users\hanhu\AppData\Roaming\com.han1997.maiyin-analysis\MaiyinAnalysisData\history-v1.sqlite3` 体积 **1.1 GB**，WAL **267 MB**，`PRAGMA user_version = 2`，records 表 **453,506 行**。
* 启动时 `SessionStore::open` → `initialize_schema` 命中 `version == 2` 分支 → `migrate_records_v2_to_v3`。
* 迁移函数在 **单一 transaction** 中循环以 rowid 升序 500 条/批读取 records、解析 JSON、对每条记录 16 列字段做 `UPDATE`。整体在同一个写事务内完成，持有写锁直到 45 万行全部改写完毕；`bootstrap_workspace` 是同步 `#[tauri::command]`（非 async、非 spawn_blocking），窗口进程被阻塞，前端 `appApi.bootstrap()` Promise 一直 pending，UI 停留在 `<LoadingShell />` 白屏。
* 前次"修复"仅把整表 backfill 拆成 rowid 分批，但**没有拆事务、也没有把 bootstrap 移出主线程** —— 写锁与主线程阻塞依旧存在，因此白屏未消失。
* 同时：`PRAGMA user_version = 3` 是在 `initialize_schema` 末尾连同建表 SQL 一起 `execute_batch` 才写入的；只要迁移事务没提交，`user_version` 永远停在 2，下次启动还会从头迁移，形成"永远卡住"循环。WAL 267 MB 正是上次迁移未提交的残留。

## Fix Plan (v3 — final, supersedes v2)

用户决定：**不做数据库迁移**。结构改变时直接清理旧数据，用户重新导入即可。
之前 v2 计划的"分批事务 + user_version 提前落盘 + bootstrap async + 进度透明化"全部废弃。

具体改动（仅 `src-tauri/src/storage.rs`）：

1. **v2 分支改为清理重建**：`initialize_schema` 中 `else if version == 2` 分支不再调用
   `migrate_records_v2_to_v3`，改为调用已有的 `reset_legacy_database`（与 v1 完全一致：DROP
   全部表 → `PRAGMA user_version = 0`，随后由 `initialize_schema` 末尾的建表语句重建为 v3）。
2. **删除迁移相关代码**：移除 `migrate_records_v2_to_v3` 函数与 `RECORDS_V3_COLUMNS` 常量。
3. **回退 `&mut Connection` 改动**：`initialize_schema` 与 `SessionStore::open` 恢复为
   `&Connection`（`reset_legacy_database` 只需 `&Connection`，不再需要 `connection.transaction()`）。
4. **删除本次任务新增的迁移测试** `v2_to_v3_migration_backfills_duplicate_uids_per_session_independently`。
5. **新增清理测试** `version_two_database_is_cleared_instead_of_migrated`：参考已有
   `version_one_database_is_cleared_instead_of_migrated`，把 `user_version` 设为 2，重开后断言
   `list().is_empty()` 且 `user_version == DATABASE_VERSION`。

## Acceptance Criteria (updated — final)

* [ ] Frontend test suite passes
* [ ] `cargo test` / `cargo clippy` 通过
* [ ] `migrate_records_v2_to_v3` 与 `RECORDS_V3_COLUMNS` 已删除，`initialize_schema` 不再含任何
  从 `record_json` 回填结构化列的逻辑
* [ ] `user_version == 2` 的数据库重开后被清空重建为 v3（与 v1 行为一致），不再白屏
* [ ] `bootstrap_workspace` 未被改动（不再需要 async/spawn_blocking）
* [ ] No unrelated files are changed（仅 `src-tauri/src/storage.rs`）

## Lesson for spec

结构变化时优先"清理旧数据 + 重新导入"，不要写跨全表的迁移/回填代码——在数十万行级别会持有
写锁阻塞 Tauri 主线程导致白屏。该约定将记入 `.trellis/spec/backend/database-guidelines.md`。
