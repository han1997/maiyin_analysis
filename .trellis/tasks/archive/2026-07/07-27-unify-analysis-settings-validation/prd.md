# Unify analysis settings validation

## Goal

消除 `applySettings`（前端 `src/App.tsx:286-306`）与 `validate_settings`（后端 `src-tauri/src/commands.rs:388-421`）的校验规则漂移：当前两处各写一遍"阈值边界 / selected 必填 / 起止顺序"，且已存在上界检查与错误消息格式不一致。提炼共享常量并对齐语义，加交叉测试守卫，行为不变（除修正前端缺失的上界检查与对齐错误消息）。

## What I already know

* 审计报告定位：`src/App.tsx:270-290`（前端）与 `src-tauri/src/commands.rs:388-421`（后端），P2 代码质量。审计原文："同一套'阈值 ≥ 1 / selected 需同时有起止 / 起不晚于止'规则在两层各写一遍。前端版本是为提交前给即时 toast，但规则一旦在 Rust 侧调整，前端极易漏改而漂移。把阈值边界与'selected 必填项'这类纯规则提炼为共享常量/JSON 契约（或至少在前端注释标注'须与 commands::validate_settings 同步'），并在两层加交叉测试守卫。"
* 前端 `applySettings` (App.tsx:286-306)：
  * `activeThresholds = mode==="selected" ? [frequencyThreshold] : [week, month, year]`
  * `activeThresholds.some((value) => value < 1)` → toast "频次阈值必须是大于 0 的整数。"
  * `mode==="selected" && (!frequencyStart || !frequencyEnd)` → toast "选定入住时间范围时，开始时间和结束时间均为必填。"
  * `mode==="selected" && frequencyStart > frequencyEnd` → toast "入住开始时间不能晚于结束时间。"
* 后端 `validate_settings` (commands.rs:388-421)：
  * `thresholds = mode==Selected ? [("时间窗口", frequency_threshold)] : [("7 天", week), ("30 天", month), ("365 天", year)]`
  * `for (label, value) in thresholds { if !(1..=99999).contains(&value) → Err "{label}阈值应在 1 到 99999 之间" }`
  * `mode==Selected && (start.is_none() || end.is_none())` → Err "选定入住时间范围时，开始时间和结束时间均为必填"
  * `mode==Selected && start.zip(end).is_some_and(|(s,e)| s > e)` → Err "入住开始时间不能晚于结束时间"
* **已知漂移点**：
  1. 阈值上界：前端 `< 1`（无上界），后端 `1..=99999`（有上界 99999）。前端缺失上界检查，允许提交 `> 99999` 的值，后端会拒绝但前端 toast 文案不匹配。
  2. 错误消息：前端统一"频次阈值必须是大于 0 的整数。"，后端带 label + 范围"{label}阈值应在 1 到 99999 之间"。UI 提交后显示的是后端 CommandError message，与前端 toast 文案不一致。
  3. 时间比较：前端 `frequencyStart > frequencyEnd` 是字符串比较（`frequencyStart: string | null`，ISO8601 字典序），后端 `NaiveDateTime >` 比较。ISO8601 格式一致时字典序 == 时间序，等价；但类型不同。
* 后端已有测试 `validates_analysis_thresholds_and_time_order` (commands.rs:457-501) 覆盖：阈值 0 拒绝、selected 缺边界拒绝、selected 起止倒置拒绝、起止正常接受、rolling 模式忽略 inactive 字段。
* `AnalysisSettings` 字段：`frequencyMode: "rolling" | "selected"`、`frequencyStart/End: string | null`（前端）/ `Option<NaiveDateTime>`（后端）、`frequencyThreshold/weekThreshold/monthThreshold/yearThreshold: number/i64`。
* 默认值：阈值 `3, 3, 12, 144`（week/month/year/frequency）。
* 相关审计记录：`.trellis/tasks/archive/2026-07/07-24-repo-code-audit/audit-report.md` 3.3 节第 1 条。

## Assumptions

* 不引入跨语言代码生成（P2 风险收益不匹配）。
* 前端对齐后端语义（上界检查 + 错误消息格式），而非反向。
* 不改变后端 `validate_settings` 的任何行为（它是 source of truth）。
* 不改变 `AnalysisSettings` 类型定义。
* 前端 toast 仍作为提交前即时反馈，后端校验仍是最终守卫。

## Open Questions

* None — 已确认采用 Approach A（抽前端校验模块）。

## Requirements

* 新增 `src/domain/validation.ts`，导出：
  * `THRESHOLD_MIN = 1`、`THRESHOLD_MAX = 99999` 常量。
  * `THRESHOLD_LABELS: Record<"weekThreshold" | "monthThreshold" | "yearThreshold" | "frequencyThreshold", string>`（值 `"7 天"` / `"30 天"` / `"365 天"` / `"时间窗口"`，对齐后端 label）。
  * `validateAnalysisSettings(settings: AnalysisSettings): string | null` —— 返回错误消息或 `null`（通过）。内部逻辑与后端 `validate_settings` 语义对齐：
    * 阈值边界 `1..=99999`（含上界，对齐后端），错误消息 `"{label}阈值应在 1 到 99999 之间"`（带 label + 范围，对齐后端）。
    * selected 必填检查，错误消息 `"选定入住时间范围时，开始时间和结束时间均为必填"`（对齐后端，去掉前端原有的句尾句号）。
    * selected 起止顺序检查（字符串比较，ISO8601 一致时等价于后端 NaiveDateTime 比较），错误消息 `"入住开始时间不能晚于结束时间"`（对齐后端）。
  * 文件顶部注释标注"须与 commands::validate_settings 同步"。
* `applySettings` (App.tsx:286-306) 改为调用 `validateAnalysisSettings(draftSettings)`，返回非 null 时 `setToast({ tone: "error", message })` 并 return。
* 后端 `validate_settings` 不改（source of truth）。
* 新增 `src/domain/validation.test.ts`，覆盖：阈值 0 拒绝、阈值 100000 拒绝（上界）、阈值 1/99999 接受、selected 缺起止拒绝、selected 起止倒置拒绝、rolling 模式忽略 inactive 字段、错误消息格式对齐。
* 不改变 `AnalysisSettings` 类型定义、前端 toast 机制、`applySettings` 提交流程。

## Acceptance Criteria

* [ ] 前端 `applySettings` 通过 `validateAnalysisSettings` 检查阈值 `1..=99999`（含上界）。
* [ ] 前端阈值错误消息与后端一致（带 label + 范围 `"{label}阈值应在 1 到 99999 之间"`）。
* [ ] `src/domain/validation.ts` 提炼 `THRESHOLD_MIN/MAX` + `THRESHOLD_LABELS` + `validateAnalysisSettings`，前端无硬编码阈值上下界。
* [ ] `src/domain/validation.test.ts` 覆盖上界检查与对齐后的错误消息。
* [ ] 后端 `validate_settings` 行为不变，现有测试全绿。
* [ ] `npm run lint`、`npm run test`、`npm run build`、`cargo fmt --check`、`cargo test`、`cargo clippy --all-targets -- -D warnings` 全部通过。

## Definition of Done

* 改动范围：前端 `src/App.tsx`（+ 可能新增 `src/domain/` 下的常量/校验模块）、前端测试。
* 后端 `commands.rs` 不改（source of truth）。
* 质量门全绿（Rust 三项 + 前端三项）。

## Out of Scope

* 引入跨语言代码生成（JSON Schema → Rust/TS）。
* 改变后端 `validate_settings` 行为或错误消息。
* 改变 `AnalysisSettings` 类型定义。
* 改变前端 toast 机制或 `applySettings` 的提交流程。
* 统一时间比较类型（前端字符串 vs 后端 NaiveDateTime，ISO8601 一致时等价）。

## Technical Approach

采用 Approach A（抽前端校验模块）：

1. 新增 `src/domain/validation.ts`：
   ```ts
   // 须与 src-tauri/src/commands.rs::validate_settings 同步
   export const THRESHOLD_MIN = 1;
   export const THRESHOLD_MAX = 99999;
   export const THRESHOLD_LABELS = {
     weekThreshold: "7 天",
     monthThreshold: "30 天",
     yearThreshold: "365 天",
     frequencyThreshold: "时间窗口",
   } as const;
   export function validateAnalysisSettings(settings: AnalysisSettings): string | null { ... }
   ```
2. `validateAnalysisSettings` 内部逻辑与后端 `validate_settings` 语义对齐：
   * 按 `frequencyMode` 选活跃阈值（selected → `[frequencyThreshold]`；rolling → `[week, month, year]`）。
   * 每个阈值检查 `!(THRESHOLD_MIN <= value && value <= THRESHOLD_MAX)` → 返回 `"{label}阈值应在 1 到 99999 之间"`。
   * selected 模式检查 `!frequencyStart || !frequencyEnd` → 返回 `"选定入住时间范围时，开始时间和结束时间均为必填"`。
   * selected 模式检查 `frequencyStart > frequencyEnd`（字符串比较，ISO8601 一致时等价）→ 返回 `"入住开始时间不能晚于结束时间"`。
   * 全通过返回 `null`。
3. `applySettings` (App.tsx:286-306) 改为：
   ```ts
   const error = validateAnalysisSettings(draftSettings);
   if (error) { setToast({ tone: "error", message: error }); return; }
   ```
   删除原内联的 3 段检查。
4. 后端 `validate_settings` 不改（source of truth）。
5. 新增 `src/domain/validation.test.ts` 覆盖对齐后的语义与错误消息格式。

## Decision (ADR-lite)

**Context**: 前后端各写一遍相同校验规则，已存在上界检查与错误消息格式漂移；审计建议提炼共享常量 + 交叉测试守卫，但不建议跨语言代码生成（P2 风险收益不匹配）。

**Decision**: 采用 Approach A —— 前端抽 `src/domain/validation.ts`（常量 + `validateAnalysisSettings`），`applySettings` 调用它；后端 `validate_settings` 作为 source of truth 不改；注释标注同步关系；前端测试覆盖。前端对齐后端语义（补上界检查 + 对齐错误消息）。

**Consequences**: 消除已知漂移（上界 + 消息格式），前端校验逻辑单点维护，为未来扩展留结构；仍需人工同步两层规则（无代码生成），但注释 + 交叉测试降低漏改风险；后端行为零变化。

## Technical Notes

* 前端文件：`src/App.tsx`（`applySettings` 286-306）、`src/domain/types.ts`（`AnalysisSettings` 定义 + 默认值 202-207）。
* 后端文件：`src-tauri/src/commands.rs`（`validate_settings` 388-421、测试 457-501）。
* 后端是 source of truth，前端对齐。
* 质量命令：`npm run lint`、`npm run test`、`npm run build`、`cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo test --manifest-path src-tauri/Cargo.toml`、`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`。
