# 导入分析进度条与默认全屏启动

## Goal

为导入和分析两类长耗时操作增加进度条 + 进度提示，让用户看到"做到哪一步、还要等多久"；同时让软件启动时默认占满屏幕，避免用户每次手动最大化。

## What I already know

### 进度现状（完全缺失）

* **后端无任何进度事件**：`src-tauri/src/commands.rs` 中 `import_paths`(`:51-95`)、`import_folders`(`:97-112`)、`reanalyze`(`:218-241`) 全是请求-响应模型，`spawn_blocking` 跑完后一次性返回 `WorkspaceSnapshot`。无 `tauri::ipc::Channel`、无 `AppHandle::emit`、无任何事件监听。
* **前端只有不定进度动画**：`App.tsx:40` 的 `busy` 状态枚举（`boot|import|reanalyze|session|export|delete|null`）是唯一加载信号源。`runSnapshotAction`(`App.tsx:235-262`) 是所有长操作的统一入口。
* **现有视觉**：
  * `.inline-progress`（`styles.css:223-233`）—— 46% 宽不定进度条，用于导入/删除。
  * `.busy-line`（`styles.css:655-656`）—— 顶部 2px 全宽扫动条，用于 reanalyze/session/export。
  * 都是纯 CSS 动画，无 JS 驱动的百分比。
* **无 toast 进度态**：toast（`App.tsx:92`，`styles.css:631-654`）只有 `info/success/error` 三种终态，单条不堆叠。

### 导入/分析内部阶段（可报告进度的点）

* **import_paths**（`commands.rs:67` spawn_blocking 内）：
  1. `importer::import_paths(&paths)` —— Rayon 并行解析文件（`importer.rs:126-131` `par_iter`）+ 合并去重。
  2. `analyze_records(&imported.records, &settings)` —— 分组 + Rayon 并行 `analyze_person`（`analysis.rs:89-92`）。
  3. `store.save(&session)` —— SQLite 写入。
* **reanalyze**（`commands.rs:225` spawn_blocking 内）：
  1. `store.load_records(&session_id)` —— SQLite 读取。
  2. `analyze_records(&records, &settings)` —— 同上并行分析。
  3. `store.replace_analysis(...)` —— SQLite 写入。
* **可计数点**：文件数（导入阶段 1）、人员数（分析阶段 2，用 `AtomicUsize` 在 `analyze_person` 内递增并节流 emit）。

### 窗口启动现状

* `src-tauri/tauri.conf.json` 窗口配置：`width:1440 height:900 center:true resizable:true fullscreen:false`，**无 `maximized` 字段**（Tauri 默认 false）。
* Rust 侧 `lib.rs:13-43` setup hook 只做 `AppState::open`，不碰窗口。无 `set_maximized`/`set_fullscreen`/`WindowBuilder`。
* 前端不引用 `@tauri-apps/api/window`，不操作窗口。
* `capabilities/default.json` 权限只有 `core:default`、`dialog:allow-open`、`dialog:allow-save`，无窗口操作权限。但**声明式配置（tauri.conf.json 加 `maximized:true`）不需要额外权限**。

### 前端结构（可扩展点）

* 单一 `App.tsx`（760 行），无第三方 UI 库（package.json 只有 react/tauri-api）。
* 组件目录 `src/components/` 全是手写领域组件，无通用 `ProgressBar`/`Modal`/`Banner`。
* 现有 overlay 模式：`panel-backdrop`（`styles.css:571-579`，`position:fixed inset:0 z-index:50`），被 `ConfirmDialog`、`SettingsPanel` 复用。
* 进度 UI 自然接入点：`App.tsx:754` 的 `{busy && ...}` 全局条附近 + `App.tsx:427-432` 的导入 `inline-progress` 块。
* 样式：单一 `styles.css`（719 行）vanilla CSS + `oklch()` + CSS 变量。`styles.css:718` 有 reduced-motion 覆盖（动画降到 ~0.01ms）。

## Assumptions (temporary)

* "全屏"指最大化（保留标题栏、可还原），而非真·全屏（覆盖任务栏、无装饰）。待确认。
* 进度需要 Rust→前端事件通道（Tauri 2 `ipc::Channel<T>` 是惯用法）。
* 进度 UI 用内联 banner/条而非模态遮罩（符合现有 `inline-progress`/`busy-line` 的内联风格，不打断用户看其他面板）。

## Open Questions

* None — 全部收敛。

## Requirements

* 导入（`import_paths` / `import_folders`）过程中显示进度条 + 进度提示：
  * 相位 1「解析文件」：按文件数报告百分比（X/Y）。
  * 相位 2「分析计算」：按人员数报告百分比（X/Y）。
  * 相位 3「保存会话」：仅相位标签（`total=0`，不定）。
* 重分析（`reanalyze`）过程中显示进度条 + 进度提示：
  * 相位 1「分析计算」：按人员数报告百分比（X/Y）。
  * 相位 2「保存分析」：仅相位标签（`total=0`，不定）。
* 合并会话（`merge_sessions`）分析阶段也报告人员数百分比。
* 软件启动默认最大化（保留标题栏，`tauri.conf.json` 加 `"maximized": true`）。
* delete / export / session load / clear 保持现有不定进度（不在本任务升级）。
* 不改变 `WorkspaceSnapshot` / `analyze_records` 返回类型 / 命令的返回值契约。
* 前后端质量门全绿。

## Acceptance Criteria

* [ ] 导入过程中用户看到确定性进度条 + 相位文字（解析文件 X/Y → 分析计算 X/Y → 保存会话）。
* [ ] 重分析过程中用户看到确定性进度条 + 相位文字（分析计算 X/Y → 保存分析）。
* [ ] 合并会话过程中用户看到分析阶段百分比。
* [ ] 软件启动后窗口最大化（占满工作区，保留标题栏）。
* [ ] `npm run lint`、`npm run test`、`npm run build` 全绿。
* [ ] `cargo fmt --check`、`cargo test`、`cargo clippy --all-targets -- -D warnings` 全绿。
* [ ] 不破坏现有 `WorkspaceSnapshot` 契约与现有测试。

## Definition of Done

* 前后端质量门全绿。
* 进度在真实数据集上有可见反馈。
* 不破坏现有 `WorkspaceSnapshot` 契约。

## Out of Scope (explicit)

* delete / export / session load / clear 的进度升级（保持现有不定进度）。
* 真·全屏（`fullscreen: true`）。
* 进度条的取消按钮（进度期间不提供取消操作）。
* 改变 `analyze_records` / `analyze_person` 返回类型或 `WorkspaceSnapshot` 结构。
* 引入第三方 UI 库或进度条组件。

## Technical Approach

### 后端（Rust）

#### 1. 进度 payload

`src-tauri/src/commands.rs` 新增可序列化结构：

```rust
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressPayload {
    phase: String,   // "parsing" | "analyzing" | "saving"
    current: usize,
    total: usize,    // 0 = 该相位不定
    label: String,   // 中文展示文本，如 "正在分析 1200/3500"
}
```

#### 2. Tauri 2 Channel 注入

`import_paths` / `import_folders` / `reanalyze` / `merge_sessions` 新增参数 `on_progress: tauri::ipc::Channel<ProgressPayload>`。Tauri 2 自动从前端 `Channel` 对象反序列化此参数。命令在各相位边界 emit：

- 相位开始：`on_progress.emit(ProgressPayload { phase, current: 0, total, label })`。
- 计数进行中：节流 emit（见下）。
- 相位结束：`on_progress.emit(ProgressPayload { phase, current: total, total, label })`。

#### 3. 域层回调注入（保持 Tauri 无关）

`analysis.rs` 的 `analyze_records` 与 `importer.rs` 的 `import_paths` 新增可选回调参数：

```rust
// analysis.rs
pub fn analyze_records(
    records: &[Record],
    settings: &AnalysisSettings,
    on_progress: Option<&dyn Fn(usize, usize) + Send + Sync>, // (current, total)
) -> (Vec<PersonAnalysis>, AnalysisStats)
```

```rust
// importer.rs
pub fn import_paths(
    paths: &[String],
    on_progress: Option<&dyn Fn(usize, usize) + Send + Sync>, // (current, total)
) -> Result<ImportedData, AppError>
```

- `analyze_records`：分组后 `total = grouped.len()`；Rayon `into_par_iter().map` 内 `AtomicUsize::fetch_add` 递增并调用 `on_progress(current, total)`。
- `importer::import_paths`：`files.par_iter().map(parse_file)` 外层用 `AtomicUsize` 计数并调用回调（`parse_file` 本身不改）。
- 现有无进度调用方（测试等）传 `None`；回调为 `None` 时零开销（`if let Some(f) = on_progress { f(...) }`）。

#### 4. 节流 emit（commands.rs）

`commands.rs` 构建闭包，用 `Arc<AtomicU64>`（纳秒时间戳）节流，每 50ms 或末尾 emit 一次：

```rust
fn make_throttled_progress(
    channel: &tauri::ipc::Channel<ProgressPayload>,
    phase: &str,
    total: usize,
    template: &str,
) -> Arc<dyn Fn(usize, usize) + Send + Sync> { ... }
```

闭包内：读 `AtomicU64` 上次 emit 时间，差 >50ms 或 `current == total` 时 `channel.emit(...)` 并更新时间戳。

#### 5. 命令调用链

- `import_paths`：emit「解析文件 0/N」→ `importer::import_paths(&paths, Some(&parse_cb))` → emit「分析计算 0/M」→ `analyze_records(&records, &settings, Some(&analyze_cb))` → emit「保存会话」(total=0) → `store.save`。
- `import_folders`：先 `discover_supported_files`（快，emit「扫描文件」total=0），再委托 `import_paths`（传同一 Channel）。
- `reanalyze`：emit「分析计算 0/M」→ `analyze_records(...)` → emit「保存分析」(total=0) → `store.replace_analysis`。
- `merge_sessions`：合并记录后 emit「分析计算 0/M」→ `analyze_records(...)` → emit「保存会话」(total=0)。

#### 6. 窗口最大化

`src-tauri/tauri.conf.json` 窗口配置加 `"maximized": true`。无需 Rust 代码、无需改权限。

### 前端（React + TypeScript）

#### 1. 类型

`src/domain/types.ts` 新增：

```ts
export interface Progress {
  phase: string;
  current: number;
  total: number;
  label: string;
}
```

#### 2. API 契约

`src/api/contract.ts` 给 `importFiles` / `importFolder` / `reanalyze` / `mergeSessions` 加可选 `onProgress?: (p: Progress) => void`。

`src/api/tauriApi.ts`：用 `Channel` from `@tauri-apps/api/core`：

```ts
async importFiles(onProgress?: (p: Progress) => void) {
  const paths = await selectPaths(false);
  if (!paths.length) return null;
  const channel = new Channel<Progress>();
  channel.onmessage = (p) => onProgress?.(p);
  return invoke("import_paths", { paths, onProgress: channel });
}
```

浏览器 fixture adapter：忽略 `onProgress`（fixture 数据瞬时返回，无需模拟进度）。

#### 3. App.tsx 进度状态与渲染

- 新增 `progress` state：`useState<Progress | null>(null)`。
- `runSnapshotAction` 扩展或在 import/reanalyze/merge 调用点传 `onProgress: setProgress`。
- `finally` 块清除 `progress`（与 `busy` 一起清）。
- 进度 UI：
  - 导入：替换 `App.tsx:427-432` 的 `inline-progress`，改为确定性进度条 + 相位标签。
  - 重分析/合并：在 `App.tsx:754` 的 `busy-line` 位置渲染确定性进度条 + 相位标签。
  - `total === 0` 时回退为不定动画（保留现有 CSS 动画）。
  - 百分比计算：`total > 0 ? Math.round(current / total * 100) : null`。

#### 4. 样式

`src/styles.css` 新增确定性进度条样式：
- `.progress-determinate` —— 轨道 + 填充条（`width` 由 inline style 或 CSS 变量驱动，`transition: width 0.2s ease`）。
- 复用现有 `--accent` / `--surface` 变量与 `oklch()` 色彩。
- `reduced-motion`（`styles.css:718`）自动降级：`transition` 被覆盖为 ~0.01ms，宽度仍正确显示。

## Decision (ADR-lite)

**Context**: 导入/分析为 `spawn_blocking` 长耗时操作，前端只有不定 CSS 动画，用户无法判断进度。需要真实百分比。Tauri 2 提供 `ipc::Channel<T>` 作为命令参数注入的进度通道，无全局事件命名冲突。

**Decision**: 后端用 `tauri::ipc::Channel<ProgressPayload>` 分相位 emit；域层（`analysis.rs` / `importer.rs`）通过 `Option<&dyn Fn(usize, usize) + Send + Sync>` 回调保持 Tauri 无关；`commands.rs` 负责节流（50ms）与 payload 构造。前端 `Channel` + `onProgress` 回调更新 `progress` state，渲染确定性进度条 + 相位文字，`total=0` 回退不定。窗口用声明式 `"maximized": true`。

**Consequences**: 
- 域层签名变更（`analyze_records` / `importer::import_paths` 加可选参数），现有调用方传 `None`，零行为变化。
- `AppApi` 契约加可选 `onProgress`，浏览器 adapter 忽略它，向后兼容。
- 进度 Channel 是 per-call 的（每次 invoke 新建），无监听器泄漏。
- 节流避免高频 emit 对前端造成压力。
- `total=0` 相位（保存阶段）仍为不定，无百分比但保留相位标签。

## Technical Notes

* Tauri 2 `ipc::Channel<T>`：命令参数注入，前端 `new Channel()` 传入，`onmessage` 接收。`Channel` 是 `Clone + Send + Sync`。
* 域层回调用 `&dyn Fn(usize, usize) + Send + Sync`（不是 `FnMut`）以兼容 Rayon `par_iter`。
* `AtomicUsize` + `AtomicU64`（时间戳）用于并行计数与节流，无锁。
* 现有 `analyze_records` 调用方：`commands.rs` 的 `import_paths`、`reanalyze`、`merge_sessions`，以及 `analysis.rs` 测试。
* 现有 `importer::import_paths` 调用方：`commands.rs` 的 `import_paths`，以及 `importer.rs` 测试。
* 窗口配置文件：`src-tauri/tauri.conf.json` `app.windows[0]`。
* 前端关键文件：`src/App.tsx`（`runSnapshotAction` :235-262、`inline-progress` :427-432、`busy-line` :754）、`src/api/contract.ts`、`src/api/tauriApi.ts`、`src/domain/types.ts`、`src/styles.css`。
* 质量命令：`cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo test --manifest-path src-tauri/Cargo.toml`、`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`、`npm run lint`、`npm run test`、`npm run build`。
