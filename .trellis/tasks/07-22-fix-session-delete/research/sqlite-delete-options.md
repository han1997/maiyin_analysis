# SQLite 会话删除方案研究

## 观察

* 当前应用把多个历史会话放在同一个 `history-v1.sqlite3`；因此删除主文件只适用于数据库中没有其他需要保留的会话。
* `DELETE FROM sessions` 会通过外键级联触发 `records`、`people` 及其子表的逐行/索引删除；对数十万行会产生大量 WAL 写入。逻辑删除不会自动缩小主数据库文件，空闲页会留在文件中，除非执行 `VACUUM` 或重建文件。
* contentless FTS5 表没有普通外键级联，必须显式删除对应文档，否则会留下搜索索引和磁盘占用。实测其 UNINDEXED `session_id` 不能作为可靠回读键，因此应在删除主表行之前，用 `rowid IN (SELECT rowid FROM <content_table> WHERE session_id = ?)` 删除 FTS 文档。
* 当前 Tauri `delete_session` 是同步命令；其它大 I/O/CPU 命令使用 `spawn_blocking`，所以删除路径会额外阻塞主线程。

## 可行方案

### A：后台删除 + 最后会话文件级重置（采用）

* 删除命令改为 `async`，在 `spawn_blocking` 中执行。
* 删除前确认目标存在，并统计目标之外是否还有会话。若没有，关闭连接后移除主库及 `-wal`/`-shm`/journal 文件，再通过现有 schema 初始化空库。
* 若还有会话，显式清理 FTS 和关系表，再删除目标 session；保留其他会话及 active-session 替换规则。

优点：单大​​会话（最常见的 1GB+ 场景）接近文件删除速度；多会话语义正确；改动集中。风险：文件删除遇到并发读连接时可能失败，需要返回结构化错误或安全回退到行删除。

### B：每次删除都 `VACUUM INTO`/重建剩余数据库

优点：每次都回收空间、不会残留空闲页。缺点：保留会话越多，复制成本越大；删除一个小会话可能反而比级联删除更慢；需要原子替换与崩溃恢复策略。

### C：一会话一 SQLite 文件

优点：任意会话删除都接近 `remove_file`。缺点：改变存储契约、迁移/移动目录/合并/查询和 schema 管理，超出本次修复范围。

## 结论

采用 A。先解决主线程阻塞和最后会话的巨大空闲文件；多会话路径保留共享数据库并显式清理所有当前子表/FTS。后续若多会话删除仍有性能数据，再单独评估 B 或 C。

## 实现后验证

* 本机旧库：主文件约 `1,649,860,608` bytes，`sessions/records/people = 0`，`page_count = 402798`，`freelist_count = 367337`；新启动自愈条件会识别为异常膨胀空库并文件级重建。
* Rust：34 passed、5 ignored；删除回归覆盖多会话 active 替换、FTS rowid 清理、最后 listed 会话 + hidden combined、异常膨胀空库重建、缺失 session 不改数据。
* Frontend：20 passed；删除确认后可见 busy status，竞争会话操作 disabled，完成后切换到空工作区。
* `cargo clippy --all-targets -- -D warnings`、`npm run lint`、`npm run build` 均通过。

参考：

* SQLite VACUUM：<https://www.sqlite.org/lang_vacuum.html>
* SQLite WAL 与 checkpoint：<https://www.sqlite.org/wal.html>
* SQLite foreign-key cascading deletes：<https://www.sqlite.org/foreignkeys.html>
