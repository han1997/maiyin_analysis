# Research: importer dedup key (borrowed or prehashed)

- **Query**: 调研 importer 去重键实现 —— 审计项 #10 (P3) 的建议、当前代码位置/类型、是否在热路径、importer 基准设施、相关 spec 约定
- **Scope**: internal (代码 + spec + 审计报告)
- **Date**: 2026-07-29

## Findings

### 1. 审计项 #10 的原文建议（verbatim）

审计报告位于 `.trellis/tasks/archive/2026-07/07-24-repo-code-audit/audit-report.md`，第 63–67 行（属于 §3.1 性能维度）：

> #### [P3] importer 去重键为每条记录 clone 全部字段
> - **文件**: `src-tauri/src/importer.rs:800-813`（`deduplication_key`）
> - **问题**: 结构化 `DeduplicationKey` 已避免了"拼接字符串"的额外分配（符合 spec 的 Correct 范式），但构建键时仍对 10 个 `String` 字段逐个 `clone()`。453k 记录导入约产生 ~450 万次字符串 clone 用于 `HashSet` 插入。属 spec 已知且已优化过的权衡，仅作低优先记录。
> - **建议方向**: 探讨借用键（`&str` 绑定 record 生命周期，插入后即弃）或先哈希字段再比较的方案；仅在基准显示为瓶颈时落地。
> - **建议子任务**: `importer-dedup-key-borrowed-or-prehashed`

在报告 §5 子任务列表第 205 行亦登记为：

> | 10 | `importer-dedup-key-borrowed-or-prehashed` | 性能 | P3 | 探讨去重键借用化/预哈希，仅在基准瓶颈时落地 |

**关键点**：审计明确将其定位为"已优化过的权衡的进一步探讨"，且**仅在基准显示为瓶颈时才落地**——不是必须项。

> 注：审计引用的行号 `importer.rs:800-813` 与 `54-66` 基于 2026-07-24 当日源码；当前源码（2026-07-29）行号已漂移，见下表。

### 2. 当前 dedup key 代码位置与类型

| 元素 | 当前位置 (2026-07-29) | 审计引用位置 (2026-07-24) | 说明 |
|---|---|---|---|
| `DeduplicationKey` 结构体 | `src-tauri/src/importer.rs:55-67` | `importer.rs:54-66` | `#[derive(Debug, Hash, PartialEq, Eq)]`，8 个 `String` 字段 + 2 个 `DateKey` |
| `DateKey` 枚举 | `src-tauri/src/importer.rs:69-73` | — | `Parsed(NaiveDateTime)` \| `Raw(String)` |
| `deduplication_key()` 构造器 | `src-tauri/src/importer.rs:841-854` | `importer.rs:800-813` | 对 8 个 String 字段逐个 `.clone()`，再经 `date_key()` 构造两个 `DateKey` |
| `date_key()` | `src-tauri/src/importer.rs:856-860` | — | 有 parsed 时间用 `Parsed`，否则 `Raw(raw.trim().to_string())`（raw 路径另有一次分配） |

**类型与构造特征**：
- 键类型是**结构化 struct**（非拼接字符串），derive 了 `Hash/PartialEq/Eq`，可直接作为 `HashSet` 元素。
- 字段全部**owned**（`String` / `NaiveDateTime`）。构造时对 record 的 8 个 String 字段逐个 `clone()`。
- `DateKey::Raw` 分支额外 `raw.trim().to_string()`，即 raw 时间路径每条记录多 1–2 次小字符串分配。
- 审计所述"10 个 String 字段"= 8 个直接 String 字段 + 2 个 `DateKey::Raw` 内含的 String。

完整代码片段见同目录 `code-samples.md`。

### 3. 是否在热路径（带证据）

**是——但仅在 import / merge 路径，不在 analyze_records / detect_dense_day_overlaps 路径。**

| 调用点 | 位置 | 是否热路径 | 证据 |
|---|---|---|---|
| `merge_parsed_files`（生产导入合并） | `importer.rs:151-177` | **是** | `seen = HashSet::with_capacity(total_records)`(158)，对**每条** record 调 `deduplication_key(&record)`(168) + `seen.insert(key)`(169)。453k 记录 → 453k 次键构造 |
| `merge_sessions`（合并历史会话） | `commands.rs:222-243` | **是** | `seen = HashSet::new()`(224)，对每个 session 的每条 record 调 `importer::deduplication_key(&record)`(235) + `seen.insert(key)`(236)。合并多个 453k 会话时随规模线性放大 |
| `analyze_records` | `analysis.rs:74-127` | **否** | 按 `&record.person_key`(`&str`) 分组进 `HashMap<&str, Vec<&Record>>`(79,88)，**不构造 DeduplicationKey**。person_key 是 record 上既有字段，借用即可 |
| `detect_dense_day_overlaps` | `analysis.rs:139-268` | **否** | 用 `HashMap<(String,String), usize>`(157) 做同住宿分组（key=`(compact(hotel_name), compact(room_no))`）、`HashSet<u64>`(159) 做涉及人员集合、`BTreeMap<NaiveDateTime, Vec<usize>>`(158) 做扫描线。**与 DeduplicationKey 无关** |

**结论修正**：任务描述将"analyze_records / detect_dense_day_overlaps"列为待查路径，但 dedup key **不经过**该路径。分析阶段操作的是已去重、已入库的 record，其去重发生在更早的 import/merge。`analysis.rs` 自身的 O(n²) 问题是另一条独立审计项（#4 `analysis-overlap-scanline-for-dense-persons`，P2），已通过扫描线方案落地（见 `quality-guidelines.md` "Threshold-switched hybrid for dense overlap detection"）。dedup key 的真正热路径是 `merge_parsed_files` 与 `merge_sessions`。

### 4. importer 基准设施

**存在 importer 基准，且已覆盖 merge/dedup 路径。** 无 `MAIYIN_IMPORTER_BENCH_*` 前缀的变量；importer 用的是 `MAIYIN_BENCH_*` 前缀。`MAIYIN_ANALYSIS_BENCH_*`（如 `MAIYIN_ANALYSIS_BENCH_OVERLAPS`）属分析侧，与 importer 无关。

| 基准函数 | 位置 | env 门控 | 度量内容 | 与 dedup key 关系 |
|---|---|---|---|---|
| `benchmark_parallel_file_parsing` | `importer.rs:1021-1056` | `MAIYIN_BENCH_FILE`（必需）+ `MAIYIN_BENCH_COPIES`（默认 15） | 顺序 vs 并行 `parse_file` 的 `sequential_ms`/`parallel_ms`/`reduction_percent` | **不涉及**——只测 parse，不进 merge |
| `benchmark_synthetic_multi_file_import_merge` | `importer.rs:1058-1123` | `MAIYIN_BENCH_FILES`（默认 15）+ `MAIYIN_BENCH_ROWS_PER_FILE`（默认 10_000） | `parse_ms` + `old_merge_ms` + `new_merge_ms` + `merge_reduction_percent` | **直接覆盖**——对比 `merge_parsed_files_baseline`(1143-1212，旧拼接字符串键) 与 `merge_parsed_files`(生产结构化键)，断言 records/duplicates/uid 三元组一致 |

**重要细节**：
- 两个基准均 `#[ignore]` 门控，通过 `cargo test --manifest-path src-tauri/Cargo.toml -- --ignored` + 设置 env 运行。审计 §1 质量门基线记载"45 passed / 8 ignored"——8 个 ignored 即各基准。
- `benchmark_synthetic_multi_file_import_merge` 的 baseline 是**旧的拼接字符串键**（`baseline_deduplication_key` at `importer.rs:1192-1206`，用 `\u{1f}` join 10 字段），对比对象是当前**结构化键**。即该基准已能度量"结构化键 vs 拼接键"的 merge 收益，但**不能隔离结构化键自身的 clone 开销**——baseline 不是"借用键变体"，而是更差的"拼接键变体"。
- 因此，若要按审计建议评估"借用键/预哈希"收益，**现有基准不足以直接度量**：需要新增一个对比"当前结构化 owned 键" vs "借用键/预哈希键"的微基准，或在现有基准里增加第三个分支。

### 5. 相关 spec 约定

`.trellis/spec/backend/tauri-contract.md` "Scenario: importer determinism and performance"（第 232–303 行）**直接规范了 dedup key 范式**：

- **契约**（第 253–256 行）：
  > - Dedup keys should avoid avoidable large joined-string allocation on hot paths; use a structured hash key when fields are already available as typed values.
  > - Multi-file import benchmarks must report parse time separately from merge/dedup time so optimization work targets the actual bottleneck.

- **Wrong**（第 290–294 行）：
  ```rust
  let key = fields.join("\u{1f}");
  ```
  > This clones fields and allocates one large separator string per imported row.

- **Correct**（第 298–303 行）：
  ```rust
  let key = DeduplicationKey { id_no, hotel_name, check_in, check_out, /* ... */ };
  ```
  > Structured keys preserve equality semantics while avoiding the extra joined-string allocation in the merge/dedup hot path.

- **Bad**（第 276 行）：
  > Bad: reintroducing a separator-joined string dedup key for every row in a large import.

**解读**：spec 的 "Correct" 范式示例仍使用**owned 字段**（`id_no`, `hotel_name` 等），即当前 `deduplication_key()` 实现已满足 spec 契约。审计项 #10 探讨的"借用键/预哈希键"是**超出 spec 现状要求**的进一步优化，spec 并未要求，也未禁止。落地时需注意不要回退到拼接字符串键（spec 明确列为 Bad）。

`.trellis/spec/backend/quality-guidelines.md`：无针对 importer dedup-key clone 的专门约定。相关条目有：
- "Threshold-switched hybrid for dense overlap detection"（第 100–142 行）——属分析侧 O(n²) 优化，与 dedup key 无关。
- "Test-gated timing helpers"（第 87–98 行）——`SessionStore::save` 用 `SaveTimer` + `MAIYIN_SAVE_TIMINGS` 门控计时；若为 dedup-key 优化新增计时，可参考此零生产成本模式。

### Related Specs

- `.trellis/spec/backend/tauri-contract.md` §"importer determinism and performance"（232–303）——dedup key 范式权威约定
- `.trellis/spec/backend/quality-guidelines.md` §"Test-gated timing helpers"（87–98）——计时外置模式（若需基准则参考）
- `.trellis/tasks/archive/2026-07/07-24-repo-code-audit/audit-report.md` §3.1 / §5——审计原文

## Caveats / Not Found

- **行号漂移**：审计（2026-07-24）引用 `importer.rs:800-813`/`54-66`，当前源码（2026-07-29）已漂移至 `841-854`/`55-67`。期间归档任务 `split-importer-parse-file`、`deduplicate-importer-sheet-scoring` 可能移动了行号；本报告的当前行号以今日源码为准。
- **未找到**名为 `MAIYIN_IMPORTER_BENCH_*` 的 env 变量。importer 基准统一用 `MAIYIN_BENCH_*` 前缀（`MAIYIN_BENCH_FILE`/`MAIYIN_BENCH_COPIES`/`MAIYIN_BENCH_FILES`/`MAIYIN_BENCH_ROWS_PER_FILE`）。
- **未找到** analyze_records 路径使用 DeduplicationKey 的任何证据——dedup key 严格限于 import/merge，分析阶段不重新去重。
- 现有 `benchmark_synthetic_multi_file_import_merge` 的 baseline 是"拼接字符串键"，**不是**"借用键/预哈希键"。因此要按审计建议验证"借用/预哈希"收益，需新增对比分支；现有基准只能证明"结构化键优于拼接键"，不能证明"结构化键已无进一步优化空间"。
- 实际代码片段见同目录 `code-samples.md`。
