# 移除 analysis 生产路径的 .expect()

## Goal

移除 `analysis.rs` 中 `different_accommodation_cached` 和 `day_ranges` 的 4 处 `.expect()`，改为 `Option`/`Result` 或直接使用 `entry` API 的返回值。消除 `spawn_blocking` 上的 panic 面——一旦未来重构破坏不变式，`.expect()` 会让整次重算以 `task_error` 失败且无结构化错误。**行为不变。**

## What I already know

### 4 处 .expect() 定位

1. **`different_accommodation_cached` 363-364**：
   ```rust
   cache.entry(first.uid).or_insert_with(|| (...));
   cache.entry(second.uid).or_insert_with(|| (...));
   let first_location = cache.get(&first.uid).expect("first location is cached");
   let second_location = cache.get(&second.uid).expect("second location is cached");
   ```
   不变式：`or_insert_with` 刚插入了 key，紧接着 `get` 必能命中。但 `entry().or_insert_with()` 本身返回 `&mut V`，完全不需要再 `get` 一次。

2. **`day_ranges` 379**：
   ```rust
   let day = record
       .check_in
       .expect("scoped analysis records have check-in times")
       .date();
   ```
   不变式：`within_analysis_time_window`（334）对 `check_in = None` 返回 `false`，过滤掉无 check_in 的记录，所以传入 `day_ranges` 的记录都有 check_in。但防御性处理更安全。

3. **`day_ranges` 382**：
   ```rust
   if days.last().is_some_and(|current| current.day == day) {
       days.last_mut().expect("day exists").end = index + 1;
   }
   ```
   不变式：`is_some_and` 返回 `true` 意味着 `days` 非空，所以 `last_mut()` 必返回 `Some`。但 `if let` 更惯用。

### 调用链

- `analyze_records`（72）→ `within_analysis_time_window` 过滤 → `grouped` → `analyze_person`（124）→ `day_ranges`（130）
- `analyze_person`（136-158）overlap 循环 → `different_accommodation_cached`（151）
- `analyze_records` 运行在 `par_iter()`（88）→ `spawn_blocking` 上，panic 转为 `task_error`

## Decisions (locked)

- **方案**：
  1. `different_accommodation_cached`：直接使用 `entry().or_insert_with()` 的返回值 `&mut V`，删除 `get().expect()` 两行。
  2. `day_ranges` 379：改为 `let Some(check_in) = record.check_in else { continue; }; let day = check_in.date();`。
  3. `day_ranges` 382：改为 `if let Some(current) = days.last_mut() { current.end = index + 1; }`。
- **行为不变**：4 处改动都不改变运行时行为（在当前不变式下，`expect` 从不触发，新代码的 else 分支也不触发）。
- **不新增测试**：现有 45 个测试（含 `analysis_regression_checksum`）守卫行为。
- **质量门**：`cargo test` 45 passed / 8 ignored、`cargo fmt --check`、`cargo clippy -D warnings` 全绿。

## Requirements

### 修复 1：`different_accommodation_cached`（363-364）

原：
```rust
cache.entry(first.uid).or_insert_with(|| (compact(&first.hotel_name), compact(&first.room_no)));
cache.entry(second.uid).or_insert_with(|| (compact(&second.hotel_name), compact(&second.room_no)));
let first_location = cache.get(&first.uid).expect("first location is cached");
let second_location = cache.get(&second.uid).expect("second location is cached");
```

新：
```rust
let first_location = cache
    .entry(first.uid)
    .or_insert_with(|| (compact(&first.hotel_name), compact(&first.room_no)));
let second_location = cache
    .entry(second.uid)
    .or_insert_with(|| (compact(&second.hotel_name), compact(&second.room_no)));
```

`or_insert_with` 返回 `&mut (String, String)`，后续代码只读取 `.0` 和 `.1`（无 mutation），与 `get` 返回的 `&(String, String)` 在读取场景完全等价。

### 修复 2：`day_ranges` 379

原：
```rust
let day = record
    .check_in
    .expect("scoped analysis records have check-in times")
    .date();
```

新：
```rust
let Some(check_in) = record.check_in else {
    continue;
};
let day = check_in.date();
```

### 修复 3：`day_ranges` 382

原：
```rust
if days.last().is_some_and(|current| current.day == day) {
    days.last_mut().expect("day exists").end = index + 1;
} else {
    days.push(DayAnalysis {
        day,
        start: index,
        end: index + 1,
        overlap: None,
    });
}
```

新：
```rust
if days.last().is_some_and(|current| current.day == day) {
    if let Some(current) = days.last_mut() {
        current.end = index + 1;
    }
} else {
    days.push(DayAnalysis {
        day,
        start: index,
        end: index + 1,
        overlap: None,
    });
}
```

## Acceptance Criteria

- [ ] `different_accommodation_cached` 不再有 `.expect()`。
- [ ] `day_ranges` 不再有 `.expect()`。
- [ ] `analysis.rs` 生产路径（非 `#[cfg(test)]`）无 `.expect()`。
- [ ] `cargo test` 通过数仍为 45 passed / 8 ignored。
- [ ] `cargo fmt --check` 无 diff。
- [ ] `cargo clippy --all-targets -- -D warnings` 零告警。
- [ ] `git diff` 仅 `src-tauri/src/analysis.rs` 有改动。

## Definition of Done

- 上述 AC 全部满足。
- 运行时行为完全一致（`analysis_regression_checksum` 守卫）。

## Out of Scope

- 改变分析逻辑、算分公式、预警规则。
- 处理 `#[cfg(test)]` 测试代码中的 `.expect()`/`unwrap()`（测试代码用 panic 是惯用且可接受的）。
- 新增测试。

## Technical Notes

- 审计报告 P2 #9 原文：改为返回 `Option`/`Result` 并在调用处显式处理（或 `unwrap_or` 配文档化回退），消除生产路径的 panic 面。
- `different_accommodation_cached` 的修复方案比审计建议更直接——`entry` API 本身就返回引用，不需要 `get` + `expect` 的两步式。
- `day_ranges` 的 `check_in` 修复用 `continue` 而非 `unwrap_or` 回退值，因为无 `check_in` 的记录不应参与日期分桶——这与 `within_analysis_time_window` 的过滤语义一致。
