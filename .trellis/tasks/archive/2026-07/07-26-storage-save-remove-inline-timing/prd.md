# Storage save remove inline timing

## Goal

把 `storage.rs` 中 `save` 函数内穿插的 `#[cfg(test)]` 计时块外置为门控 helper，让生产主干在源码阅读时不再被 `#[cfg(test)]` 注解打断，行为与计时能力保持不变。

## What I already know

* 审计报告定位：`src-tauri/src/storage.rs:190-203, 257-258, 330-331, 343-344, 362-363, 367-368, 370-371, 383-384`（行号已漂移，当前实际为 142-345 范围内），P3 代码质量。
* 当前实现：`save` 开头 3 处 `#[cfg(test)]` 设置（`save_timing_enabled` 读 `MAIYIN_SAVE_TIMINGS` env、`save_started = Instant::now()`、`save_mark` 闭包），加 7 处 `#[cfg(test)] save_mark("<label>")` 调用，标签依次为 `session_row` / `records_base` / `records_fts` / `records_and_fts` / `people_base` / `people_fts` / `commit`。
* 生产构建里全部被 `#[cfg(test)]` 移除，零运行时开销；`MAIYIN_SAVE_TIMINGS` 仅测试构建生效，输出 `save_stage=<label> elapsed_ms=<n>` 到 stderr。
* 问题纯粹是源码可读性：阅读 `save` 时被 10 处 `#[cfg(test)]` 块打断生产 SQL 流程。
* `save` 是 `StoredSession` 的持久化主入口，承载 session 行写入 + records/FTS/filter_counts 批量插入 + analysis + people search index + commit，约 200 行，逻辑紧凑、事务边界敏感。
* 计时被现有测试/集成路径间接使用（打印分阶段耗时），移除会丢失可观测性。
* 相关审计记录：`.trellis/tasks/archive/2026-07/07-24-repo-code-audit/audit-report.md` 3.3 节最后一条。

## Assumptions

* 纯行为保持 refactor：计时输出格式、env 门控、标签集合、stderr 输出全部不变。
* 不拆分 `save` 为多个子函数（避免引入事务/错误传播语义变化的风险）。
* 不引入新依赖。

## Open Questions

* None — 已确认采用 Approach C（计时 helper 结构）。

## Requirements

* 移除 `save` 函数体内所有 `#[cfg(test)]` 计时注解，让生产主干连续可读。
* 保留 `MAIYIN_SAVE_TIMINGS` 门控的分阶段耗时输出能力，输出格式与标签集合不变。
* 不改变 `save` 的事务结构、SQL 顺序、错误传播或任何对外行为。

## Acceptance Criteria (evolving)

* [ ] `save` 函数体内不再出现 `#[cfg(test)]` 计时块（设置与调用全部外置）。
* [ ] 计时能力保留：测试构建下设置 `MAIYIN_SAVE_TIMINGS` 仍输出 7 个 `save_stage=<label> elapsed_ms=<n>` 行，标签与格式与修改前一致。
* [ ] `cargo fmt --check`、`cargo test`、`cargo clippy --all-targets -- -D warnings` 全部通过。

## Definition of Done

* 重构范围限于 `src-tauri/src/storage.rs`。
* 质量门全绿（Rust 三项）。
* 不改变 `save` 的事务/SQL/错误语义，不改变计时输出文本。

## Out of Scope

* 拆分 `save` 为多个子函数（避免事务语义变化）。
* 改变 `MAIYIN_SAVE_TIMINGS` 的 env 名、输出格式或标签集合。
* 改变 `replace_analysis` 或其它 `SessionStore` 方法。
* 引入新的 benchmark 入口或外部计时库。

## Technical Approach

采用 Approach C（计时 helper 结构）：

1. 新增一个 `SaveTimer` 结构（或等价 helper），生产构建里字段为零大小、`mark` 为 no-op；测试构建里持有 `started: Instant` 与 `enabled: bool`，`mark` 在 `enabled` 时 `eprintln!("save_stage={} elapsed_ms={}", label, started.elapsed().as_millis())`。
2. `save` 开头改为 `let timer = SaveTimer::start();` 一行（替代当前 149-162 的三段 `#[cfg(test)]` 设置），所有 `#[cfg(test)]` 注解从 `save` 函数体内移除。
3. 7 处 `#[cfg(test)] save_mark("<label>");` 改为 `timer.mark("<label>");`，标签集合 `session_row` / `records_base` / `records_fts` / `records_and_fts` / `people_base` / `people_fts` / `commit` 与顺序不变。
4. helper 自身的 `#[cfg(test)]` 分支集中在它的定义处，`save` 主干连续可读。
5. 不动 `save` 的事务结构、SQL 顺序、错误传播；不动 `replace_analysis` 及其它方法。

## Decision (ADR-lite)

**Context**: `save` 内 10 处 `#[cfg(test)]` 块（3 设置 + 7 调用）穿插在 ~200 行事务逻辑中，生产构建零开销但源码阅读被打断；计时能力本身有价值（分阶段耗时观测），不应移除。

**Decision**: 采用 Approach C —— 抽一个零生产开销的 `SaveTimer` helper，把 `#[cfg(test)]` 注解从 `save` 体内全部外置到 helper 定义；`save` 主干只剩 `let timer = SaveTimer::start();` 与 7 处 `timer.mark("...");` 一行调用。

**Consequences**: `save` 主干连续可读，生产构建零开销保持（no-op 编译期消除）；计时输出格式、env 门控、标签集合与顺序完全不变；改动局限于 `storage.rs`，事务/SQL/错误语义零变化。

## Technical Notes

* 主要文件：`src-tauri/src/storage.rs`（`save` 142-345，计时设置 149-162，调用点 217/290/303/322/327/330/343）。
* 计时输出格式：`eprintln!("save_stage={} elapsed_ms={}", label, save_started.elapsed().as_millis())`。
* env 门控：`std::env::var_os("MAIYIN_SAVE_TIMINGS").is_some()`。
* 质量命令：`cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo test --manifest-path src-tauri/Cargo.toml`、`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`。
