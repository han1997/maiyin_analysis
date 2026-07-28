# Analysis overlap scanline for dense persons

## Goal

把 `analyze_person` 中单人重叠检测的 O(n²) 两两比较改为扫描线/分桶策略，消除密集人员（同日数百条入住记录）的 n²/2 量级开销，同时保留现有重叠证据产出（pair_count、different_place_count、pair_labels、evidence_ids）与基准回归。

## What I already know

* 审计报告定位：`src-tauri/src/analysis.rs:136-158`，P2 性能。审计原文："`analyze_person` 对单人记录做双层 `for` 枚举所有 `(first, second)` 对判定时间重叠。对稀疏人员，`second_start >= first_end` 的 `break` 能早停；但对'同一天多 record'的密集人员（例如 benchmark_dense_overlap_analysis 的 800 条 → 32 万对），仍是 n²/2 量级。该路径已在 `spawn_blocking` 上且有基准覆盖，不会阻塞 UI，但极端密集人员（上千条同日入住）会拉长分析耗时。"
* 审计建议方向："评估按 `(check_in, effective_end)` 区间排序后用扫描线/事件点统计替代两两比较，或对单日 record 数设阈值切换分桶策略；保留现有基准回归。"
* 当前算法 (`analysis.rs:136-158`)：
  * `records.sort_by_key(|r| r.check_in.unwrap_or(MIN))` (line 125)。
  * 外层 `for (first_index, first) in records.iter().enumerate()`，内层 `for second_index in first_index+1..records.len()`。
  * `second_start >= first_end` → `break`（因按 check_in 排序，后续 second 起始更晚，不可能重叠）。
  * `first_start < effective_end(second)` → 重叠，调 `add_pair(first, second, different_place)`。
  * `add_pair` (line 52-69) 累积：`pair_count += 1`、`different_place_count += usize::from(different_place)`、`pair_labels`（前 4 个，格式 `"{hotel} {room} 与 {hotel} {room}"`）、`evidence_ids`（去重 uid 集，用 `evidence_seen: HashSet<u64>`）。
  * 重叠归属到 `days[record_days[second_index]].overlap`（按 second 的日期分桶）。
* `different_accommodation_cached` (line 354-377)：比较 `compact(hotel_name)` 与 `compact(room_no)`，任一非空且不等即不同住宿；用 `HashMap<u64, (String, String)>` 缓存。
* 基准测试 `benchmark_dense_overlap_analysis` (line 795-824)：
  * `#[ignore = "dense overlap analysis performance benchmark"]`（release-only，env `MAIYIN_ANALYSIS_BENCH_OVERLAPS`，默认 800）。
  * 800 条记录，全部同日不同秒入住，check_out 均为 2 天后 → 全部两两重叠。
  * 断言：`stats.people == 1`、`overlap_days == 1`、`evidence_count == record_count`（全部 800 条都参与重叠）。
  * 打印 `analysis_benchmark=dense_overlap records=800 pairs=319600 analysis_ms=<n>`。
* 重叠证据 4 类产出：
  1. `pair_count`：重叠对总数。
  2. `different_place_count`：不同住宿的对数。
  3. `pair_labels`：前 4 对的 hotel/room 标签。
  4. `evidence_ids`：参与重叠的去重 uid 列表。
* `DayAnalysis` (line 26-31)：按日分桶 `day/start/end/overlap: Option<OverlapSummary>`；`day_ranges` (line 379-402) 按 check_in.date() 分桶。
* 重叠归属语义：`days[record_days[second_index]]` —— 按 second 记录的入住日期归属。
* 相关审计记录：`.trellis/tasks/archive/2026-07/07-24-repo-code-audit/audit-report.md` 3.1 节第 2 条。
* spec：`.trellis/spec/backend/tauri-contract.md` "analysis ownership" 场景要求重叠测试覆盖不同旅馆/房间、同日非重叠计数。

## Assumptions

* 保留全部 4 类证据产出（pair_count、different_place_count、pair_labels、evidence_ids）。
* 保留按日分桶归属语义（重叠归到 second 记录的入住日期）。
* 保留现有基准回归（`benchmark_dense_overlap_analysis` 断言不变）。
* 保留 `spawn_blocking` 调用边界（不改 `analyze_records` 签名）。
* 不引入新依赖。

## Open Questions

* None — 已确认采用 Approach A（阈值切换混合）。

## Requirements

* 对单日记录数超过阈值（`DENSE_OVERLAP_THRESHOLD = 32`）的日，切换到扫描线+公式快速路径，消除 O(n²) 两两比较。
* 对单日记录数 ≤ 阈值的日，保留现有 O(n²) 嵌套循环，证据产出完全不变。
* 密集路径保留全部 4 类证据产出（`pair_count`、`different_place_count`、`pair_labels`、`evidence_ids`），语义对齐现有基准断言。
* 保留按日分桶归属语义（重叠归到 second 记录的入住日期）。
* 保留 `spawn_blocking` 调用边界（不改 `analyze_records` / `analyze_person` 签名）。
* 现有重叠相关测试全绿，基准回归断言不变。
* `cargo fmt --check`、`cargo test`、`cargo clippy --all-targets -- -D warnings` 全部通过。

## Acceptance Criteria

* [ ] 密集人员场景（800+ 同日记录）的分析耗时显著降低（基准对比 before/after）。
* [ ] 现有重叠测试全绿，`benchmark_dense_overlap_analysis` 断言不变（`evidence_count == record_count`、`overlap_days == 1`）。
* [ ] 稀疏人员场景行为不变（≤ 阈值的日走原 O(n²) 路径，break 早退语义保留）。
* [ ] `cargo fmt --check`、`cargo test`、`cargo clippy --all-targets -- -D warnings` 全部通过。

## Definition of Done

* 重构范围限于 `src-tauri/src/analysis.rs`。
* 质量门全绿（Rust 三项）。
* 基准回归断言不变。

## Out of Scope

* 改变 `analyze_records` / `analyze_person` 签名或 `spawn_blocking` 边界。
* 改变重叠评分公式（`min(35, 20 + P*2 + D*5)`）。
* 改变 `AlertSummary` / `EvidenceRecord` DTO 结构。
* 优化其它分析路径（same_day_many、frequency 等）。
* 引入新依赖。

## Technical Approach

采用 Approach A（阈值切换混合）：

### 常量

```rust
const DENSE_OVERLAP_THRESHOLD: usize = 32;
```

### 总体结构

`analyze_person` 的重叠检测循环改为按日分派：
- 对 `days[d]` 的记录数 `end - start` ≤ `DENSE_OVERLAP_THRESHOLD`：走现有 O(n²) 嵌套循环（完全不变，含 break 早退）。
- 对 `days[d]` 的记录数 > `DENSE_OVERLAP_THRESHOLD`：走密集快速路径 `detect_dense_day_overlaps`。

### 密集快速路径 `detect_dense_day_overlaps`

处理一个密集日 d 的所有 second 记录（`days[d].start..days[d].end`），与所有可能的 first 记录（`0..days[d].end`）的重叠。

#### 1. pair_count —— 扫描线 + end-time 二分

- 收集所有 first 候选（index 0..days[d].end，有 check_in 的记录）的 `(effective_end, index)`。
- 按 `effective_end` 排序（或用 `Vec` + `sort_by_key`）。
- 对每个 second（index 在 days[d].start..days[d].end，有 check_in）：
  - `second_start = second.check_in`。
  - 二分搜索 `effective_end > second_start` 的 first 数量 → 该 second 的重叠 first 数。
  - 累加到 `day_pair_count`。

注意：first 的 `check_in < second_start` 已由排序保证（first index < second index）。重叠还需 `second_start < first.effective_end`，即 `first.effective_end > second_start`。二分搜索计数满足此条件的 first。

#### 2. different_place_count —— 住宿分组公式

- 对该日所有参与重叠的 first + second 记录，按住宿 key `(compact(hotel_name), compact(room_no))` 分组，用 `HashMap<(String, String), usize>` 计数。
- `same_place_pairs = Σ C(group_size, 2)`。
- `different_place_count = pair_count - same_place_pairs`。
- **空字段近似**：当 hotel 或 room 为空时，`different_accommodation_cached` 返回 false（空字段不贡献差异）。分组公式会把空字段的记录分到各自的 key 组，略高估 different_place_count（偏保守，severity 倾向 高）。这在密集路径（> 32 条/日，病态场景）可接受；≤ 32 条走精确 O(n²) 不受影响。

#### 3. pair_labels —— 有界采样

- 对前 `min(DENSE_OVERLAP_THRESHOLD, day_record_count)` 个 second 记录，用有界嵌套循环找前 4 对重叠，生成标签。
- 找满 4 个即 break。O(阈值 × 阈值) = O(1024)，常数级。

#### 4. evidence_ids —— 全重叠快捷 + 回退

- **全重叠检测**：若该日首条记录 `effective_end >= 末条 second.check_in`，则所有记录两两重叠 → `evidence_ids = 该日全部 uid`。O(n)。
- **非全重叠回退**：用扫描线追踪 involved 集。当 second 与 active first 重叠时，标记 second + 所有重叠 first 的 uid。用 `HashSet<u64>` 去重。最坏情况 O(pair_count)，但非全重叠的密集场景实际重叠数远低于 n²/2。

#### 5. 归属

重叠归属到 `days[d].overlap`（d 是 second 的日期），与现有语义一致。`pair_count`、`different_place_count`、`pair_labels`、`evidence_ids` 写入 `days[d].overlap` 的 `OverlapSummary`。

### 不变项

- `analyze_person` 签名不变。
- `overlap_score` 公式不变：`min(35, 20 + P*2 + D*5)`。
- `DayAnalysis` / `OverlapSummary` 结构不变。
- `effective_end` / `day_ranges` / `different_accommodation_cached` / `compact` / `fallback` 不变。
- ≤ 阈值的日走原 O(n²) 路径，break 早退、`add_pair` 精确调用、证据产出完全不变。
- `AlertSummary` / `EvidenceRecord` DTO 不变。

## Decision (ADR-lite)

**Context**: `analyze_person` 重叠检测对密集人员（同日数百~上千条）是 O(n²/2)。现有 break 早退处理稀疏情况，但密集场景（基准 800 条 → 32 万对）仍线性放大。路径在 `spawn_blocking` 上不阻塞 UI，但极端密集人员拉长分析耗时。重叠证据产出 4 类（pair_count、different_place_count、pair_labels、evidence_ids），纯扫描线计数无法直接产出后 3 类。

**Decision**: 采用 Approach A —— 阈值切换混合。≤ 32 条/日走原 O(n²)（精确、break 早退、证据不变）；> 32 条/日走密集快速路径：pair_count 用扫描线+二分 O(n log n)，different_place_count 用住宿分组公式 O(n)，pair_labels 有界采样 O(1)，evidence_ids 全重叠快捷 O(n) + 非全重叠回退。空字段场景在密集路径略高估 different_place_count（偏保守），≤ 32 条走精确路径不受影响。

**Consequences**: 密集场景从 O(n²) 降到 O(n log n)（全重叠快捷）或 O(n log n + k)（非全重叠，k 为实际重叠数）；正常场景零变化；基准断言保持；空字段高估是密集路径的可接受近似（severity 偏保守更安全）；评分因 pair_count 大必然触顶 35，different_place_count 近似不影响实际评分。

## Technical Notes

* 主要文件：`src-tauri/src/analysis.rs`（`analyze_person` 124-200、重叠循环 136-158、`OverlapSummary` 33-70、`different_accommodation_cached` 354-377、`day_ranges` 379-402、`effective_end` 499+、基准 795-824）。
* 重叠评分：`overlap min(35, 20 + P*2 + D*5)`（P=pair_count, D=different_place_count）。
* 质量命令：`cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo test --manifest-path src-tauri/Cargo.toml`、`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`、基准 `MAIYIN_ANALYSIS_BENCH_OVERLAPS=800 cargo test --manifest-path src-tauri/Cargo.toml --release benchmark_dense_overlap_analysis -- --ignored --nocache`。
