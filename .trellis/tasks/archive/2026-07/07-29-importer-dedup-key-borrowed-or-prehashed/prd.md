# importer dedup key borrowed or prehashed

## Goal

审计项 #10 (P3):探讨 importer 去重键的借用化(`&str` 绑定 record 生命周期)或预哈希方案,**仅在基准显示为瓶颈时落地**。当前结构化 `DeduplicationKey`(owned `String` 字段)已满足 spec "Correct" 范式,审计将其定位为"已优化过的权衡的进一步探讨"。

## What I already know

* 审计原文(`audit-report.md` §3.1, 第 63-67 行):建议探讨借用键或预哈希方案;仅在基准显示为瓶颈时落地。属 P3 低优先。
* 当前实现:`DeduplicationKey`(`importer.rs:55-67`)+ `deduplication_key()`(`importer.rs:841-854`),8 个 String 字段逐个 `.clone()` + 2 个 `DateKey`。
* 热路径:`merge_parsed_files`(`importer.rs:151-177`)与 `merge_sessions`(`commands.rs:222-243`)。**不在** `analyze_records`/`detect_dense_day_overlaps` 路径(分析侧借用 `&record.person_key` 分组,不构造 DeduplicationKey)。
* 基准设施:`benchmark_synthetic_multi_file_import_merge`(`importer.rs:1058-1123`,env: `MAIYIN_BENCH_FILES`/`MAIYIN_BENCH_ROWS_PER_FILE`)已覆盖 merge/dedup,但 baseline 是"拼接字符串键",不是"借用/预哈希键"——只能证明"结构化键优于拼接键",不能隔离结构化键自身的 clone 开销。
* spec(`tauri-contract.md` §"importer determinism and performance", 232-303)明确 endorse 当前 owned 结构化键为 "Correct",拼接字符串键为 "Bad"。借用/预哈希是超出 spec 现状的进一步优化。

## Requirements

### Phase 1: 基准建立(必做)
* 在大规模(目标 ~453k 记录)下运行现有 `benchmark_synthetic_multi_file_import_merge`,捕获 `new_merge_ms`(当前结构化 owned 键)与 `old_merge_ms`(拼接键 baseline)的绝对耗时。
* 基准结果记录到任务 research 目录,作为决策依据。

### Phase 2: 条件实现(仅当 Phase 1 显示瓶颈)
* **决策门**:若 `new_merge_ms` 在 453k 规模下绝对耗时小(非瓶颈),记录发现关闭任务,不引入过度工程。
* 若显示为瓶颈:原型借用键变体(`DeduplicationKeyRef` with `&str` 字段,绑定 record 生命周期),在现有基准中新增对比分支(owned vs borrowed)。
* 若借用键显示有意义提升:在 `merge_parsed_files`(importer.rs)与 `merge_sessions`(commands.rs)两条热路径落地实现。
* 若借用键无有意义提升:记录发现关闭任务。

## Acceptance Criteria

* [ ] Phase 1: 基准在大规模下运行,结果记录到 research/benchmark-results.md
* [ ] Phase 1: 瓶颈决策有文档化结论(是/否瓶颈)
* [ ] Phase 2 (条件): 若瓶颈,借用键变体原型完成并基准对比
* [ ] Phase 2 (条件): 若落地,merge_parsed_files 与 merge_sessions 两条热路径均覆盖
* [ ] Phase 2 (条件): 若落地,现有基准断言 records/duplicates/uid 三元组一致性保持
* [ ] cargo fmt --check / clippy -D warnings / 50 tests 通过
* [ ] npm lint / 37 tests / build 通过(若前端未触及则跳过)
* [ ] spec 更新(若行为变更)

## Decision (ADR-lite)

**Context**: 审计 #10 (P3) 建议探讨去重键借用化/预哈希,但明确标注"仅在基准显示为瓶颈时落地"。当前结构化 owned 键已满足 spec "Correct" 范式。

**Decision**: 采用两阶段基准先行策略。Phase 1 跑现有基准建立数据;Phase 2 仅在数据显示瓶颈时才原型并落地借用键,否则记录发现关闭任务。

**Consequences**: 避免对已满足 spec 的实现做过度工程。若基准证明非瓶颈,任务以调研结论关闭;若是瓶颈,则数据驱动地投入优化。两条热路径(merge_parsed_files + merge_sessions)需同步覆盖。

## Out of Scope (explicit)

* `analyze_records` / `detect_dense_day_overlaps` 路径——不使用 DeduplicationKey,无关联
* 预哈希方案——若借用键可行则不额外探讨预哈希(预哈希是借用键不可行时的替代方案)
* 分析侧 O(n²) 优化——属已完成的独立任务 #4

## Technical Notes

* 参考 research/findings.md 与 research/code-samples.md
* 现有基准:`cargo test --manifest-path src-tauri/Cargo.toml --release -- --ignored benchmark_synthetic_multi_file_import_merge --nocapture`(配合 `MAIYIN_BENCH_FILES`/`MAIYIN_BENCH_ROWS_PER_FILE` env)

## Research References

* [`research/findings.md`](research/findings.md) — 审计原文、当前代码位置/类型、热路径判定、基准设施、spec 约定
* [`research/code-samples.md`](research/code-samples.md) — 10 段实际代码片段
