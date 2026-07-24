# 拆分 App.tsx 为组件与自定义 hook

## Goal

将 `src/App.tsx`（~1104 行）拆分为子组件文件与自定义 hook，降低单文件体积与重渲染范围，提升可测试性。**行为不变，公共渲染结果与交互逻辑保持一致。**

## What I already know

- 技术栈：React 19 + TypeScript 6 strict + Vite 8。无第三方状态库（纯 useState/useEffect）。
- `src/App.tsx` 结构（行号）：
  - 常量/类型/初始状态：`21-70`（BusyAction、ToastState、riskLevels、exportActions、pageSizeOptions、initialQuery、initialPage、initialRecordsPage）
  - 主 `App` 组件：`72-751`（~680 行，11 useState / 5 useEffect / 0 useCallback / 0 useMemo）
  - 9 个内联子组件：
    - `TableSkeleton` 753、`ImportedRecordsTable` 757、`PageSizeSelect` 883、`DetailInspector` 895、`SettingsPanel` 1006、`Field` 1028、`NumberField` 1032、`DateTimeField` 1036、`ConfirmDialog` 1040、`EmptyWorkspace` 1044、`LoadingShell` 1048
  - 8 个辅助函数：`modeLabel` 1052、`frequencyScopeLabel` 1059、`analysisTimeScopeLabel` 1064、`activeExtraFilterCount` 1071、`activeRecordsFilterCount` 1080、`activeDelimitedFieldCount` 1088、`errorMessage` 1095
  - `export default App` 1104
- 现有 `src/components/` 已有 3 个小组件：`Icon.tsx`、`RiskBadge.tsx`、`StatStrip.tsx`——小型展示型组件模式。
- 现无 `src/hooks/` 目录。
- 质量门：`npm run lint` / `npm run test`（23 用例）/ `npm run build` 必须保持全绿。
- spec 约定（frontend/state-management、quality-guidelines、type-safety）：
  - 生产 React 不得拥有评分或全集合过滤；`snapshot` 不持有 people 集合。
  - `filterDraft` / `query` 分离；snapshot/query 变更只请求一页；late response 忽略。
  - DTO 在 `src/domain/types.ts`；`AppApi` 在 `src/api/contract.ts` 是唯一边界。
  - 零 `any` / `as any` / `@ts-ignore`。

## Decisions (locked)

- **范围**：仅子组件外移 + 辅助函数外移，state 全部留在 App。纯机械搬移，零行为变更。
- **子组件去向**：`src/components/`（沿用现有 Icon/RiskBadge/StatStrip 模式）。
- **辅助函数去向**：`src/lib/`（现有 filter.ts/format.ts 同类）。
- **不抽 hook**：state 归属不变，hook 抽取留作后续独立子任务（降低本轮风险）。
- **质量门**：`npm run lint` / `npm run test`（23 用例）/ `npm run build` 全绿。

## Assumptions (temporary)

- 子组件 props 接口保持现有内联形状，仅从内联提到独立文件 + import。
- 类型（BusyAction/ToastState）与初始状态常量留在 App.tsx 或按需外移到 lib。

## Requirements (evolving)

- 将 9 个内联子组件外移到 `src/components/` 独立文件。
- 将 8 个辅助函数外移到 `src/lib/`（与现有 filter.ts/format.ts 同类）。
- 子组件 props 接口形状不变，仅从内联提到独立文件 + import。
- 保持渲染结果与交互行为一致。
- 保持 `npm run lint` / `npm run test` / `npm run build` 全绿。

## Acceptance Criteria (evolving)

- [ ] App.tsx 行数显著下降（目标 < 400 行，主组件壳层 + 状态编排）。
- [ ] 9 个子组件各自独立文件于 `src/components/`。
- [ ] 辅助函数外移到 `src/lib/`。
- [ ] `npm run lint` 零告警。
- [ ] `npm run test` 23 用例全过。
- [ ] `npm run build` 通过。

## Definition of Done

- 上述 AC 全部满足。
- 行为保持不变（现有 23 个测试全过）。

## Out of Scope (explicit)

- 抽取自定义 hook（usePeoplePage 等）——留作后续独立子任务。
- 合并 activeExtraFilterCount/activeRecordsFilterCount（子任务 #15）。
- 任何交互逻辑/状态语义修改。
- 新增测试（本任务纯结构重构，现有测试守卫行为）。

## Technical Notes

- 审计报告 P1 #2 原文建议：子组件拆到 `src/components/`；按域抽 `usePeoplePage`、`useImportedRecordsPage`、`useDisclosure`、`useSnapshotAction`；主 App 仅保留壳层编排。
- 风险点：抽 hook 改变 state 归属，需确保 effect 依赖与闭包捕获不变；React 19 无额外 hook 限制。
- 子代理风险：storage.rs 拆分时 trellis-implement 子代理两次静默失败，本任务可能更复杂（React state），需考虑执行方式。
