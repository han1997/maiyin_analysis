# 导出性能优化与进度反馈与中文化

## Goal

导出当前三个问题：(1) 太慢（主因：全量加载 `StoredSession` 含 records+analyses，且 `risk_xlsx` 全在内存构建）；(2) 无进度也无完成提示（export 是上一任务的明确 Out of Scope，只有不定 `busy-line`，无百分比、无相位）；(3) 格式与参考仓库 `maiyin_analysis`（Python 原版）不一致，需要完全对齐——尤其是 `risk_xlsx` 的合并行结构、预警类型中文化、分级配色、freeze/autofilter。

## What I already know

### 当前导出实现（`src-tauri/src/exporter.rs`，225 行）

* `export_result` 命令（`commands.rs:381-395`）`spawn_blocking` 跑 `export_to(kind, session, path)`，返回 `OperationResult { message: "已导出到 {path}" }`。
* 4 种导出：`summary_csv`（15 列）、`raw_csv`（16 列）、`risk_xlsx`（13 列，1 行/预警）、`template_xlsx`（静态文件）。
* CSV：`csv` crate + UTF-8 BOM + `safe()` 公式注入防护（前缀 `'`）。流式写入。
* XLSX：`rust_xlsxwriter` 0.96，全内存构建 + `autofit()` 全扫。**不流式**。
* 表头**已是全中文**——但与参考仓库在结构/列数/格式上有显著差距（见下表）。
* 无进度事件、无 benchmark。

### 性能瓶颈（按影响排序）

1. **全量加载 `StoredSession`**（`commands.rs:389` `store.load(&session_id)`）：`StoredSession` 同时持有 `records: Vec<Record>`（可达 45 万+）和 `analyses: Vec<PersonAnalysis>`。但 `summary_csv`/`risk_xlsx` 只需 `analyses`，`raw_csv` 只需 `records`——当前总是全加载。
2. **`risk_xlsx` 全内存构建**：`Workbook` 累积所有单元格字符串 + zip 缓冲，`autofit()` 再全扫一遍。
3. **`safe()` 每字段 `to_string()` 克隆**：`raw_csv` 45 万行 × ~15 字段 = 百万级小分配。
4. **数值写为字符串**：`score`/`age`/`evidence_count` 先 `format!` 成 `String` 再 `write_string`，XLSX 里变文本，XML 体积更大且无数值格式。

### 当前 vs 参考仓库格式差距

| 方面 | newUI (`exporter.rs`) | 参考 (`io_service.py`) |
|---|---|---|
| summary_csv 列数 | 15（单列 `户籍地`） | 18（拆 `户籍省/市/县区` + `时间窗口内入住次数`） |
| raw_csv 列数 | 16（单 `户籍地`，无年龄性别） | 23（拆 `户籍省/市/县区/区划/详址` + `地域省市县` + `年龄/性别`） |
| risk sheet 名 | `风险人员` | `风险合并明细` |
| risk 列数 | 13（扁平 1 行/预警） | 26（合并人块 + N 证据行，垂直合并前 14 列） |
| risk 预警类型 | 原始 `alert.kind` | `ALERT_KIND_LABELS` 映射中文 + `\n` 连接 |
| risk 格式 | 仅粗体表头 | 分级配色（高/中/关注/正常）+ freeze_panes + autofilter + 固定列宽 |
| risk 进度 | 无 | `progress_callback` 每 100 人触发 |
| BOM | 显式 `0xEF 0xBB 0xBF` | `utf-8-sig`（等价） |
| 公式注入防护 | CSV 有 `safe()`；XLSX 无 | CSV `_safe_csv`；XLSX `strings_to_formulas: False` |

### 参考仓库位置

`C:\Users\hanhu\Code\maiyin_analysis\desktop_app\io_service.py`（477 行，Python + `xlsxwriter`）。

### 前端导出 UX（`src/App.tsx:325-336`）

* `exportResult(kind)`：`setBusy("export")` → `await appApi.exportResult(kind)` → toast（success/info/error）。无 `onProgress`（export 在上一任务 Out of Scope）。
* 顶部 `busy-line`（不定 2px 扫动条，`aria-hidden`，无文字）。
* 导出入口：顶栏"下载导入模板"按钮（仅 template）+ 工具栏"导出"下拉（summary_csv/risk_xlsx/raw_csv 三项）。

### 上一任务已建立的进度模式

`commands.rs:22-67` 的 `ProgressPayload { phase, current, total, label }` + `make_progress_callback`（50ms 节流）+ 域层 `Option<&dyn Fn(usize, usize) + Send + Sync>` 回调。前端 `Channel` + `progress` 伴生 state。**export 可直接复用此模式**。

## Assumptions (temporary)

* "完全中文化"指对齐参考仓库的中文表头/预警类型标签/sheet 名/格式——当前表头已是中文，但结构与参考不一致。
* 导出进度按行计数（summary/risk 按 person，raw 按 record），复用 `ProgressPayload` + `Channel`。
* "完成提示"指导出成功后的 toast——当前已有，但可能需要更明确（如含路径、行数）。

## Open Questions

* None — 全部收敛。

## Requirements

### 格式对齐（三种导出全部重写，对齐参考仓库 `io_service.py`）

* **`summary_csv`**（18 列）：姓名, 身份证号, 手机号, **户籍省, 户籍市, 户籍县区**, 年龄, 性别, 记录总数, **时间窗口内入住次数**, 7天最大次数, 30天最大次数, 365天最大次数, 重合天数, 非重合超3天数, 风险分, 风险等级, 预警摘要。
  * 拆户籍省/市/县区（当前单列 `户籍地`）。
  * 加 `时间窗口内入住次数` 列（`frequency_window_count` 字段，待确认 model 是否已有）。
* **`raw_csv`**（23 列）：源文件, 源表行号, 姓名, 身份证号, 手机号, **户籍省, 户籍市, 户籍县区, 户籍地区划, 户籍地详址**, **年龄, 性别**, 酒店名称, 省, 市, 县区, **地域省市县**, 地址, 房间号, 入住时间, 登记时间, 退房时间, 数据问题。
  * 拆户籍省/市/县区/区划/详址，加 `地域省市县`，加 `年龄/性别`（当前缺失）。
  * 用 `format_datetime` 格式化 `NaiveDateTime`（`%Y-%m-%d %H:%M`）。
* **`risk_xlsx`**（26 列，sheet 名 `风险合并明细`）：
  * 表头：姓名, 身份证号, 手机号, 户籍省, 户籍市, 户籍县区, 年龄, 性别, 风险等级, 风险分, 预警类型, 预警级别, 风险标题, 风险说明, 源文件, 源表行号, 酒店名称, 酒店地址, 房间号, 省, 市, 县区, 地域省市县, 入住时间, 退房时间, 登记时间。
  * **合并行结构**：每人的前 14 列（姓名~风险说明）垂直合并，后续 12 列（源文件~登记时间）每条证据一行。
  * **预警类型中文化**：`ALERT_KIND_LABELS` 映射（overlap→入住时间重叠, same_day_many→同日多次入住, window_frequency→时间窗口高频入住, week/month/year_frequency→7/30/365天高频入住），多条用 `\n` 连接，去重保序。
  * **分级配色**：高风险 `#FDE8E7`/`#8C2929`、中风险 `#FFF2D8`/`#7B551B`、关注 `#FFF8D8`/`#635A24`、正常 `#E8F4EC`/`#315A40`。风险等级列用对应配色。
  * 表头：粗体白字 `#17324D` 底色，居中，边框 `#D4DCE5`，行高 28。
  * `freeze_panes(1, 0)` + `autofilter`。
  * 证据按 `check_in` 排序。
* **`template_xlsx`**：不变（静态文件复制）。

### 性能优化

* **拆分加载**：
  * `summary_csv`：新增 `SessionStore::load_analyses` 方法，只加载 analyses 不加载 records（避免 45 万 records 反序列化）。
  * `raw_csv`：用现有 `load_records`（只加载 records 不加载 analyses）。
  * `risk_xlsx`：仍需全 `load`（analyses 查预警 + records 查证据），但优化写入。
* **`safe()` 优化**：返回 `Cow<str>` 而非 `to_string()`，避免无注入风险的字段克隆。
* **XLSX 数值**：`score`/`age`/`evidence_count` 用 `write_number` 而非 `write_string`（减小 XML、启用数值格式）。`rust_xlsxwriter` 合并行用 `merge_range`。

### 进度反馈

* 复用上一任务的 `ProgressPayload` + `tauri::ipc::Channel` + `make_progress_callback`（50ms 节流）。
* `export_result` 命令加 `on_progress: Channel<ProgressPayload>` 参数。
* 域层 `exporter.rs` 的 3 个导出函数加 `Option<&dyn Fn(usize, usize) + Send + Sync>` 回调：
  * `export_summary_csv`：按 person 计数 `(current, total=analyses.len())`。
  * `export_raw_csv`：按 record 计数（仅窗口内 records，total 预先过滤计数）。
  * `export_risk_xlsx`：按 person 计数。
* 相位：`exporting`（有百分比）+ `writing`（不定，仅 XLSX save 阶段）。
* 前端 `appApi.exportResult` 加可选 `onProgress`，`Channel` 接线（同 import/reanalyze 模式）。
* 前端 `App.tsx` `exportResult` 调用传 `setProgress`，`finally` 清除。
* 导出期间顶部渲染确定性进度条 + 相位文字（复用现有 `progress-line`）。

### 完成提示

* 导出成功 toast 含路径（当前已有 `"已导出到 {path}"`，保持）。
* 导出取消保持 `"已取消导出。"` toast。
* 导出失败保持 error toast。

### 不变项

* `export_result` 返回类型 `Result<OperationResult, CommandError>` 不变。
* `OperationResult { message: String, path: Option<String> }` 结构不变。
* `StoredSession` / `PersonSummary` / `AlertSummary` / `Record` 结构不变（除非新增字段）。
* 导出入口（顶栏模板按钮 + 工具栏导出下拉）不变。
* delete / session load / clear 的进度不动。

## Acceptance Criteria (evolving)

* [ ] 导出过程中用户看到确定性进度条 + 相位文字。
* [ ] 导出完成后用户看到明确成功提示（含路径）。
* [ ] 导出文件格式与参考仓库一致（具体列、中文标签、sheet 名、格式）。
* [ ] 导出耗时显著降低（对比 before/after）。
* [ ] `npm run lint`、`npm run test`、`npm run build` 全绿。
* [ ] `cargo fmt --check`、`cargo test`、`cargo clippy --all-targets -- -D warnings` 全绿。

## Definition of Done

* 前后端质量门全绿。
* 导出格式与参考仓库人工对比一致。
* 不破坏现有 `WorkspaceSnapshot` / `OperationResult` 契约。

## Out of Scope (explicit)

* `template_xlsx` 格式调整（保持静态文件复制）。
* 导出取消按钮（进度期间不提供取消）。
* 导出多格式批量打包。
* 改变 `OperationResult` / `StoredSession` 结构。
* delete / session load / clear 的进度升级。
* 引入新第三方导出库（继续用 `csv` + `rust_xlsxwriter`）。

## Technical Approach

### 后端（Rust）

#### 1. `exporter.rs` 重写

* 新增 `ALERT_KIND_LABELS: &[(&str, &str)]` 常量（6 条映射）。
* `safe()` 改返回 `Cow<'_, str>`：无注入风险返回 `Cow::Borrowed`，有风险返回 `Cow::Owned(format!("'{value}"))`。
* `export_summary_csv(path, analyses, on_progress)`：
  * 18 列表头。
  * 按 person 迭代，每行 `write_record`，回调 `(current, total)`。
* `export_raw_csv(path, records, settings, on_progress)`：
  * 23 列表头。
  * 先过滤窗口内 records 计 total，再迭代写入，回调按 record 计数。
  * `format_datetime` 用 chrono `format("%Y-%m-%d %H:%M")`。
* `export_risk_xlsx(path, analyses, records, on_progress)`：
  * 26 列表头，sheet 名 `风险合并明细`。
  * 构建 `record_map: HashMap<u64, &Record>`。
  * 每人：收集 evidence_ids（按 check_in 排序），构造 person_values（前 14 列）+ 每条证据 detail_values（后 12 列）。
  * `rust_xlsxwriter` 的 `merge_range` 实现垂直合并；单行证据时直接写。
  * 分级配色：`Format` 对象按 level（高风险/中风险/关注/正常）。
  * `freeze_panes(1, 0)` + `autofilter`。
  * 数值列用 `write_number`。
  * 回调按 person 计数。
* `export_template` 不变。

#### 2. `commands.rs` — `export_result` 加 Channel

* 加 `on_progress: tauri::ipc::Channel<ProgressPayload>` 参数（复用现有 `ProgressPayload` 结构）。
* `spawn_blocking` 内：按 kind 调用对应的 `load` 方法 + `export_*` 函数，传 `make_progress_callback` 生成的闭包。
* 拆分加载：
  * `summary_csv` → `store.load_analyses(&session_id)?`（新增方法）。
  * `raw_csv` → `store.load_records(&session_id)?`（已存在）。
  * `risk_xlsx` → `store.load(&session_id)?`（全量）。

#### 3. `storage.rs` — 新增 `load_analyses`

* 新增 `pub fn load_analyses(&self, session_id: &str) -> Result<Vec<PersonAnalysis>, AppError>`。
* 复用 `load` 内部的 summaries + alerts 查询逻辑，不加载 records。
* 返回 `Vec<PersonAnalysis>`（不含 records，summary_csv 只需此）。

### 前端（React + TS）

#### 4. `src/api/contract.ts` + `tauriApi.ts`

* `exportResult` 加可选 `onProgress?: (p: Progress) => void`。
* `tauriApi.exportResult` 用 `new Channel<Progress>()` 接线（同 import/reanalyze 模式）。
* 浏览器 adapter 忽略 `onProgress`。

#### 5. `src/App.tsx`

* `exportResult(kind)` 传 `setProgress` 作为 `onProgress`。
* `finally` 清除 `progress`（与 `busy` 一起）。
* 导出期间顶部渲染确定性进度条 + 相位文字（复用现有 `progress-line`）。

## Decision (ADR-lite)

**Context**: 导出三个问题：慢（全量加载 + XLSX 全内存 + safe 克隆）、无进度（export 是上一任务 Out of Scope）、格式与参考仓库不一致（列结构、合并行、配色、预警类型中文）。

**Decision**: 三种导出全部重写对齐参考仓库；性能上拆分加载（summary 只取 analyses，raw 只取 records）+ safe 返回 Cow + 数值列用 write_number；进度复用上一任务 ProgressPayload/Channel 模式，按行计数百分比；前端 exportResult 加 onProgress + progress 伴生 state。

**Consequences**:
- `exporter.rs` 几乎重写（但仍是单文件，无新依赖）。
- `storage.rs` 加 `load_analyses` 方法（复用 `load` 内部逻辑）。
- `export_result` 命令签名加 `on_progress`（向后兼容，前端旧调用不传会被 Tauri 反序列化默认值处理——但实际前端会同步更新）。
- `safe()` 返回 `Cow` 减少无注入字段的克隆。
- `risk_xlsx` 合并行结构匹配参考仓库，用户可直接对比两版输出。

## Technical Notes

* 当前导出文件：`src-tauri/src/exporter.rs`（225 行）。
* 参考仓库导出：`C:\Users\hanhu\Code\maiyin_analysis\desktop_app\io_service.py`（477 行）。
* 进度模式已建立：`commands.rs:22-67` `ProgressPayload` + `make_progress_callback`；前端 `App.tsx:93` `progress` state + `App.tsx:761-774` 渲染。
* `StoredSession` 结构：`model.rs:371-384`，含 `records` + `analyses`。
* 存储层是否有"只加载 analyses"或"只加载 records"的方法——待探查 `storage.rs`。
* `ALERT_KIND_LABELS` 参考映射：`overlap`→入住时间重叠, `same_day_many`→同日多次入住, `window_frequency`→时间窗口高频入住, `week/month/year_frequency`→7/30/365天高频入住。
* 质量命令：`cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo test --manifest-path src-tauri/Cargo.toml`、`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`、`npm run lint`、`npm run test`、`npm run build`。
