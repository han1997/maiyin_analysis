# 简化并美化分析界面

## Goal

在不改变现有导入、分析、筛选、详情和导出能力的前提下，降低主界面的视觉噪音与操作认知负担，让用户能按“导入数据 → 查看风险结果 → 核查详情 → 导出材料”的顺序快速完成工作。

## What I already know

- 产品是 Windows 办公场景中的本地 Tauri 数据研判工具，界面应克制、可靠、可核查。
- 当前主界面同时展示导入、历史会话、分析参数、存储目录、双搜索框、两类筛选、应用/重置和三种导出操作。
- 分析参数在左侧栏和完整设置侧板重复出现。
- 人员详情已经使用右侧检查器，适合保留，不应改成阻断主表的模态框。
- 表格扫描效率、显式风险文字和敏感信息控制必须保留。
- 现有技术栈为 React + TypeScript + 原生 CSS，不迁移框架或引入新的 UI 库。

## Assumptions (temporary)

- 保持浅色办公工具风格和单一深蓝强调色，重点改进信息架构、间距、层级、文案与交互状态，而非进行装饰性重做。
- 后端接口和赋分逻辑不变。

## Open Questions

- 无。

## Requirements (evolving)

- 保留全部现有业务能力与赋分结果。
- 采用渐进披露：主界面只常驻导入、人员搜索、主要风险筛选和结果浏览；高级参数、附加筛选与导出格式按需展开。
- 减少重复入口和同时可见的次要操作。
- 强化导入、查看风险结果、核查详情、导出四个核心动作的顺序感。
- 保证键盘焦点、风险文字标签、窄窗口适配和 reduced-motion 支持。
- 提供清晰的加载、空数据、无筛选结果和错误状态。
- 同步优化首次打开、无数据、加载、无搜索结果和窄窗口场景，确保渐进披露规则在完整流程中一致。

## Acceptance Criteria (evolving)

- [ ] 用户首次打开界面时能立即识别主要导入入口。
- [ ] 导入后，人员搜索、风险筛选和结果表成为主要视觉焦点。
- [ ] 分析参数不再在两个位置重复常驻。
- [ ] 低频导出格式不会占据多个常驻按钮，但所有导出能力仍可访问。
- [ ] 人员详情仍从右侧展开且不会遮断主工作流。
- [ ] 首次打开、加载、空数据、无搜索结果和窄窗口状态具有明确的下一步操作。
- [ ] 原有前端测试、类型检查、Lint 和构建通过。

## Definition of Done

- Tests added/updated where interaction structure changes.
- Lint, typecheck, tests and production build pass.
- UI behavior remains compatible with Tauri and browser demo runtime.
- Relevant design/spec notes are updated if conventions change.

## Out of Scope

- 修改分析算法、赋分阈值或 Rust 数据接口。
- 引入新的前端框架、组件库或远程服务。
- 营销页式视觉、深色指挥大屏、地图装饰或夸张动效。

## Technical Notes

- Primary files: `src/App.tsx`, `src/styles.css`.
- Supporting components: `src/components/Icon.tsx`, `src/components/StatStrip.tsx`, `src/components/RiskBadge.tsx`.
- Product constraints: `PRODUCT.md`, `DESIGN.md`.
- Frontend specs: `.trellis/spec/frontend/index.md`, `.trellis/spec/frontend/type-safety.md`.
- Design direction follows progressive disclosure, compact data-table scanning, right-side inspection, WCAG 2.1 AA, and restrained 150–220 ms motion.

## Decision (ADR-lite)

**Context**: 当前功能完整，但侧栏、工具栏和设置面板同时暴露大量同级操作，用户需要反复辨认高频与低频入口。

**Decision**: 采用渐进披露结构。高频路径保持直接可见；高级参数、附加筛选和导出格式通过展开区、菜单或侧板提供。

**Consequences**: 主界面更聚焦，既有能力仍然可达；需要为折叠状态、键盘操作和窄窗口行为补充测试与样式。

## Technical Approach

- 将侧栏重构为以导入和当前数据源为核心的控制区，分析参数仅保留摘要与单一“调整参数”入口。
- 将结果工具栏分为常驻的人员搜索与主要风险筛选，以及按需展开的旅馆搜索、预警筛选和重置操作。
- 将多个导出按钮合并为一个主导出入口，在菜单中选择具体格式。
- 保留统计带、人员/记录页签和右侧详情检查器，但重新梳理间距、标题层级、按钮优先级和状态反馈。
- 在不改变 API 与领域类型的前提下完成前端结构与样式调整，并更新受影响的交互测试。
