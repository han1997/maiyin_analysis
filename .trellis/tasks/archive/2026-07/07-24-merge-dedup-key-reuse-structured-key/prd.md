# merge_sessions 复用结构化 DeduplicationKey

## Goal

`merge_sessions`（`commands.rs`）当前用 `\u{1f}` 拼接 10 个字段成 `String` 做 `HashSet` 去重键——这是 `tauri-contract.md` 在 importer 场景明确标注的 Wrong 模式。importer 自身已改用结构化 `DeduplicationKey`，但合并路径仍沿用旧拼接键。本任务让 `merge_sessions` 复用 importer 的 `DeduplicationKey`，消除每条记录的 10 次 String clone + 拼接分配，并统一两处去重语义。**去重结果不变。**

## What I already know

### 当前 Wrong 代码（commands.rs）

- `record_key(record: &Record) -> String`（439-453）：把 10 个字段 clone 后 `.join("\u{1f}")` 拼成 String。
- `command_date_key(value, raw) -> String`（455-459）：parsed → `format!("dt:{}", ...)`，raw → `format!("raw:{}", raw.trim())`。
- `merge_sessions`（127-189）：`let mut seen = HashSet::new();` → `let key = record_key(&record);` → `seen.insert(key)`。合并多个 453k 级会话时，每条记录都要 clone 10 个字段并分配一个拼接字符串。

### 已有 Correct 代码（importer.rs）

- `DeduplicationKey` struct（54-66）：10 个字段，与 `record_key` 的字段完全一致、顺序相同。派生 `Hash, PartialEq, Eq`。
- `DateKey` enum（68-72）：`Parsed(NaiveDateTime)` / `Raw(String)`。派生 `Hash, PartialEq, Eq`。
- `deduplication_key(record: &Record) -> DeduplicationKey`（800-813）：逐字段 clone 构建结构化键。
- `date_key(parsed, raw) -> DateKey`（815-819）：`Parsed(value)` / `Raw(raw.trim().to_string())`。
- `merge_parsed_files`（135-183）：importer 自身的去重路径已用 `DeduplicationKey`。
- 以上四项均为私有（无 `pub`），需提升为 `pub(crate)` 才能跨模块复用。

### 行为等价性证明

- 字段列表完全一致：`record_key` 和 `deduplication_key` 的 10 个字段、顺序完全相同。
- 日期键语义等价：两者都区分 parsed vs raw，都 trim raw 文本。importer 的基准测试 `benchmark_synthetic_multi_file_import_merge`（1014-1079）已用旧拼接键 baseline 与结构化键对比，断言 `records.len()`、`duplicate_count`、`(uid, source_file, id_no)` 三者完全一致。
- 唯一理论差异：`DateKey::Parsed(NaiveDateTime)` 直接比较含微秒，`command_date_key` 格式化 `%Y-%m-%dT%H:%M:%S` 丢微秒。但电子表格日期不含亚秒精度，importer 的基准测试已在真实数据上证明等价。
- `commands.rs:4` 已 `use crate::importer;`，`commands.rs:12` 已 `use std::collections::HashSet;`，无需新增 import。

## Decisions (locked)

- **方案**：提升 importer 的 `DeduplicationKey`/`DateKey`/`deduplication_key`/`date_key` 为 `pub(crate)`，在 `commands.rs` 删除 `record_key`/`command_date_key`，`merge_sessions` 改调 `importer::deduplication_key`。
- **去重结果不变**：`duplicate_count`、保留记录、UID 分配与抽取前完全一致。
- **不改动 importer 的去重逻辑**：仅提升可见性，不改 `deduplication_key`/`date_key` 的实现。
- **质量门**：`cargo test` 45 passed / 8 ignored 不变、`cargo fmt --check` 无 diff、`cargo clippy -D warnings` 零告警。

## Requirements

- `importer.rs`：将 `DeduplicationKey`、`DateKey`、`deduplication_key`、`date_key` 的可见性从私有改为 `pub(crate)`。
- `commands.rs`：
  - 删除 `record_key` 函数（439-453）。
  - 删除 `command_date_key` 函数（455-459）。
  - `merge_sessions` 内 `let key = record_key(&record);` 改为 `let key = importer::deduplication_key(&record);`。
  - `HashSet` 类型由推断自动从 `HashSet<String>` 变为 `HashSet<importer::DeduplicationKey>`，无需显式标注。
- 不改动 `importer.rs` 的任何函数实现，仅改可见性修饰符。
- 不改动 `merge_sessions` 的任何其他逻辑（UID 分配、统计、save 等）。

## Acceptance Criteria

- [ ] `importer.rs` 的 4 个项可见性为 `pub(crate)`。
- [ ] `commands.rs` 的 `record_key` 和 `command_date_key` 已删除。
- [ ] `merge_sessions` 调用 `importer::deduplication_key`。
- [ ] `cargo test` 通过数仍为 45 passed / 8 ignored。
- [ ] `cargo fmt --check` 无 diff。
- [ ] `cargo clippy --all-targets -- -D warnings` 零告警。
- [ ] `git diff` 仅 `src-tauri/src/commands.rs` 和 `src-tauri/src/importer.rs` 有改动。

## Definition of Done

- 上述 AC 全部满足。
- 去重结果（duplicate_count、保留记录、UID）与改动前完全一致。

## Out of Scope

- 改动 importer 的去重逻辑或 DateKey 语义。
- 改动 merge_sessions 的 UID 分配、统计、save 等非去重逻辑。
- 新增测试（现有 45 个测试守卫行为，其中 importer 基准测试已证明两种键等价）。
- 探讨借用键/预哈希（属子任务 #10，P3）。

## Technical Notes

- 审计报告 P2 #3 原文：在 `model` 或共享模块暴露 importer 的 `DeduplicationKey`（及对应 `DateKey`），让 `merge_sessions` 复用同一结构化键。本方案选择 `pub(crate)` 而非移到 `model`，因为 `DeduplicationKey` 的字段全部来自 `Record`，且仅 commands 与 importer 两处使用，无需跨 crate 暴露。
- 删除 `command_date_key` 后，`commands.rs` 不再直接使用 `chrono::NaiveDateTime` 格式化；`use chrono::Local;` 仍需保留（`merge_sessions` 用 `Local::now()`）。
