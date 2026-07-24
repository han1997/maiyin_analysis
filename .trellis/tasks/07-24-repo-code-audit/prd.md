# 全仓代码体检与优化

## Goal

对麦隐研判（Tauri 2 + React 19 + TS）全仓做一次系统性代码体检，产出一份分维度、分优先级的改进清单，作为后续单项优化任务的入口。本任务**不修改任何业务代码**，只输出清单。

## What I already know

- 技术栈：Tauri 2 桌面壳 + React 19 + TypeScript 6 + Vite 8；Rust 后端（src-tauri/src），React 前端（src/）。
- 质量门：`npm run lint` / `npm run test` / `npm run build` / `cargo fmt --check` / `cargo test` / `cargo clippy -D warnings`。
- 规模热点（行数，git ls-files 统计）：
  - `src-tauri/src/storage.rs` 3704 行（明显偏大）
  - `src-tauri/src/importer.rs` 1154 行
  - `src/App.tsx` 1039 行（单组件偏大）
  - `src-tauri/src/analysis.rs` 931 行
  - `src/styles.css` 679 行
  - `src-tauri/src/commands.rs` 491 行
- 历史优化记录（.trellis/tasks/archive/2026-07）：folder import / import history / hotel result filters / analysis / fuzzy multi-region filters 等多轮性能与体验优化已完成。
- Spec 状态：frontend/backend 部分指南仍为 "To fill"；Active 的有 frontend/state-management、quality-guidelines、type-safety，backend/database-guidelines、tauri-contract。

## Decisions (locked)

- **交付形态**：只出改进清单，不修改业务代码；后续每项优化各自建子任务。
- **维度**：性能 / 结构 / 代码质量 / 类型安全，四维度等权。
- **产物**：`{TASK_DIR}/audit-report.md`，不污染 prd.md。
- **分级**：每条发现带严重度（P0 阻断 / P1 高 / P2 中 / P3 低）与风险标签（低风险可立即修 / 架构性需评估），并附建议的后续任务 slug。
- **清单结构**：先列“质量门现状”，再按维度分节，每节内按严重度排序，最后汇总“速赢清单”与“建议子任务列表”。

## Assumptions (temporary)

- 若 lint / test / clippy 当前存在失败或告警，其本身即作为发现记录。

## Requirements (evolving)

- 跑通并记录前端 `lint` / `test` / `build` 与 Rust `fmt --check` / `test` / `clippy -D warnings` 的现状。
- 全仓扫描代码质量、结构、性能隐患、类型安全，按维度归类。
- 每条发现包含：维度、严重度、文件:行号、问题描述、建议方向、建议的后续任务 slug。
- 产出一份分级改进清单文档，作为后续子任务的入口。
- 本任务不修改业务代码。

## Acceptance Criteria (evolving)

- [ ] 质量门当前状态已记录（通过/失败/告警数量）。
- [ ] 改进清单覆盖四个维度，每条有文件:行号定位。
- [ ] 清单按严重度排序，含后续任务 slug 建议。
- [ ] 用户确认清单可作为后续子任务入口。

## Definition of Done (team quality bar)

- 体检报告文档完成并由用户确认。
- 未修改任何业务代码（工作树仅 .trellis/ 下有新增）。
- 后续优化路径清晰（每个高/中优先项有建议的子任务）。

## Out of Scope (explicit)

- 任何业务代码修改（留待后续子任务）。
- 重构落地（如拆分 storage.rs / App.tsx）。
- 性能基准压测（仅识别隐患）。

## Technical Notes

- 产物建议路径：`{TASK_DIR}/audit-report.md`。
- 维度参考：性能（热点循环、重复计算、数据传输）、结构（超长文件/函数、职责混杂）、代码质量（重复代码、坏味道、错误处理）、类型安全（any、断言、边界类型）。
- Spec 索引：`.trellis/spec/frontend/index.md`、`.trellis/spec/backend/index.md`。
