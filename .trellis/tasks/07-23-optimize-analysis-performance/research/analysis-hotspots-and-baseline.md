# 数据分析性能热点与基准设计

## 调用链与用户感知范围

当前三个命令都在 `spawn_blocking` 闭包内执行，因此不会直接阻塞 React/Tauri 主线程：

```text
首次导入: import_paths -> importer::import_paths -> analyze_records -> SessionStore::save
合并会话: merge_sessions -> SessionStore::load × N -> 去重/重编号 -> analyze_records -> save
重新分析: reanalyze -> SessionStore::load -> analyze_records -> save
```

共享 `analyze_records` 是三者共同的 CPU 热点，但完整等待时间还包含加载和保存。特别是 `reanalyze` 与 `merge_sessions` 会通过 `load` 解压并反序列化旧 people/alerts，随后立即丢弃；`reanalyze` 又通过完整 `save` 删除并重建未变化的 records、records FTS 和记录筛选计数。

上一轮代表性保存基准（352,948 people / 453,506 records）：

```text
旧 save: 48,560 ms
当前 save: 32,368 ms
records_base: 9,377 ms
records_fts: 3,555 ms
people_base: 12,112 ms
people_fts: 1,857 ms
commit/checkpoint: 5,236 ms
```

因此重新分析继续完整保存时，至少约 12.9 秒的 records 重建工作在业务上没有必要；同时 checkpoint 成本也会因写入量增加。分析核心即使提速明显，也可能被这段固定写入掩盖。

## `analyze_records` 当前热点

### 1. 全量分组后再次过滤和扫描

- 第一遍把所有 records（包括缺少入住时间和 selected 窗口外记录）放入人员 `HashMap`。
- 每个人再次 `retain(within_analysis_time_window)`。
- 统计问题记录时第三次过滤全量 records，并创建 `scoped: Vec<&Record>`。

可将范围过滤、issues 计数和分组融合成一遍，减少无效人员分组和临时 Vec。

### 2. 大量单记录人员仍走完整通用路径

代表性负载平均每人仅 1.285 条记录。由 `singletons >= 2 * people - records` 可得：

```text
singletons >= 2 × 352,948 - 453,506 = 252,390
占 people 至少 71.5%
```

当前这些人员仍创建：

- `BTreeMap<date, Vec<&Record>>`
- 空的 overlap `BTreeMap`
- 三次 `max_window_records` 的过滤 Vec 与结果 Vec
- 酒店名/区域 fold 容器

单记录快路径可以直接构造 max counts=1、无预警的 summary，仅保留必要克隆。

### 3. 三个滚动窗口重复过滤、扫描与复制

`max_window_records` 对 7/30/365 天各执行一次：

1. 过滤所有有效 `check_in` 并分配 ordered Vec；
2. 双指针扫描；
3. 复制最佳窗口到新 Vec。

在全局提前过滤后，人员 records 均有 `check_in` 且已排序。可以一次循环维护三个单调 end 指针，只返回 `(start, end)`；只有对应预警真正触发时才从 slice 生成证据 UID。

复杂度仍为 O(k)，但从三组扫描/六次附近的分配降为一次扫描和常数状态。

### 4. 重叠配对完整物化后再次统计

当前 `overlapping_stay_pairs` 的内层因入住时间排序而提前退出，复杂度更准确地说是 O(k + P)，其中 P 为实际重叠配对数；密集重叠时 P 可达 O(k²)，而输出规则又要求精确 pair count 与 different-place count，不能简单跳过所有配对。

主要可消除成本是：

- 先保存全部 `(&Record, &Record)`，再按天二次遍历；
- 每个 pair 为 location cache 克隆两个规范化字符串元组；
- evidence UID 通过 `Vec::contains` 保序去重，最坏接近 O(P × U)；
- pair labels 在第二遍才构造。

低风险方案是在保持原有“外层 first、内层 second”访问顺序的同时，直接按天累计：pair count、different count、前四个标签、`Vec + HashSet` 保序证据。这样结果顺序不变，时间降为 O(P)，且不再保留完整 pair Vec。

### 5. 酒店字段使用线性保序去重

`hotel_names` 与 `hotel_regions` 都对输出 Vec 调用 `contains`。可使用借用字段组成的 `HashSet` 判断首次出现，仍按排序后 records 的首次出现顺序 push 克隆，保持序列化结果一致。

### 6. 最终排序

当前 `analyses.sort_by` 是串行稳定排序。比较器依次使用 score、total_records、name、person_key；由于每个 group 的 person_key 唯一，比较器形成全序。可基准比较 `par_sort_unstable_by`：

- 不稳定不会改变最终顺序，因为不会存在比较相等的两个不同人员；
- Rayon 并行排序可能改善 35 万人员规模；
- 小数据上可能有调度开销，必须以 benchmark 决定是否采用。

## 端到端可消除工作

### 重新分析

业务输入仅需要：session metadata + records + 新 settings。旧 people/alerts 不参与新结果。

推荐事务：

1. records-only 加载；
2. `analyze_records`；
3. 开启写事务；
4. 在 people 源行仍存在时删除 people FTS rowids；
5. 删除 people（外键级联 alerts/person hotels/regions）；
6. 更新 sessions 的 settings/stats/records/people；
7. 批量写新 people/alerts/hotel mappings 与 people FTS；
8. commit。

records、records FTS 和 `record_filter_counts` 不动。失败时事务回滚，旧分析仍可查询。

### 合并会话

合并只需要每个源会话的 metadata、import stats 与 records。提供 records-only 加载可避免对 35 万级旧 summary JSON 和 alerts 的解压、解析与分配；合并结果仍使用完整 `save`，因为它是一个新 records 集合。

## 方案比较

| 方案 | 收益范围 | 风险 | 结论 |
| --- | --- | --- | --- |
| 仅核心扫描/分配优化 | 三条路径的 analysis 阶段 | 低 | 必做，但可能被 reanalyze 完整保存掩盖 |
| 核心 + records-only load + analysis-only replace | 三条路径，尤其重新分析/合并 | 中低，需要事务一致性测试 | 推荐 MVP |
| 持久化中间状态与增量重算 | 频繁改参数 | 高，涉及 schema/cache invalidation | 暂不采用 |

## 基准设计

所有正式数据使用 `cargo test --release ... -- --ignored --nocapture`，同一二进制连续运行至少三次并报告中位数；优化前后使用同一生成种子和设置。

### A. 代表性稀疏人员分布

- 默认或环境变量配置为 352,948 people / 453,506 records。
- 至少 252,390 个单记录人员，其余用 2+ 条记录补足总量。
- 混合少量 selected-window 外记录、issues、重复酒店和不同酒店区域。
- 输出 `records`、`people`、`analysis_ms` 和稳定摘要哈希/JSON 等价断言。

目的：测量真实大盘中的人员分组、单人固定开销、summary 构造和最终排序。

### B. 高频非重叠人员

- 单人生成大量按时间排序/乱序输入记录，覆盖 7/30/365 天窗口。
- 断言最佳窗口选择及相同最大值时保留最早窗口的行为。

目的：隔离三窗口重复扫描和临时证据 Vec。

### C. 密集重叠人员

- 单人在同日/跨日生成可配置的重叠区间、重复/不同酒店房号。
- 比较 pair count、different count、前四个标签和 evidence UID 顺序。

目的：验证流式聚合移除完整 pair map 与 `Vec::contains` 退化后的收益。

### D. 重新分析端到端

- 先保存一个合成大 session，再分别计时 records-only load、analysis、analysis-only persist。
- 优化前对照为现有 full `load + analyze + save`。
- SQL 断言 records 的 `(rowid, uid, record_json)`、records FTS rowids 和 `record_filter_counts` 在成功重新分析前后不变。
- 注入 people 写入失败，断言 settings/stats/people/alerts/FTS 全部回滚。

## 等价性保护

- 对固定边界样例比较完整 serde JSON，而不只比较 count/score。
- 对确定种子的多人员、多日期、缺失/无效时间数据，使用测试内参考实现或优化前 golden 输出比较。
- 专门覆盖相同 check-in 的稳定顺序，因为它会影响 summary 首条身份字段、重叠 pair 标签和 evidence UID 顺序。
- 保留现有 overlap、selected/rolling、时间窗和 hotel regions 测试。

## 当前结论

推荐先确认用户实际慢路径，然后采用“共享核心优化 + 针对重新分析/合并裁剪无效存储工作”。该方案不改变 DTO 或 schema，且比单纯增加并行更符合当前代码和已有保存基准证据。

## 优化前实测基线（2026-07-23）

Release 构建，同一二进制连续三次；计时只包含 `analyze_records`，不包含合成数据生成：

```text
稀疏代表性负载（352,948 people / 453,506 records）:
937 ms, 947 ms, 926 ms
中位数: 937 ms

密集重叠负载（1 person / 800 records / 319,600 overlap pairs）:
94 ms, 94 ms, 89 ms
中位数: 94 ms
```

回归数据的完整 JSON FNV-1a 指纹：

```text
rolling: 11531671983614133412
selected: 7793499981386381458
```

后续优化必须保持两个指纹不变；最终用相同 release benchmark 重跑并比较中位数。

## 优化后实测结果（2026-07-23）

### 共享 `analyze_records`

相同 release 二进制、相同生成数据、连续三次：

```text
稀疏代表性负载（352,948 people / 453,506 records）:
优化前: 937 ms, 947 ms, 926 ms；中位数 937 ms
优化后: 480 ms, 490 ms, 490 ms；中位数 490 ms
提升: 47.7%

密集重叠负载（1 person / 800 records / 319,600 pairs）:
优化前: 94 ms, 94 ms, 89 ms；中位数 94 ms
优化后: 37 ms, 36 ms, 39 ms；中位数 37 ms
提升: 60.6%
```

rolling/selected 完整 JSON 指纹保持不变：

```text
rolling: 11531671983614133412
selected: 7793499981386381458
```

主要收益来自：提前范围过滤并融合 issues 统计、单记录人员快路径、三个滚动窗口一次扫描且只在触发时构造证据、按原 pair 顺序流式聚合重叠、保序哈希去重，以及大人员集合的确定性并行排序。

### 重新分析端到端

20,000 people / 25,000 records，同一输入分别执行旧完整路径与新 analysis-only 路径，连续三次：

```text
旧 full total: 2071 ms, 1952 ms, 2167 ms；中位数 2071 ms
新 partial total: 1021 ms, 1112 ms, 1087 ms；中位数 1087 ms
总耗时提升: 47.5%

旧 full persist 中位数: 1835 ms
新 persist analysis 中位数: 927 ms
持久化提升: 49.5%

旧 full load 中位数: 200 ms
新 records-only load 中位数: 154 ms
加载提升: 23.0%
```

放大到 100,000 people / 128,500 records 的单次 release 验证：

```text
旧路径: load=1278 ms, analysis=130 ms, persist=11428 ms, total=12903 ms
新路径: load_records=799 ms, analysis=129 ms, persist_analysis=4809 ms, total=5737 ms
总耗时提升: 55.5%
```

新路径成功前后 SQL 快照证明以下数据保持原位：

- records 的 SQLite `rowid`、业务 `uid` 和压缩 `record_json`
- `records_search_fts_v2` 的真实 rowid 文档
- `record_filter_counts`

people/alerts/person hotel 表和 people FTS 在同一事务中替换；注入重复 person key 失败后，旧 settings、stats、people、alerts、people FTS 与 records 快照全部回滚。
