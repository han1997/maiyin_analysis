# 修复筛选弹窗显示并增强人员核查详情对比

## Goal

修复"更多筛选"弹窗在桌面窗口下右侧内容被裁切、横向滑动后自动回弹的问题；同时让人员核查详情支持最大化放大，使预警说明与住宿证据更易浏览与对比。

## What I already know

- `.filter-popover`（`src/styles.css:400`）：`left: 0`、`width: min(620px, calc(100vw - 32px))`、`max-height` 视口约束、`overflow-y: auto`、`overscroll-behavior: contain`，`overflow-x` 默认 `visible`。宽度公式假定起点在视口左缘 x=0，但 `left: 0` 实际相对 `.toolbar-menu`（工具栏左侧偏右位置），弹窗右缘常超出视口 → 页面横向滚动；用户横向滑动能看到右侧内容但被回弹。这是上一任务放开 `.results-region { overflow: visible }` 后未同步约束宽度的遗留。
- `DetailInspector`（`src/App.tsx:749`）是右侧固定面板：`position: fixed; right: 0; width: var(--detail-width)`，字号 9–11px，单列纵向滚动：人员信息 → 预警说明(alert-list) → 住宿证据(evidence-list)。
- 后端 `AlertSummary.evidence_ids: Vec<u64>`（`src-tauri/src/model.rs:144`，`#[serde(default)]`）已由 `analysis.rs` 填充，`EvidenceRecord.uid: u64`（`model.rs:184`）与之对应；`storage.rs:482` 已用 `evidence_ids` 做证据筛选。
- **契约缺口**：TS `AlertSummary`（`src/domain/types.ts:22`）只声明 `evidenceCount`，未声明 `evidenceIds`，后端实际已下发 `evidenceIds`。需补该字段以联动。

## Requirements

- 修复"更多筛选"弹窗右侧内容被裁切/横向回弹：弹窗宽度与定位保证在常见桌面分辨率下右缘不超出视口，不再产生页面横向滚动与回弹。
- 人员核查详情支持最大化：新增一个按钮把详情展开为占满主区的宽视图，再点收起回到常规侧栏；最大化时仍可关闭、可 Escape 收起。
- 预警 ↔ 关联证据联动：点击某条预警时，住宿证据列表仅显示触发该预警的记录（按 `evidenceIds` ↔ `evidence.uid` 匹配）；提供"全部"按钮恢复全部证据；默认（未点预警）显示全部。
- 住宿证据并排查看：最大化视图中证据以网格/卡片并排排列，便于看出时间与旅馆重叠；常规侧栏视图保持现行单列以兼容窄空间。
- 补齐 TS `AlertSummary.evidenceIds: number[]` 与后端契约一致。
- 视觉延续浅色、克制的办公工具风格，选中/禁用/hover/focus 状态清晰。

## Acceptance Criteria

- [ ] "更多筛选"弹窗在常见桌面分辨率下全部内容可见，不再出现横向滚动与回弹。
- [ ] 详情有最大化按钮；最大化展开为占满主区的宽视图，再次点击或 Escape 收起回常规侧栏。
- [ ] 点击某条预警后住宿证据仅显示其关联记录；点"全部"恢复全部证据；未选预警时显示全部。
- [ ] 最大化视图中证据并排排列，可直观对比时间/旅馆重叠。
- [ ] TS `AlertSummary.evidenceIds` 与后端 `evidence_ids` 一致，build 通过。
- [ ] 前端交互测试、lint、build、fmt、clippy、cargo test 全部通过。
- [ ] 不改变重合入住、同日多次入住的评分公式与导出格式。

## Definition of Done

- 弹窗显示回归测试通过。
- 详情最大化、预警联动、证据并排的回归测试通过。
- 更新相关 Trellis 前端交互规范与跨层契约（TS `AlertSummary.evidenceIds` 同步）。
- lint / build / fmt / clippy / test 全绿。

## Technical Approach

- 弹窗：改为按视口右缘约束宽度（如 `max-width: calc(100vw - <左偏移> - 16px)` 或改用右锚定/`right` 约束），确保右缘不超视口；保留现有纵向内部滚动与 `overscroll-behavior`。
- 最大化：在 `DetailInspector` 增加 `maximized` 受控状态（由 `App` 持有）；最大化时面板用固定/绝对铺满主区（top 从 topbar 下沿到底部、左右铺开或大幅加宽），内容区放大字号/并排网格；按钮在 `detail-header` 内，`aria-expanded` 反映状态。
- 预警联动：`App` 持有 `selectedAlertKind`；预警项可点击，点击后证据列表按 `alert.evidenceIds.includes(record.uid)` 过滤；"全部"按钮清空选择。联动仅在前端，不调 Rust。
- 证据并排：最大化视图下 `evidence-list` 改为 `grid-template-columns: repeat(auto-fill, minmax(...))` 卡片并排；常规侧栏保持单列。
- 类型同步：`src/domain/types.ts` 的 `AlertSummary` 增加 `evidenceIds: number[]`（后端已 `#[serde(default)]`，旧数据安全）。

## Out of Scope

- 不改变重合入住、同日多次入住的评分公式。
- 不改变导出文件格式与更多筛选业务含义。
- 不重新设计整张人员表或新增列拖拽缩放。
- 不做跨人员详情对比。
- 不新增后端分析逻辑（`evidence_ids` 已存在，仅补 TS 类型与前端联动）。

## Technical Notes

- 主要文件：`src/App.tsx`（`DetailInspector`、筛选弹窗、联动状态）、`src/styles.css`（`.filter-popover`、`.detail-inspector` 及最大化样式、`.evidence-list` 网格）、`src/domain/types.ts`（`AlertSummary.evidenceIds`）。
- 跨层契约：`PersonDetail.alerts[].evidenceIds` ↔ `evidence[].uid`，见 `.trellis/spec/backend/tauri-contract.md` 与 `src-tauri/src/model.rs`。
- 业务场景：公安办公人员白天桌面环境长时间扫描密集表格，优先清晰、稳定、低干扰。
