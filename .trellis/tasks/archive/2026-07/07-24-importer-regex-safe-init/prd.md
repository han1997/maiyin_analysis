# importer 静态正则 unwrap 改 expect 带说明

## Goal

`importer.rs` 中 age 与 identity 正则经 `OnceLock::get_or_init` 编译并 `unwrap`。模式为常量，当前安全；但若日后误改成非法模式，首次调用即在导入热路径 panic 且无结构化错误。改 `expect` 带静态说明，降低误改即 panic 风险。**行为不变。**

## What I already know

### 2 处 unwrap 定位

1. **`parse_age` 778-780**：
   ```rust
   static AGE: OnceLock<Regex> = OnceLock::new();
   AGE.get_or_init(|| Regex::new(r"\d{1,3}").unwrap())
   ```

2. **`compact_identity` 794-795**：
   ```rust
   static VALUE: OnceLock<Regex> = OnceLock::new();
   VALUE.get_or_init(|| Regex::new(r"^(?:\d{17}[\dX]|\d{15})$").unwrap())
   ```

### 调用链

两处均在 importer 的解析热路径上（`parse_age` 和 `compact_identity` 被 `build_record` 调用）。`OnceLock::get_or_init` 的闭包只在首次调用时执行一次，但 panic 会在导入热路径触发。

## Decisions (locked)

- **方案**：2 处 `.unwrap()` 改为 `.expect("static regex pattern is valid")`。
- **行为不变**：正则模式不变，`expect` 在模式合法时与 `unwrap` 行为一致。
- **质量门**：`cargo test` 45 passed / 8 ignored、`cargo fmt --check`、`cargo clippy -D warnings` 全绿。

## Requirements

- `parse_age` 780：`.unwrap()` → `.expect("static regex pattern is valid")`
- `compact_identity` 795：`.unwrap()` → `.expect("static regex pattern is valid")`
- 不改动正则模式本身。

## Acceptance Criteria

- [ ] 2 处 `unwrap()` 改为 `expect("static regex pattern is valid")`。
- [ ] `cargo test` 通过数仍为 45 passed / 8 ignored。
- [ ] `cargo fmt --check` 无 diff。
- [ ] `cargo clippy --all-targets -- -D warnings` 零告警。
- [ ] `git diff` 仅 `src-tauri/src/importer.rs` 有改动。

## Out of Scope

- 改正则模式。
- 处理 `#[cfg(test)]` 中的 `unwrap()`（测试代码用 panic 是惯用且可接受的）。
- 引入编译期正则校验（如 `static-regex` crate）。

## Technical Notes

- 审计报告 P3 #17 原文：改 `expect` 带静态说明或编译期校验更稳妥。本方案选 `expect`——最小改动，不引入新依赖。
- `expect` 的消息 `"static regex pattern is valid"` 说明这些正则模式是编译时常量，正常情况下永远不会触发 panic。
