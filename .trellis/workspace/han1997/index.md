# Workspace Index - han1997

> Journal tracking for AI development sessions.

---

## Current Status

<!-- @@@auto:current-status -->
- **Active File**: `journal-1.md`
- **Total Sessions**: 37
- **Last Active**: 2026-07-28
<!-- @@@/auto:current-status -->

---

## Active Documents

<!-- @@@auto:active-documents -->
| File | Lines | Status |
|------|-------|--------|
| `journal-1.md` | ~1233 | Active |
<!-- @@@/auto:active-documents -->

---

## Session History

<!-- @@@auto:session-history -->
| # | Date | Title | Commits | Branch |
|---|------|-------|---------|--------|
| 37 | 2026-07-28 | Task: 导入分析进度条与默认最大化启动 | `3abc254` | `main` |
| 36 | 2026-07-28 | Task #4: analysis-overlap-scanline-for-dense-persons | `4606419` | `main` |
| 35 | 2026-07-27 | 抽取前端 validateAnalysisSettings 对齐后端校验语义 | `5006c3a` | `main` |
| 34 | 2026-07-27 | 抽取 bulk_insert_batch 宏统一 execute_*_batch 骨架 | `27c9bab` | `main` |
| 33 | 2026-07-27 | lib.rs 启动 expect 改 stderr + exit(1) | `d8a72cd` | `main` |
| 32 | 2026-07-27 | 抽取 SaveTimer 外置 save 内 cfg(test) 计时块 | `a0da004` | `main` |
| 31 | 2026-07-26 | 抽取 score_and_pick_sheet 合并 importer sheet 择优 | `33c25a6`, `a40a186` | `main` |
| 30 | 2026-07-25 | importer 静态正则 unwrap 改 expect | `b21020b` | `main` |
| 29 | 2026-07-24 | 合并 activeExtraFilterCount 与 activeRecordsFilterCount | `91b4dfb` | `main` |
| 28 | 2026-07-24 | 清理 browserApi toImportedStayRecord 字段裁剪 | `4f3609d` | `main` |
| 27 | 2026-07-24 | 抽取 export_error helper 收敛重复 map_err | `2590844` | `main` |
| 26 | 2026-07-24 | 去重前端 filter 编排 | `0821c40` | `main` |
| 25 | 2026-07-24 | 移除 analysis 生产路径 .expect() | `2964170` | `main` |
| 24 | 2026-07-24 | 拆分 parse_file 为定起始+列映射与逐行构造 | `1c65f0d` | `main` |
| 23 | 2026-07-24 | merge_sessions 复用结构化 DeduplicationKey | `8c60291` | `main` |
| 22 | 2026-07-24 | 统一 storage filter 构建器 | `7305803` | `main` |
| 21 | 2026-07-24 | 拆分 App.tsx 子组件与辅助函数外移 | `b9f9690` | `main` |
| 20 | 2026-07-24 | 拆分 storage.rs 按职责分模块 | `72dcc17` | `main` |
| 19 | 2026-07-24 | 全仓代码体检与优化 | `a71fbc7` | `main` |
| 18 | 2026-07-23 | 优化数据分析性能 | `329023b` | `main` |
| 17 | 2026-07-23 | 完成区域模糊多选筛选 | `2b8d232` | `main` |
| 16 | 2026-07-23 | 优化文件夹批量导入性能 | `d969bea` | `main` |
| 15 | 2026-07-22 | 修复会话删除与 SQLite 清理 | `4d78fe9` | `main` |
| 14 | 2026-07-22 | 优化多文件导入合并性能 | `f12a11b` | `main` |
| 13 | 2026-07-22 | 优化历史筛选查询性能 | `5c5cf5c` | `main` |
| 12 | 2026-07-22 | 修复启动白屏：v2→v3 数据库迁移改为清理重建 | `8eaad35` | `main` |
| 11 | 2026-07-22 | 为导入记录增加结果筛选功能 | `e6360f1` | `main` |
| 10 | 2026-07-22 | 修复筛选弹窗显示并增强人员核查详情对比 | `7748838`, `d4b57d9`, `089affd` | `main` |
| 9 | 2026-07-22 | 简化分析参数并优化结果表交互 | `856fb75` | `main` |
| 8 | 2026-07-22 | 修复导入记录分页与视图切换 | `b8b99d6` | `main` |
| 7 | 2026-07-22 | Optimize import and history performance | `c3b65c7`, `10f32cf` | `main` |
| 6 | 2026-07-22 | Optimize hotel result filters | `47782f6` | `main` |
| 5 | 2026-07-22 | Simplify analysis workspace UI | `3e9b38c` | `main` |
| 4 | 2026-07-22 | Sync upstream scoring rules and analysis UI | `f46e12c` | `main` |
| 3 | 2026-07-21 | Fix legacy XLS import compatibility | `e733117` | `main` |
| 2 | 2026-07-21 | fix folder recursive import + archive task | `81e689e` | `main` |
| 1 | 2026-07-16 | Tauri Rust refactor and UI redesign | `0796023`, `87c7a80` | `main` |
<!-- @@@/auto:session-history -->

---

## Notes

- Sessions are appended to journal files
- New journal file created when current exceeds 2000 lines
- Use `add_session.py` to record sessions