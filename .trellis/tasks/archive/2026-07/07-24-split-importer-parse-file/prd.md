# 拆分 parse_file 为定起始+列映射与逐行构造

## Goal

将 `importer.rs` 中 ~115 行的 `parse_file` 拆分为三个职责清晰的单测友好函数：`resolve_data_start_and_indexes`（表头/模板/推断分派）、`build_record`（逐行字段抽取与 Record 构造）、`parse_file`（编排）。**行为不变，解析结果与统计完全一致。**

## What I already know

### 当前 parse_file 结构（185-300）

- **185-186**：`read_table(path)` 读表。
- **187-210**：表头/模板/推断三段式分派，确定 `data_start` 和 `indexes`。失败时返回空 `ParsedFile` 带 reason。
- **212-214**：初始化 `stats`、`records`、`source_file`、`today`。
- **216-294**：逐行循环，含空行跳过、id/check_in 缺失统计、短住统计与跳过、日期解析/校验、身份区域查询、Record 构造。
- **295-300**：返回 `ParsedFile`。

### 类型定义

- `FieldIndexes = HashMap<&'static str, Vec<usize>>`（464）。
- `detect_header_row` 返回 `(usize, FieldIndexes, usize)`（466）—— header_index, indexes, score。
- `detect_template_data_start` 返回 `Option<usize>`（533）。
- `infer_core_fields` 返回 `Option<(usize, FieldIndexes)>`（542）。
- `template_indexes()` 返回 `FieldIndexes`（596）。
- `ParsedFile { records, stats, reason }`（48-52）。
- 逐行逻辑的副作用：`stats.missing_id_count += 1`（223）、`stats.short_stay_count += 1`（244，含 `continue`）。

### 已有辅助函数（被逐行逻辑调用）

`pick`、`compact_identity`、`parse_datetime`、`parse_date`、`parse_age`、`identity_birth_date`、`normalize_gender`、`lookup_identity_area`、`nonempty`、`calculate_age`（model.rs）。

## Decisions (locked)

- **拆分方案**：抽出 `resolve_data_start_and_indexes` 和 `build_record` 两个函数，`parse_file` 仅做编排。
- **stats 副作用处理**：`build_record` 返回 `RowOutcome` 枚举（`Skip`/`MissingId`/`ShortStay`/`Valid(Record)`），由 `parse_file` 根据结果更新 stats。这保持 stats 累积逻辑在编排层，`build_record` 纯计算无副作用。
- **行为不变**：解析结果、stats（missing_id_count/short_stay_count）、reason 字符串完全一致。
- **不新增测试**：现有 45 个测试（含 importer 的确定性/重复/并行测试）守卫行为。
- **质量门**：`cargo test` 45 passed / 8 ignored、`cargo fmt --check`、`cargo clippy -D warnings` 全绿。

## Requirements

### 新增 `RowOutcome` 枚举

```rust
enum RowOutcome {
    Skip,        // 空行，跳过不计
    MissingId,   // id_no 或 check_in 为空
    ShortStay,   // 入住不足 10 分钟
    Valid(Record),
}
```

### 新增 `resolve_data_start_and_indexes`

```rust
fn resolve_data_start_and_indexes(
    rows: &[Vec<String>],
    file_name: &str,
) -> Result<(usize, FieldIndexes), String>
```

- 封装 187-210 的三段式分派逻辑。
- 成功返回 `Ok((data_start, indexes))`。
- 失败返回 `Err(reason)`，reason 格式与原代码一致：`"{} 未识别到证件号码或入住时间列（表头得分 {}）"`。

### 新增 `build_record`

```rust
fn build_record(
    row: &[String],
    row_index: usize,
    indexes: &FieldIndexes,
    source_file: &str,
    today: NaiveDate,
) -> RowOutcome
```

- 封装 216-293 的逐行逻辑。
- 空行 → `RowOutcome::Skip`。
- id_no 或 check_in 为空 → `RowOutcome::MissingId`。
- 短住（< 10 分钟）→ `RowOutcome::ShortStay`。
- 正常 → `RowOutcome::Valid(Record { ... })`。
- `issues` Vec 的构建逻辑不变。

### 重构 `parse_file`

```rust
fn parse_file(path: &Path) -> Result<ParsedFile, AppError> {
    let rows = read_table(path)?;
    let source_file = file_name(path);
    let (data_start, indexes) = match resolve_data_start_and_indexes(&rows, &source_file) {
        Ok(result) => result,
        Err(reason) => return Ok(ParsedFile {
            records: vec![], stats: ImportStats::default(), reason: Some(reason),
        }),
    };
    let mut stats = ImportStats::default();
    let mut records = Vec::with_capacity(rows.len().saturating_sub(data_start));
    let today = Local::now().date_naive();
    for (row_index, row) in rows.iter().enumerate().skip(data_start) {
        match build_record(row, row_index, &indexes, &source_file, today) {
            RowOutcome::Skip => {}
            RowOutcome::MissingId => stats.missing_id_count += 1,
            RowOutcome::ShortStay => stats.short_stay_count += 1,
            RowOutcome::Valid(record) => records.push(record),
        }
    }
    Ok(ParsedFile { records, stats, reason: None })
}
```

## Acceptance Criteria

- [ ] `resolve_data_start_and_indexes` 抽取完成，签名与上述一致。
- [ ] `build_record` 抽取完成，返回 `RowOutcome`。
- [ ] `parse_file` 仅做编排（读表 + 调 resolver + 循环调 build_record + 累积 stats + 返回）。
- [ ] `RowOutcome` 枚举定义完成。
- [ ] `cargo test` 通过数仍为 45 passed / 8 ignored。
- [ ] `cargo fmt --check` 无 diff。
- [ ] `cargo clippy --all-targets -- -D warnings` 零告警。
- [ ] `git diff` 仅 `src-tauri/src/importer.rs` 有改动。

## Definition of Done

- 上述 AC 全部满足。
- 解析结果、stats、reason 与改动前完全一致。

## Out of Scope

- 改变解析逻辑、日期校验、短住阈值、身份区域查询等任何业务规则。
- 改变 `detect_header_row`/`detect_template_data_start`/`infer_core_fields`/`template_indexes` 的实现。
- 新增测试。
- 拆分 `read_table` 或其他函数。

## Technical Notes

- `RowOutcome` 为模块私有枚举，无需 `pub`。
- `build_record` 接收 `row: &[String]` 而非 `row: &Vec<String>`，符合 Rust 惯例。
- `source_file` 以 `&str` 传入 `build_record`，避免每行 clone。
- `today: NaiveDate` 从 `Local::now().date_naive()` 获取，在 `parse_file` 中只调用一次后传入循环。
- 审计报告 P2 #6 原文：拆出 `resolve_data_start_and_indexes(&rows) -> (start, indexes)` 与 `build_record(row, indexes, ...) -> Option<Record>`。本方案用 `RowOutcome` 枚举替代 `Option<Record>` 以干净处理 stats 副作用。
