# Align PersonSummary max-count optionality

## Goal

统一 `PersonSummary` 中 `maxWeekCount`、`maxMonthCount`、`maxYearCount` 在 Rust serde 模型与 TypeScript DTO 中的可选性，消除跨层契约不一致，同时保留旧 SQLite 会话中缺少新增字段时的兼容能力。调整只涉及 DTO、缺省值和展示兜底，不改变分析、评分或筛选规则。

## What I already know

* 代码审查报告将问题定位为 P3 类型安全项：`src/domain/types.ts:45-47` 与 `src-tauri/src/model.rs:164-167`。
* `analyze_records` 在 `src-tauri/src/analysis.rs:301-303` 会同时填充三个计数字段，因此新生成的载荷始终包含三者。
* 当前只有 Rust 的 `max_week_count` 带 `#[serde(default)]`，TypeScript 只有 `maxWeekCount` 标记为可选；月、年字段为必填。
* `PersonSummary` 通过 SQLite `summary_json` 持久化；数据库 v4→v5 是保留历史数据的无损升级，旧摘要可能缺少后来新增的计数字段。
* 人员列表与详情目前只对 `maxWeekCount` 使用 `?? 0`，月、年字段直接渲染；浏览器 demo 也省略了 `maxWeekCount`。
* Rust DTO 使用 camelCase 序列化，React 不能复制或改变 Rust 的业务评分逻辑。

## Assumptions

* 历史会话可正常打开优先于严格拒绝缺字段载荷。
* 缺失计数的展示缺省值沿用现有周窗口行为，按 `0` 显示；显式的 `0` 保持为 `0`。
* 不增加数据库 schema 版本，不迁移或重算历史分析结果。

## Open Questions

* None — 用户已选择兼容优先方案（方案 A）。

## Requirements

* Rust `PersonSummary` 的 `max_week_count`、`max_month_count`、`max_year_count` 全部带 `#[serde(default)]`，缺失时反序列化为 `0`。
* TypeScript `PersonSummary` 的 `maxWeekCount`、`maxMonthCount`、`maxYearCount` 全部标记为可选，并继续使用 camelCase。
* 人员列表、人员详情和浏览器 fixture 对三个字段统一使用 `?? 0` 或等价的安全显示逻辑。
* 增加/更新 legacy JSON 测试，覆盖任意 max-count 字段缺失（包括三个字段全部缺失）仍可成功反序列化；新载荷仍序列化三个字段。
* 导出、筛选、评分和统计计算行为保持不变。

## Expansion sweep

### Future evolution

未来若增加其它时间窗口统计，应沿用“旧字段缺失安全回退”的兼容模式，避免新增字段破坏历史会话。

### Related scenarios

人员列表、详情、CSV 导出和浏览器 demo 都消费同一个 `PersonSummary`，应共享同一套缺省语义，而不是各自猜测字段是否存在。

### Failure and edge cases

覆盖旧摘要只缺周字段、只缺月/年字段、三个字段都缺失，以及字段显式为 `0`；这些情况均不得导致启动、查询或详情页面崩溃。

## Acceptance Criteria

* [x] 三个 `max*Count` 字段在 Rust 与 TypeScript 中都表达为可缺省。
* [x] 旧摘要缺少任意或全部 max-count 字段时仍能成功反序列化，并得到零值安全缺省。
* [x] 新分析结果仍序列化三个 camelCase 字段，数值与修改前一致。
* [x] 人员列表、详情和浏览器 fixture 在字段缺失/为零时稳定渲染。
* [x] `cargo fmt --check`、`cargo test`、`cargo clippy --all-targets -- -D warnings`、`npm run lint`、`npm run test`、`npm run build` 全部通过。

## Definition of Done

* 测试覆盖兼容边界并记录验证结果。
* Rust/TypeScript DTO 与 `.trellis/spec/backend/tauri-contract.md` 的 camelCase 约定一致。
* 不引入 `any`，不复制评分逻辑，不修改数据库迁移策略。
* 变更范围限于必要的模型、展示兜底和测试文件。

## Out of Scope

* 改变 7/30/365 天窗口的计算或评分规则。
* 改变 SQLite 数据库版本、迁移或历史会话清理策略。
* 重构 `PersonSummary` 之外的 DTO、筛选器或导出格式。
* 新增其它统计窗口或新的 UI 配置项。

## Technical Approach

采用兼容优先的方案：

1. Rust 三个 max-count 字段都添加 `#[serde(default)]`。
2. TypeScript 三个字段都改为可选。
3. 列表、详情和 fixture 统一使用 `?? 0` 展示。
4. 在 `src-tauri/src/model.rs` 增加覆盖部分/全部字段缺失的 legacy JSON 测试，并保留新载荷字段存在性的断言。

## Decision (ADR-lite)

**Context**: 三个计数字段由新分析同时生成，但历史摘要可能没有后来新增的字段；当前跨层类型只镜像了 Rust 不一致的默认策略。

**Decision**: 采用方案 A：三个 Rust 字段均使用 serde 默认值，三个 TypeScript 字段均可选，UI 统一以零值兜底。

**Consequences**: 历史会话继续可读，跨层契约语义一致；调用方需要接受字段缺失，并在展示处提供零值兜底。新载荷格式和业务数值不变。

## Implementation Plan

* PR1（模型契约）：更新 Rust `PersonSummary` serde 默认值与 TypeScript `PersonSummary` 可选字段。
* PR2（消费端）：更新人员表、详情 inspector、浏览器 fixture/相关测试的零值兜底。
* PR3（验证）：补齐 legacy 序列化/反序列化测试，运行 Rust 与前端质量命令并检查跨层字段名。

## Technical Notes

* 主要模型：`src-tauri/src/model.rs`、`src/domain/types.ts`。
* 生成路径：`src-tauri/src/analysis.rs`；导出消费：`src-tauri/src/exporter.rs`。
* UI 消费：`src/App.tsx`、`src/components/DetailInspector.tsx`；fixture：`src/data/demo.ts`。
* 兼容测试位置：`src-tauri/src/model.rs` 的 `legacy_person_summary_defaults_structured_household_fields` 附近。
* 质量命令：`npm run lint`、`npm run test`、`npm run build`、`cargo fmt --check`、`cargo test`、`cargo clippy --all-targets -- -D warnings`。
* 相关审查记录：`.trellis/tasks/archive/2026-07/07-24-repo-code-audit/audit-report.md`。

## Verification

* `npm run lint` — passed.
* `npm run test` — 23 passed.
* `npm run build` — passed.
* `cargo fmt --manifest-path src-tauri/Cargo.toml --check` — passed.
* `cargo test --manifest-path src-tauri/Cargo.toml` — 45 passed, 8 ignored benchmarks.
* `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` — passed.
