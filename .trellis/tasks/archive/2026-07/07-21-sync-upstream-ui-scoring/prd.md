# 同步上游界面功能与赋分规则

## Goal

将 `origin/main` 最新 Python 桌面版的有效功能、交互和风险赋分合同移植到当前 React + Tauri 应用，保持当前技术栈和产品视觉，同时确保分析结果、证据和导出口径与上游一致。

## What I already know

- 两个分支技术栈已经分叉，不能直接合并。
- 用户明确要求以远端最新提交为基准更新界面和功能，并确保赋分一致。
- 上游最新核心合同已记录在 `research/upstream-parity.md`。
- 当前本地实现仍使用旧的重合判断、30/365 天阈值和赋分公式。

## Decision

- 用户选择完整同步所有已识别的核心规则与界面功能。
- 当前 React + Tauri 架构、浅色办公产品风格和本地优先约束保持不变。

## Requirements (evolving)

- 赋分公式、触发条件、等级边界、证据范围与上游完全一致。
- 新增选定入住时间窗口、7/30/365 天阈值以及互斥频次计分。
- 分析时间范围同步约束结果、详情、证据、统计和导出。
- UI 参数和筛选必须显式应用，不因每次输入而触发重分析或昂贵过滤。
- 新增旅馆名称模糊搜索、导入记录页签和截断内容悬浮提示。
- 日期时间输入必须支持稳定焦点、手动录入和直观日期时间选择。
- 分析实现采用上游等价的排序复用、地点归一化缓存和滑动窗口优化。
- 保留当前 React + Tauri 架构，不迁移回 Python/Tkinter。
- 历史会话缺少新增字段时可兼容加载。

## Acceptance Criteria (evolving)

- [x] Rust 赋分测试覆盖上游风险规范的 good/base/bad cases。
- [x] 相同输入和设置在本地与上游产生一致的预警种类、分数、等级和证据。
- [x] React 参数 UI 暴露选定时间窗口和 7/30/365 天阈值。
- [x] 结果筛选显式应用，旅馆名称支持模糊搜索。
- [x] 用户可在独立页签核查导入的原始入住记录。
- [x] 表格截断内容可通过原生提示查看完整值。
- [x] 结果、详情、导出和历史使用同一分析口径。
- [x] 现有导入、历史、导出和 XLS 兼容测试不回归。
- [x] 前后端 lint、类型检查和测试通过。

## Definition of Done

- 跨层类型与默认值同步。
- Rust 分析及性能路径完成并有测试。
- React UI/筛选/记录查看完成并有必要测试。
- README 和 Trellis 风险规范更新。
- 使用代表性数据做端到端核对。

## Out of Scope (explicit)

- 将当前项目替换为 Python/Tkinter。
- 直接合并或重置到 `origin/main`。
- 与风险分析无关的上游构建脚本迁移。

## Research References

- [`research/upstream-parity.md`](research/upstream-parity.md) - 上游规则、交互、性能变化和本地差距。

## Technical Notes

- 上游规则来源：`origin/main:.trellis/spec/desktop/risk-scoring-guidelines.md`。
- 主要本地文件：`src-tauri/src/{model,analysis,commands,exporter,storage}.rs`、`src/domain/types.ts`、`src/App.tsx`、`src/styles.css`。
- `impeccable` 产品上下文要求维持浅色、克制、密集但不拥挤的办公工具界面。
