# 全仓代码体检报告 — 麦隐研判

> 生成日期: 2026-07-24 | 任务: 07-24-repo-code-audit
> 体检范围: `src/`（React 19 + TypeScript 6 + Vite 8）与 `src-tauri/src/`（Rust + Tauri 2）
> 体检方式: 静态阅读全量源码 + 跑通全部质量门；未修改任何业务代码。

---

## 1. 质量门基线

| 命令 | 结果 | 错误数 | 告警数 | 备注 |
|------|------|--------|--------|------|
| `npm run lint` | 通过 | 0 | 0 | eslint . 无输出，零告警 |
| `npm run test` | 通过 | 0 | 0 | vitest run：2 个测试文件，23 个用例全过（用时 12.82s） |
| `npm run build` | 通过 | 0 | 0 | tsc -b + vite build：29 模块，dist 产物正常（258.87 kB JS / 35.20 kB CSS） |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | 通过 | 0 | 0 | 无 diff |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 通过 | 0 | 0 | 45 passed / 8 ignored / 0 failed（忽略项均为带 env 门控的性能基准） |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` | 通过 | 0 | 0 | 无告警 |

**结论**: 六项质量门全部通过，无阻断项。代码体量热点（按实际行数，非 git ls-files 估算）：

| 文件 | 实际行数 | 备注 |
|------|----------|------|
| `src-tauri/src/storage.rs` | ~3913 | 持久化 + 建表 + SQL 过滤 + 压缩 + 批量插入 + 测试，明显偏大 |
| `src-tauri/src/importer.rs` | ~1220 | 解析 + 去重 + 历表头识别 + 测试 |
| `src/App.tsx` | ~1104 | 单文件含主组件 + 9 个子组件 + 8 个辅助函数 |
| `src-tauri/src/analysis.rs` | ~988 | 算分 + 预警 + 测试（测试约占一半） |
| `src/styles.css` | ~719 | 单一全局样式表 |
| `src-tauri/src/commands.rs` | ~524 | Tauri 命令 + 校验 + 快照组装 |

---

## 2. 维度概览

| 维度 | 发现数 | P0 | P1 | P2 | P3 |
|------|--------|----|----|----|----|
| 性能 | 3 | 0 | 0 | 2 | 1 |
| 结构 | 5 | 0 | 2 | 2 | 1 |
| 代码质量 | 7 | 0 | 0 | 2 | 5 |
| 类型安全 | 4 | 0 | 0 | 1 | 3 |
| **合计** | **19** | **0** | **2** | **7** | **10** |

整体评价：无 P0 阻断项，质量门全绿，DTO 跨层契约清晰、TS 端零 `any`。主要问题集中在“单文件/单组件过大”与“跨函数重复结构”两类可维护性隐患；性能侧无明显热点，仅有两条与超大会话/密集入住相关的边界隐患。

---

## 3. 详细发现

### 3.1 性能

#### [P2] merge_sessions 用分隔符拼接字符串做去重键，与 importer 的结构化键不一致
- **文件**: `src-tauri/src/commands.rs:439-453`（`record_key`），调用点 `commands.rs:157-165`
- **问题**: 合并会话时 `record_key` 把 10 个字段用 `\u{1f}` 拼成一个 `String` 作为 `HashSet` 去重键——这正是 `tauri-contract.md` 在 importer 场景明确列为 Wrong 的模式。importer 自身已改用结构化 `DeduplicationKey`（`importer.rs:54-66`、`800-813`），但合并路径仍沿用旧拼接键。合并多个 453k 级会话时，每条记录都要 clone 10 个字段并分配一个拼接字符串，开销随合并规模线性放大。
- **建议方向**: 在 `model` 或共享模块暴露 importer 的 `DeduplicationKey`（及对应 `DateKey`），让 `merge_sessions` 复用同一结构化键，消除拼接分配并统一两处去重语义。
- **建议子任务**: `merge-dedup-key-reuse-structured-key`

#### [P2] analysis 单人重叠检测为 O(n²)，密集入住人员放大开销
- **文件**: `src-tauri/src/analysis.rs:136-158`
- **问题**: `analyze_person` 对单人记录做双层 `for` 枚举所有 `(first, second)` 对判定时间重叠。对稀疏人员，`second_start >= first_end` 的 `break` 能早停；但对“同一天多 record”的密集人员（例如 benchmark_dense_overlap_analysis 的 800 条 → 32 万对），仍是 n²/2 量级。该路径已在 `spawn_blocking` 上且有基准覆盖，不会阻塞 UI，但极端密集人员（上千条同日入住）会拉长分析耗时。
- **建议方向**: 评估按 `(check_in, effective_end)` 区间排序后用扫描线/事件点统计替代两两比较，或对单日 record 数设阈值切换分桶策略；保留现有基准回归。
- **建议子任务**: `analysis-overlap-scanline-for-dense-persons`

#### [P3] importer 去重键为每条记录 clone 全部字段
- **文件**: `src-tauri/src/importer.rs:800-813`（`deduplication_key`）
- **问题**: 结构化 `DeduplicationKey` 已避免了“拼接字符串”的额外分配（符合 spec 的 Correct 范式），但构建键时仍对 10 个 `String` 字段逐个 `clone()`。453k 记录导入约产生 ~450 万次字符串 clone 用于 `HashSet` 插入。属 spec 已知且已优化过的权衡，仅作低优先记录。
- **建议方向**: 探讨借用键（`&str` 绑定 record 生命周期，插入后即弃）或先哈希字段再比较的方案；仅在基准显示为瓶颈时落地。
- **建议子任务**: `importer-dedup-key-borrowed-or-prehashed`

---

### 3.2 结构

#### [P1] storage.rs 单文件 ~3913 行，职责混杂
- **文件**: `src-tauri/src/storage.rs:1-3913`
- **问题**: 一个文件同时承载：`SessionStore` 实现（open/list/save/replace_analysis/load/query_people/query_imported_records/person_detail/delete/move_to，`104-833`）、schema 建表与版本迁移（`initialize_schema 894-1056`、`reset_legacy_database 1058-1078`）、两个大型 SQL 过滤构建器（`build_person_filter 1166-1295`、`build_records_filter 1297-1419`）、record/person 预处理（`1585-1698`）、6 个批量插入函数（`1750-2002`）、JSON 压缩辅助（`2099-2154`），外加 ~1500 行测试。阅读与定位成本高，任何改动都要在超长文件里跳转。
- **建议方向**: 按职责拆分模块——`schema`（建表/迁移）、`filter_builder`（两个 where 构建器）、`batch_insert`（6 个批量插入）、`json_compress`（MYL4 读写）；`SessionStore` impl 留主文件。测试可独立 `storage/tests.rs`。
- **建议子任务**: `split-storage-rs-by-responsibility`

#### [P1] App.tsx 是 ~1104 行的“上帝组件”
- **文件**: `src/App.tsx:72-1104`
- **问题**: 主 `App` 组件（`72-751`）单组件内含 ~30 个 `useState`、多个 `useEffect`，同时负责 bootstrap、快照动作编排（导入/合并/删除/重算）、人员分页、导入记录分页、筛选草稿、详情抽屉、设置面板、删除确认等全部交互；同文件还内联了 9 个子组件（`TableSkeleton 753`、`ImportedRecordsTable 757`、`PageSizeSelect 883`、`DetailInspector 895`、`SettingsPanel 1006`、`Field/NumberField/DateTimeField 1028-1038`、`ConfirmDialog 1040`、`EmptyWorkspace 1044`、`LoadingShell 1048`）与 8 个辅助函数。状态扁平堆叠导致任一 state 变更都可能触发大范围重渲染，逻辑也难以单测。
- **建议方向**: 子组件拆分到 `src/components/`；按域抽取自定义 hook（`usePeoplePage`、`useImportedRecordsPage`、`useDisclosure`、`useSnapshotAction`）；主 `App` 仅保留壳层编排。
- **建议子任务**: `decompose-app-tsx-into-components-and-hooks`

#### [P2] build_person_filter 与 build_records_filter 结构重复约 60%
- **文件**: `src-tauri/src/storage.rs:1166-1295` 与 `1297-1419`
- **问题**: 两个过滤构建器各自实现同一套“旅馆区域 EXISTS/直接子句 + 户籍包含 splits + 户籍排除”逻辑，仅列名前缀不同（人员侧用 `p.`/`phr.`+`person_hotel_regions` 子查询；记录侧用裸列名）。任一筛选语义调整都需在两处同步修改，易漂移。
- **建议方向**: 抽取一个接收“列名前缀映射 + 是否走 EXISTS”参数的共享构建器，两个入口只提供各自的列名/表名映射。
- **建议子任务**: `unify-storage-filter-builders`

#### [P2] parse_file ~115 行内含四段职责
- **文件**: `src-tauri/src/importer.rs:185-300`
- **问题**: 单函数依次完成：读表、表头/模板/推断的三段式分派（`188-210`）、逐行字段抽取（`216-247`）、`Record` 构造（`261-293`）。表头分派与行循环各自可独立成函数，便于单测与阅读。
- **建议方向**: 拆出 `resolve_data_start_and_indexes(&rows) -> (start, indexes)` 与 `build_record(row, indexes, ...) -> Option<Record>`，`parse_file` 仅做编排。
- **建议子任务**: `split-importer-parse-file`

#### [P3] 批量插入家族重复同一 chunk→拼 SQL→push 值→execute 模式 8 次
- **文件**: `src-tauri/src/storage.rs:1750-2002`（`insert_record_batches`、`insert_person_batches`、`insert_alert_batches`+`execute_alert_batch`、`insert_person_hotel_batches`+`execute_person_hotel_batch`、`insert_person_hotel_region_batches`+`execute_person_hotel_region_batch`）
- **问题**: 8 个函数（5 个 `insert_*_batches` + 3 个 `execute_*_batch`）同一骨架，且存在两种不一致风格：record/person 两个直接在循环里 push 值；alert/hotel/region 三个先 flatten 进 `rows` Vec 再委托 `execute_*_batch`。Rust 元组 arity 差异使干净的泛型较难，但可用宏统一骨架并统一两种风格。
- **建议方向**: 引入 `bulk_insert!` 宏或公共 `chunk_and_insert` 骨架，统一先 flatten 再批量执行的写法。
- **建议子任务**: `unify-storage-batch-insert-boilerplate`

---

### 3.3 代码质量

#### [P2] 前端 applySettings 复制了 Rust validate_settings 的校验规则
- **文件**: `src/App.tsx:270-290`（前端）与 `src-tauri/src/commands.rs:388-421`（后端）
- **问题**: 同一套“阈值 ≥ 1 / selected 需同时有起止 / 起不晚于止”规则在两层各写一遍。前端版本是为提交前给即时 toast（符合 cross-layer guide 的“入口预校验”意图），但规则一旦在 Rust 侧调整，前端极易漏改而漂移。
- **建议方向**: 把阈值边界与“selected 必填项”这类纯规则提炼为共享常量/JSON 契约（或至少在前端注释标注“须与 commands::validate_settings 同步”），并在两层加交叉测试守卫。
- **建议子任务**: `unify-analysis-settings-validation`

#### [P2] filterPeople 与 recordMatchesImportedFilter 重复筛选编排
- **文件**: `src/lib/filter.ts:3-52` 与 `129-176`
- **问题**: 两个函数各自重新推导旅馆关键词、旅馆区域 splits、户籍包含/排除 splits、age/gender、search，并按几乎一致的顺序串成谓词。共享原语（`splitFilterTerms`、`matchesAnySubstring`、`matchesHouseholdRegion`）已复用，但上层编排是拷贝粘贴。两者均仅服务浏览器演示模式（生产走 Rust），但仍是维护负担。
- **建议方向**: 抽取 `buildRecordPredicate(query)` / `buildPersonPredicate(query)` 中心化编排，两个导出函数只做调用与分页。
- **建议子任务**: `deduplicate-frontend-filter-orchestration`

#### [P3] read_workbook 与 read_legacy_xls 重复“打分择优 sheet”逻辑
- **文件**: `src-tauri/src/importer.rs:313-361` 与 `363-406`
- **问题**: 两个函数都以同样结构遍历 sheet、调用 `detect_template_data_start`/`detect_header_row`/`infer_core_fields`，并用 `best_score`/`best_rows` 兜底。可抽公共 `score_and_pick_sheet` helper。
- **建议方向**: 提取共享择优函数，calamine 与 rxls 两条路径只负责把 sheet 转成行后调用它。
- **建议子任务**: `deduplicate-importer-sheet-scoring`

#### [P3] exporter.rs 重复 `.map_err(|error| AppError::Export(error.to_string()))` 约 14 处
- **文件**: `src-tauri/src/exporter.rs:23, 39, 41, 65, 86, 90, 114, 139, 143, 151, 174, 204, 212, 217`
- **问题**: storage 模块已用 `sql_error`/`storage_error` 收敛同类映射，exporter 仍逐处内联同一闭包。抽一个 `export_error(e) -> AppError` 即可。
- **建议子任务**: `exporter-extract-error-helper`

#### [P3] browserApi toImportedStayRecord 用解构 + 9 个 `void` 丢弃字段
- **文件**: `src/api/browserApi.ts:82-104`
- **问题**: 为从 `DemoImportedRecord` 去掉筛选专用字段，先解构出 9 个字段再逐个 `void` 抑制未用告警，意图不直观。一个小 `omit` 工具或显式重建对象更清晰。
- **建议子任务**: `browserapi-clean-record-shaping`

#### [P3] activeExtraFilterCount 与 activeRecordsFilterCount 近乎全等
- **文件**: `src/App.tsx:1071-1078` 与 `1080-1086`
- **问题**: 两个计数函数仅差末尾“预警状态”一项，其余旅馆/区域/户籍/年龄性别计数完全一致，属可合并的重复。
- **建议方向**: 抽 `activeFilterCount(query, includeAlertState)` 共享前 4 段。
- **建议子任务**: `deduplicate-active-filter-counters`

#### [P3] save() 内穿插 8 处 `#[cfg(test)]` 计时块，干扰生产路径阅读
- **文件**: `src-tauri/src/storage.rs:190-203, 257-258, 330-331, 343-344, 362-363, 367-368, 370-371, 383-384`
- **问题**: 203 行的 `save` 中嵌入了 `save_mark` 计时门控，使主流程被 `#[cfg(test)]` 块频繁打断。计时逻辑有价值但应外置。
- **建议方向**: 把计时收进一个受 `MAIYIN_SAVE_TIMINGS` 门控的小 trait/闭包，或把基准拆到独立 benchmark 入口，让 `save` 主干保持干净。
- **建议子任务**: `storage-save-remove-inline-timing`

---

### 3.4 类型安全

> 正向记录：全仓 TS 代码零 `any` / `as any` / `@ts-ignore` / `@ts-expect-error`（grep 全空），DTO 与 Rust `#[serde(rename_all = "camelCase")]` 一一对应，`errorMessage`（`App.tsx:1095-1102`）对 `unknown` 的结构化收窄正确。类型纪律整体优秀。下列为 Rust 侧 panic 风险与一处 DTO 一致性。

#### [P2] analysis 生产路径存在 4 处 `.expect()`
- **文件**: `src-tauri/src/analysis.rs:363, 364, 379, 382`
- **问题**: `different_accommodation_cached` 用 `cache.get(&first.uid).expect("first location is cached")`（363-364）、`day_ranges` 用 `.expect("scoped analysis records have check-in times")`（379）与 `.expect("day exists")`（382）。这些依赖由紧邻代码维持的不变式，但分析运行在 `spawn_blocking` 上，一旦未来重构破坏不变式即 panic → 转为 `task_error`，整次重算失败且无结构化错误。
- **建议方向**: 改为返回 `Option`/`Result` 并在调用处显式处理（或 `unwrap_or` 配文档化回退），消除生产路径的 panic 面。
- **建议子任务**: `analysis-remove-production-expect`

#### [P3] importer 静态正则用 `Regex::new(...).unwrap()`
- **文件**: `src-tauri/src/importer.rs:751, 766`
- **问题**: age 与 identity 正则经 `OnceLock::get_or_init` 编译并 `unwrap`。模式为常量，当前安全；但若日后误改成非法模式，首次调用即在导入热路径 panic。改 `expect` 带静态说明或编译期校验更稳妥。
- **建议子任务**: `importer-regex-safe-init`

#### [P3] lib.rs 启动用 `expect("failed to run maiyin analysis")`
- **文件**: `src-tauri/src/lib.rs:39`
- **问题**: `tauri::Builder::...run(...).expect(...)` 启动失败直接 panic 且无用户可见信息。属 Tauri 入口惯用写法，但转成日志化错误对话框能改善首启失败体验。
- **建议子任务**: `lib-startup-error-dialog`

#### [P3] PersonSummary 三个 max*Count 字段可选性不一致
- **文件**: `src/domain/types.ts:45-47`（TS）映射 `src-tauri/src/model.rs:164-167`（Rust serde default）
- **问题**: `maxWeekCount?` 为可选，而 `maxMonthCount`/`maxYearCount` 必填，但三者在 Rust 端由 `analyze_records` 同时填充（`analysis.rs:301-303`）。Rust 侧仅 `max_week_count` 带 `#[serde(default)]`（`model.rs:164-165`），另两个无 default——TS 的可选性只是镜像了 serde 差异，语义上三者对新载荷始终同时存在。属 DTO 一致性瑕疵，不影响运行。
- **建议方向**: 统一三者可选性（均带 serde default 且 TS 均可选，或均必填），并补一条 legacy 兼容测试。
- **建议子任务**: `align-person-summary-max-count-optionality`

---

## 4. 速赢清单 (Quick Wins)

低风险、小范围、可独立立即修复的项汇总：

- `src-tauri/src/exporter.rs:23..217` — 抽 `export_error(e)` helper，收敛 ~14 处重复 `map_err`。
- `src/api/browserApi.ts:82-104` — 用 `omit` 工具或显式重建替代“解构 + 8 个 `void`”的字段裁剪。
- `src/App.tsx:1071-1086` — 合并 `activeExtraFilterCount`/`activeRecordsFilterCount`，仅差“预警状态”一项。
- `src-tauri/src/importer.rs:313-406` — 抽 `score_and_pick_sheet`，消除 calamine/rxls 两条路径的择优重复。
- `src-tauri/src/storage.rs:190-384` — 把 `save` 内 8 处 `#[cfg(test)]` 计时收进门控闭包，净化生产主干。
- `src-tauri/src/importer.rs:751,766` — 静态正则 `unwrap` 改 `expect` 带静态说明，降低误改即 panic 风险。
- `src/domain/types.ts:45-47` + `src-tauri/src/model.rs:164-167` — 统一三个 `max*Count` 的可选性/serde default 一致性。
- `src-tauri/src/lib.rs:39` — 启动 `expect` 改为日志化错误提示，改善首启失败体验。

---

## 5. 建议子任务列表

| # | slug | 维度 | 严重度 | 简述 |
|---|------|------|--------|------|
| 1 | `split-storage-rs-by-responsibility` | 结构 | P1 | 拆分 storage.rs（schema / filter_builder / batch_insert / json_compress / tests） |
| 2 | `decompose-app-tsx-into-components-and-hooks` | 结构 | P1 | 拆分 App.tsx：子组件外移 + 按域抽 hook |
| 3 | `merge-dedup-key-reuse-structured-key` | 性能 | P2 | merge_sessions 复用 importer 的结构化 DeduplicationKey |
| 4 | `analysis-overlap-scanline-for-dense-persons` | 性能 | P2 | 密集人员重叠检测改扫描线，保留基准回归 |
| 5 | `unify-storage-filter-builders` | 结构 | P2 | 抽共享 where 构建器，消除两个 filter 的 ~60% 重复 |
| 6 | `split-importer-parse-file` | 结构 | P2 | 拆 parse_file 为“定起始+列映射”与“逐行构造”两段 |
| 7 | `unify-analysis-settings-validation` | 代码质量 | P2 | 前后端阈值/时间校验规则共享并加交叉测试守卫 |
| 8 | `deduplicate-frontend-filter-orchestration` | 代码质量 | P2 | 抽 buildPersonPredicate/buildRecordPredicate 统一 filter 编排 |
| 9 | `analysis-remove-production-expect` | 类型安全 | P2 | 移除 analysis 生产路径 4 处 .expect()，改 Option/Result |
| 10 | `importer-dedup-key-borrowed-or-prehashed` | 性能 | P3 | 探讨去重键借用化/预哈希，仅在基准瓶颈时落地 |
| 11 | `unify-storage-batch-insert-boilerplate` | 结构 | P3 | 宏统一 8 个批量插入函数骨架与两种风格 |
| 12 | `deduplicate-importer-sheet-scoring` | 代码质量 | P3 | 抽 score_and_pick_sheet，消除 workbook 两条路径择优重复 |
| 13 | `exporter-extract-error-helper` | 代码质量 | P3 | 抽 export_error helper，收敛重复 map_err |
| 14 | `browserapi-clean-record-shaping` | 代码质量 | P3 | 用 omit 工具替代解构+void 的字段裁剪 |
| 15 | `deduplicate-active-filter-counters` | 代码质量 | P3 | 合并两个 activeFilterCount 函数 |
| 16 | `storage-save-remove-inline-timing` | 代码质量 | P3 | save 内 cfg(test) 计时外置，净化生产主干 |
| 17 | `importer-regex-safe-init` | 类型安全 | P3 | 静态正则 unwrap 改 expect 带说明 |
| 18 | `lib-startup-error-dialog` | 类型安全 | P3 | 启动 expect 改日志化错误提示 |
| 19 | `align-person-summary-max-count-optionality` | 类型安全 | P3 | 统一三个 max*Count 的可选性与 serde default |

---

**附注**:
- 本次体检未触碰任何业务代码（`src/` 与 `src-tauri/src/` 均未修改），唯一产物为本报告。
- 建议优先落 P1 两项（storage.rs 拆分、App.tsx 解耦），二者会显著降低后续所有子任务的改动成本；其余 P2/P3 可按子任务独立排期。
- 所有发现均带 `file:line` 定位，行号基于体检当日源码（部分文件较 PRD 中 git ls-files 估算略有增长）。
