# Tauri 重构并美化 maiyin_analysis

## Goal

将 `https://github.com/han1997/maiyin_analysis.git` 中的 Python/Tkinter 旅馆业入住数据预警分析工具重构为一个本地优先的 Tauri 桌面应用，并把界面升级为更现代、克制、适合长时间表格核查的产品工具。

## What I Already Know

- 当前目录只有 Trellis 元数据和 `AGENTS.md`，没有应用源码。
- 原仓库已浅克隆到 `.trellis/workspace/han1997/source-maiyin_analysis` 用作迁移来源。
- 原项目是 Python/Tkinter 桌面应用，没有 `package.json`、Vite、React 或 Tauri 配置。
- 原项目核心业务模块包括：
  - `desktop_app/analysis_engine.py`: 表头识别、字段推断、导入清洗、去重、过滤、风险分析、搜索过滤。
  - `desktop_app/io_service.py`: `.xls`、`.xlsx`、`.csv` 读取，CSV/Excel/模板导出。
  - `desktop_app/session_store.py`: 本地历史会话持久化与合并。
  - `desktop_app/settings_store.py`: 存放目录设置。
  - `desktop_app/test_desktop.py`: 业务规则和导出行为测试。
- 原产品要求敏感数据留在本机，不启动远程服务，结果可解释、可导出、可复核。
- 本机已有 Node.js `v24.14.0` 和 npm `11.9.0`。
- 本机现已安装 Rust stable：`rustc 1.96.0`、`cargo 1.96.0`，可以执行原生检查、测试和 Tauri release 构建。
- 原应用迁移规模不是单纯换皮：`app.py` 约 1410 行，`analysis_engine.py` 约 831 行，`io_service.py` 约 470 行，现有回归测试约 428 行。
- 高风险兼容点包括乱码/装饰表头推断、Excel 日期与多编码 CSV、模板固定列位、跨历史去重、短入住过滤、重合入住判定、风险合并导出和 CSV 公式注入防护。
- 设计场景明确为白天办公环境中的长时间表格核查，设计 register 为 product；应保留熟悉的桌面信息架构，同时补齐加载、空、错误、局部失败、导出反馈和键盘焦点状态。

## Requirements

- 创建一个 Tauri-ready 项目结构，使用现代前端栈承载界面。
- 保留原工具的核心工作流：
  - 选择单个/多个 `.xls`、`.xlsx`、`.csv` 文件导入。
  - 支持从文件夹递归导入支持格式。
  - 自动识别表头、固定模板列位兜底、核心列推断。
  - 去重、过滤不足 10 分钟入住、按分析参数筛选。
  - 计算同日重合入住、同日多次非重合入住、30 天高频、365 天高频。
  - 风险评分、风险等级、人员列表分页/搜索/筛选。
  - 人员详情展示预警说明和证据明细。
  - 本地历史会话列表、多选合并分析、删除当前数据。
  - 导出模板、人员汇总 CSV、风险合并 Excel、规范化原始 CSV。
- 美化界面但保持产品工具气质：
  - 浅色、低刺激、办公场景友好。
  - 左侧控制区 + 右侧结果工作区。
  - 表格密集但不拥挤，风险等级不能只靠颜色表达。
  - 避免指挥大屏、霓虹深色、夸张渐变、营销页式 hero。
  - 使用紧凑顶栏、可折叠/可调整宽度的左侧控制区、主结果表和右侧详情检查器，尽量减少阻断式弹窗。
  - 使用带色相的浅色中性色、单一低饱和深蓝强调色、表格数字等宽对齐和 1px 分隔线，不使用玻璃拟态、渐变文字或嵌套卡片。
  - 明确设计导入中、空数据、解析失败、部分文件失败、导出成功/失败、禁用和危险确认状态。
- 保持本地数据处理，不接入远程接口。
- 同时支持 `npm run dev` 浏览器演示与 Tauri 原生构建；浏览器演示不得伪装成真实文件解析。
- 生产业务规则只保留一套 Rust 实现；浏览器预览可使用与 Tauri 命令相同 DTO 的模拟适配器，但不得复制风险评分规则作为第二套生产逻辑。
- 将老式 `.xls` 中文文本兼容性作为独立验收闸门；Rust 解析器必须通过代表性样本对照，失败时只为 `.xls` 引入窄范围兼容适配。

## Acceptance Criteria

- [ ] 根目录包含可安装的 Node/Tauri 项目配置。
- [ ] `src-tauri` 配置存在，后续安装 Rust 后可继续运行 Tauri。
- [ ] 前端能导入示例/用户选择的数据并生成风险汇总。
- [ ] Tauri 模式通过粗粒度 Rust 命令完成真实文件导入、重分析、历史管理和导出；Vite 浏览器模式通过类型一致的 fixture/mock 展示完整工作流。
- [ ] 业务逻辑覆盖原 Python 测试中的关键规则：模板列位、字段推断、身份证户籍地、去重、短入住过滤、风险评分、搜索过滤、导出安全。
- [ ] `.xls` 兼容测试覆盖已知中文文本风险；未通过时不得宣称与原应用导入能力等价。
- [ ] UI 包含导入区、参数区、统计汇总、人员结果表、详情视图、历史列表、导出动作。
- [ ] `npm run lint` 或等价检查通过。
- [ ] 在当前环境中说明无法运行 Tauri 的具体原因和安装 Rust 后的下一步命令。

## Definition of Done

- Tests added/updated where appropriate.
- Lint/type-check/build verification attempted and results recorded.
- Documentation notes updated if behavior or setup differs from original Python app.
- Rollback path is clear: original source remains available under `.trellis/workspace/han1997/source-maiyin_analysis`.

## Technical Approach

Recommended MVP approach: Tauri + React/TypeScript UI with an authoritative Rust backend.

- Use React/TypeScript for the interface and state orchestration.
- Use a restrained product UI with system fonts, stable spacing, clear focus states, and accessible risk badges.
- Use coarse Tauri commands for import, reanalysis, session operations, detail retrieval, and export; avoid transferring complete workbooks row by row over IPC.
- Port parsing, normalization, risk analysis, persistence, and export into testable Rust modules isolated from Tauri command wiring.
- Use Calamine/rust_xlsxwriter/csv/encoding crates behind adapters, with an explicit compatibility fallback boundary for problematic legacy `.xls` files.
- Add `src-tauri` in an official Tauri v2 shape, but allow fixture-backed Vite preview until Rust is installed.
- Keep TypeScript domain DTOs aligned with Rust serialization contracts and keep presentation logic isolated from React components.
- Preserve the current information architecture where it supports experienced operators, but replace modal-heavy detail navigation with progressive panels where practical.
- Keep the browser preview and Tauri runtime behind adapters so sample/browser file input does not leak into desktop filesystem implementation.

## Decision (ADR-lite)

Context: The original app is Python/Tkinter with deterministic local analysis rules and sensitive data. Tauri requires Rust tooling for desktop build, but this machine currently only has Node/npm. A TypeScript-only port would be quick to preview but would place desktop file, persistence, export, and rule responsibilities in the webview layer.

Decision: Use React/TypeScript for the product UI and Rust as the authoritative backend for file parsing, normalization, analysis, session persistence, and export. Keep a fixture/mock adapter for Vite preview. Do not retain the whole Python application as a sidecar. Treat legacy `.xls` text compatibility as a test gate and add a narrow format-specific fallback only if Rust parsing fails representative fixtures.

Consequences: Native verification waits for Rust installation and the initial port is larger, but production has one business-rule implementation, a cleaner local-data boundary, and no general Python runtime. Browser preview cannot perform all native operations. Legacy `.xls` equivalence remains conditional on fixture-backed tests.

## Out of Scope

- Remote sync, cloud storage, login/auth, multi-user collaboration.
- A command-center dashboard or map visualization.
- Automatic installation of Rust, Visual Studio Build Tools, or system-wide dependencies.
- Perfect one-to-one visual recreation of the Tkinter UI.
- Rebuilding the old PyInstaller Windows 7 EXE pipeline.

## Research References

- [`research/tauri-migration.md`](research/tauri-migration.md): Tauri setup constraints, architecture options, and recommended MVP path.
- [`research/ui-redesign-audit.md`](research/ui-redesign-audit.md): Original workflow audit, product design direction, interaction states, and responsive implications.
- [`research/rust-backend-decision.md`](research/rust-backend-decision.md): Rust/React responsibility boundary, IPC shape, ecosystem check, and legacy `.xls` compatibility gate.

## Technical Notes

- Relevant original source path: `.trellis/workspace/han1997/source-maiyin_analysis`.
- Relevant product/design source files: original `PRODUCT.md` and `DESIGN.md`.
- UI register: product tool, not brand/marketing surface.
- Rust toolchain confirmed: `rustc 1.96.0`, `cargo 1.96.0`, stable MSVC target.
- Implementation status: React/Vite preview, shared DTO contract, Rust backend modules, Tauri v2 config, local resources and README are present in the project root.
- Frontend verification completed on 2026-07-16: `npm run lint`, `npm run test` (3 tests), and `npm run build` all pass; a 1440×900 preview screenshot was inspected for layout quality.
- Native verification completed: `cargo check --all-targets`, 2 Rust tests, Clippy with `-D warnings`, and `tauri build --no-bundle` pass.
- Release EXE generated at `src-tauri/target/release/maiyin-analysis.exe` (14.7 MB). MSI/NSIS bundling reached the packaging stage but WiX/NSIS downloads from GitHub timed out.

## Open Questions

- None. The user selected the complete migration and delegated the Rust backend judgment; the chosen architecture is React/TypeScript UI plus authoritative Rust backend.
