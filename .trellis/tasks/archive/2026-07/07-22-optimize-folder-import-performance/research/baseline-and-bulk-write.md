# 文件夹导入性能基线与 SQLite 批量写入调研

## 现有基线

Release benchmark（本机，当前提交 `4d78fe9`）：

```text
15 files × 20,000 CSV rows: parse_ms=501, new_merge_ms=267
352,948 people / 453,506 records: save_ms=48,560
同一数据库规模的查询：people household=554ms，imported-record filters=5ms 左右
```

结论：上一轮结构化去重已使合并阶段降到亚秒级；当前用户感知的主要等待来自
`SessionStore::save`，不是文件夹遍历或 Rayon 文件解析。保存路径对每条 record
执行一次 records INSERT + 一次 contentless FTS INSERT；对每个 person 还执行
一次 people INSERT、一次查询隐式 rowid、一次 FTS INSERT。352,948 人规模下，逐人
`SELECT rowid ...` 和逐行 FTS 调用是明显的可消除工作。

## SQLite 官方资料（2026-07-22 读取）

- [FTS5 documentation](https://www.sqlite.org/fts5.html)：FTS5 使用一系列 segment
  B-tree；`automerge` 会在写入期间合并 segment，默认值为 4，设为 0 可禁用自动
  增量合并，但会增加后续查询/优化成本。`optimize` 会重组整个 FTS 索引，可能耗时很长。
- 同一文档的 External Content / Contentless sections：contentless-delete 表支持
  `INSERT ... SELECT`，但不能使用 `rebuild`；保持 source rowid 映射是调用方责任。
- [SQLite PRAGMA documentation](https://www.sqlite.org/pragma.html#pragma_synchronous)：
  `synchronous=NORMAL` 在 WAL 模式下是应用可接受的平衡；`OFF` 只适合可丢失数据的
  场景，不适合本地历史数据，因此不采用降低 durability 的捷径。
- [WITHOUT ROWID](https://www.sqlite.org/withoutrowid.html)：WITHOUT ROWID 适合主键
  查找型表，但当前 FTS 依赖真实 rowid，不能直接把 `records`/`people` 改成该布局。

## 可行方案

### A（推荐）：同一事务内先写普通表，再批量构建 FTS

- 保留现有 schema、rowid、删除语义和查询契约。
- `records`/`people` 只负责一次 prepared INSERT；事务内分别执行
  `INSERT INTO <fts>(rowid, ...) SELECT rowid, ... FROM <source> WHERE session_id=?`。
- 删除 people 逐行 rowid 查询；FTS 继续使用 source rowid，因此内容一致。
- 移除每个 person 的隐式 rowid SELECT，并把 FTS tokenization/statement 循环交给
  SQLite 单条 `INSERT ... SELECT`。
- 先不使用 `synchronous=OFF`、不重建全库索引、不改变数据模型，回滚简单。

### B：导入期间临时停用/延迟全部索引

- 在新会话写入前删除或停用 B-tree/FTS 索引，写完后重建。
- 可能在单次导入上更快，但会重建其他历史会话索引，扩大锁定窗口和崩溃风险；
  FTS contentless 表也没有按 session 增量 rebuild 的安全捷径。不作为 MVP。

### C：放宽 SQLite durability（`synchronous=OFF`）

- 可能降低 fsync 成本，但断电/崩溃时事务损坏风险与产品的本地历史可靠性冲突。
- 不采用。

## 实施顺序

1. 先实现 A，并加入保存阶段分段计时 benchmark（普通表、FTS、提交）。
2. 若仍达不到目标，再在不改变契约的前提下优化归一化/JSON 预计算与导入器微基准。
3. 每步用同一规模数据验证记录数、people 数、查询结果、删除/重建 FTS 和事务回滚。

## 目标

在 352,948 people / 453,506 records 代表性负载上，相对当前约 48.6 秒保存基线
至少降低 30%，并且不牺牲崩溃恢复、搜索准确性或历史会话兼容性；若硬件/SQLite
波动导致绝对值变化，以相同环境下的前后比例为准。

## 最终实现与结果（2026-07-22）

最终保存路径保留 `WAL + synchronous=NORMAL`，并组合使用以下优化：

- records/people 的 JSON、归一化字段按 4,096 行分块并由 Rayon 并行准备；容量为 1
  的有界通道把下一分块准备与当前分块 SQLite 写入重叠，峰值内存仍受控。
- records、people、alerts、person_hotels、person_hotel_regions 使用最多 900 个绑定变量
  的多行 INSERT，兼容 SQLite 历史 999-variable 上限。
- records/people FTS 改为事务内 `INSERT ... SELECT`；新 v2 FTS 使用 external content、
  `detail=none`、`columnsize=0`，旧 v4 FTS 继续服务历史会话。
- 删除 people 逐行 rowid 查询，并移除与复合主键完全重复或仅为其左前缀的四个显式索引；
  EXPLAIN QUERY PLAN 验证查询继续使用主键自动索引。
- record/summary JSON 使用 `lz4_flex 0.14` 安全 block codec 压缩为带 `MYL4` 魔数的 BLOB；
  读取端同时接受旧 TEXT、普通 JSON BLOB 和新压缩 BLOB，无需重写历史行。
- 新建数据库在进入 WAL 前设置 16 KiB page size；已有 v4/v5 数据库不 VACUUM、不重写，
  避免启动白屏或长迁移。批量保存连接使用按需最多 128 MiB page cache。

目标规模 release 基准（同机、生产默认路径，连续两次）：

```text
旧基线: 352,948 people / 453,506 records, save_ms=48,560
最终 1: save_ms=32,461
最终 2: save_ms=32,368

最终 2 分段:
records_base=9,377
records_fts=3,555
people_base=12,112
people_fts=1,857
commit/checkpoint=5,236
```

相对旧基线分别提升 33.15% 和 33.34%，超过 PRD 的 30% 目标。测试过程中系统磁盘负载
导致绝对值曾在约 29.5–42.8 秒间波动，因此验收采用连续无代码变化运行与同机构建比例；
最终默认路径的两次连续结果均低于 32.5 秒。

page-size 小基准（100,000 people / 300,000 records，同一 release 二进制）：

| page size | save_ms | FTS search | household prefix |
| --- | ---: | ---: | ---: |
| 4 KiB | 14,201 | 56 ms | 169 ms |
| 8 KiB | 12,572 | 38 ms | 92 ms |
| 16 KiB | 12,108 | 26 ms | 56 ms |
| 32 KiB | 13,875 | 30 ms | 45 ms |

16 KiB 在保存和查询之间最均衡；32 KiB 已因页面过大使写入退化，因此未采用。

## 已验证但未采用

- 256 MiB page cache：收益不稳定，保留 128 MiB 上限。
- `defer_foreign_keys=ON`：无可测收益。
- `wal_autocheckpoint=0`：只把 checkpoint 转移到连接关闭，总耗时更差。
- `synchronous=OFF`：可能更快，但破坏本地历史的断电可靠性。
- 经典 contentless FTS + `columnsize=0`：删除/扫描契约不适合现有查询；采用 external-content v2。
- 仅恢复 summary TEXT：目标规模退化到 `save_ms=37,909`，说明减少 WAL/检查点字节量
  大于小 JSON 压缩的 CPU 成本。
