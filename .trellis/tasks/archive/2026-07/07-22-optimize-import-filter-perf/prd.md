# 优化数据导入与页面筛选性能

## Goal

麦音分析工具已能稳定承载 45 万级 record 的历史会话，spec 后端分页/SQLite filter 契约也已经稳定。但用户反馈在大型会话下"页面筛选卡顿"。本任务**只聚焦筛选性能**：定位 storage.rs 筛选路径的真实瓶颈并针对性优化，让 45 万级 session 的首次筛选响应进入可接受区间（具体阈值待确认）。导入路径重构另开任务。

## What I already know

来自 pre-implementation 代码侦察与 FTS5 可索引性研究：

- Spec 合规度高，前后端没有"前端 filter 全量数据"反模式；本地 `filterPeople` 仅用于 browser preview。
- 所有 Tauri 重活命令都跑在 `spawn_blocking`，主线程只有 `bootstrap_workspace`（当前只 `list()`，便宜但结构性脆弱）。
- **筛选卡顿的真正来源是 storage.rs 的非 sargable LIKE**：
  - `household_region_norm LIKE '%x%'`（people + records）——前导通配符，强制全扫描
  - `search_text LIKE '%x%'`（people + records）——同上
  - `hotel_name_norm LIKE '%a%b%c%'`（records fuzzy；people 用 EXISTS per term）——完全非 sargable
  - `hotel_province_norm / city / county LIKE '%...%'`——同上，复合索引被 LIKE 击穿
  - 户籍 + 酒店区域 OR 在 EXISTS 里也击穿索引
- 每次查询新建 Connection + 4 条 PRAGMA，无连接池，每查询固定 overhead。
- `query_imported_records` 每页都做 `ensure_session_exists` + `metadata_from`（decode 4 列 JSON 但只需 settings）。
- 索引覆盖完整，没有缺失的索引；性能断崖是 sargability 问题。
- Schema 版本 = 3，spec 允许"清库重导入"作为迁移路径。

来自 [`research/sqlite-filter-indexability.md`](research/sqlite-filter-indexability.md) 的可索引性结论：

- **零 Cargo 改动**——`rusqlite = "0.32.1" features = ["bundled"]` 已自带 `SQLITE_ENABLE_FTS5`，trigram tokenizer 内建。
- **`household_region_norm LIKE '%x%'` → 拆分列等值/前缀**：records 表已有 `household_province_norm/_city_norm/_county_norm`（impoter 已填充）；people 表需在 v4 schema 里补这 3 列 + 索引；`PersonQuery` / `ImportedRecordsQuery` 已有对应字段，UI wire-ready。
- **`search_text LIKE '%x%'` → FTS5 trigram external-content 虚拟表**：3 字符以上 query 走 MATCH，1–2 字符 fallback 全扫描（仍正确）。`records` 是 implicit rowid 表，contentless FTS5 可用。
- **`hotel_name_norm LIKE '%a%b%c%'` (fuzzy ordered-subseq) → FTS5 trigram 预过滤 + 既有 LIKE 后过滤**：trigram MATCH 缩小候选集，LIKE 做最终正确性判定。语义 100% 保留。
- **`hotel_province_norm / city / county LIKE '%x%'` → 前缀 `LIKE 'x%'`**：现有 `idx_records_hotel_region` 多列索引可服务。语义从 substring 收紧为 prefix（产品决策待确认）。
- **`person_hotel_regions` OR-of-LIKEs → 用 province/city/county 等值/前缀，去掉 OR**：表里已有 3 列。
- **schema migration 免费**：bump `DATABASE_VERSION = 4`，把 `reset_legacy_database` 的 `version == 1 || version == 2` 扩到 `∈ {1,2,3}`。v3 用户清库重导入一次。
- **死索引清理**：`idx_records_household`（substring search 永不可走）、`idx_records_search`（同上）应删除。

## Open Questions

无——所有产品决策已收敛。

## Resolved Decisions

- **MVP 范围**：只做筛选性能。导入路径另开任务。
- **破坏性程度**：允许 schema 变更 + 清库重导入（spec 既允许）。
- **数据规模上限**：100 万 record/session 为预期上限，benchmark 必须覆盖到这个量级验证。
- **筛选响应阈值**：首次 apply 筛选在 100 万 session 上 ≤500ms。453k spec benchmark 自动达标，但需额外跑 1M 行实测。
- **区域过滤语义**：`household_province/city/county` 与 `hotel_province/city/county` 由 substring 收紧为 **prefix**（`LIKE 'x%'`），让 B-tree 索引生效。任意子串关键词搜索由既有 `search_text` FTS5 路径兜底。

## Requirements (evolving)

待 brainstorm 收敛。当前拟定方向（实现细节由 implement sub-agent 落地，但范围已基本明确）：

- **schema v4 升级**：bump `DATABASE_VERSION = 4`，扩展 `reset_legacy_database`，让 v3 也走"清库重建"；people 表补 `household_province_norm/_city_norm/_county_norm` 3 列 + 索引；删除死索引 `idx_records_household` 和 `idx_records_search`；新建 FTS5 trigram 虚拟表。
- **build_person_filter / build_records_filter 重写**：
  - 户籍/酒店区域 → 拆分列 + 等值/前缀（去掉 OR-of-LIKEs）；任意子串用 `search_text` FTS5 兜底
  - search_text → FTS5 trigram MATCH（≥3 字符）；保留 1–2 字符的 fallback LIKE
  - hotel_name fuzzy → FTS5 trigram MATCH 预过滤 + 既有 `fuzzy_pattern` LIKE 后过滤（语义 100% 保留）
  - 排除户籍过滤按新拆分列改写（prefix 语义）
- **每查询固定 overhead 收敛**：合并 `ensure_session_exists` + `metadata_from` 为单 lookup；`metadata_from` 只读 settings（用于 time window）的轻量变体，或合并进 query SQL。
- **EXPLAIN QUERY PLAN 验证**：所有优化路径在 453k session 上必须显示 `SEARCH ... USING INDEX` 而非 `SCAN`。
- **既有 spec test 保持绿**：multi-hotel AND fuzzy、household include/exclude、age/gender/risk/alert、imported-record 各 filter 不能回归。
- **453k + 1M benchmark 记录**：导入前后各跑 5 个标准 query，写入 `research/benchmark.md`；1M 实测必须包含 search_text / fuzzy / 户籍拆分列 / 酒店拆分列四类 fast path 与 fallback 路径。

## Acceptance Criteria (evolving)

- [ ] 100 万级 session 首次 apply 筛选响应 ≤500ms（45 万 spec benchmark 自动达标）
- [ ] EXPLAIN QUERY PLAN 显示 search_text / hotel_name_fuzzy / 户籍拆分列 / 酒店拆分列 各路径用 `SEARCH ... USING INDEX`，不再 `SCAN`
- [ ] 既有 spec test 全绿（multi-hotel AND fuzzy、household include/exclude、age/gender/risk/alert、imported-record 各 filter）
- [ ] 新增语义保留 test：trigram ≥3 字符 MATCH 等价于 LIKE contains；fuzzy ordered-subseq 通过 trigram 预过滤 + LIKE 后过滤仍 100% 匹配原结果；区域 prefix 改写后既有 `household_*` / `hotel_*` 测试用例不回归（含 exclude 取反路径）
- [ ] v3→v4 migration test：旧 v3 DB 启动后被清空重建，`PRAGMA user_version = 4`
- [ ] 453k + 1M benchmark 写入 `research/benchmark.md`，前后对比量化；1M 实测各 fast path ≤500ms
- [ ] spec 更新 `.trellis/spec/backend/database-guidelines.md`：v4 schema、FTS5 表、filter 走索引的事实

## Definition of Done (team quality bar)

- 测试补充覆盖新引入的 fast path（不破坏既有 spec test）
- Lint / typecheck / `cargo test` / `npm test` 全绿
- spec 若涉及契约变化，更新对应 spec 文件
- 测量数据写入 `research/` 或 info.md

## Out of Scope (explicit)

- **导入路径性能**（importer.rs 跨文件去重 + uid 分配单线程瓶颈）——另开任务。
- **`bootstrap_workspace` 主线程同步风险**——当前只调 `list()` 还便宜，结构性重构不在本 MVP。
- **`reanalyze` / `merge_sessions` / `export_result` 的 full-load 耗时**——spec 允许，且与筛选瓶颈正交。
- 跨平台打包优化
- 前端渲染性能（督察发现前端无瓶颈）
- browser preview 的 `filterPeople`/`recordMatchesImportedFilter`（仅 fixture 用，无规模问题）

## Technical Notes

- 关键文件：
  - `src-tauri/src/storage.rs`（`build_person_filter`, `build_records_filter`, `connection`, `metadata_from`, `query_people`, `query_imported_records`, `save`, `initialize_schema`, `reset_legacy_database`, `DATABASE_VERSION`）
  - `src-tauri/src/importer.rs`（户籍拆分列已在此填充）
  - `src-tauri/src/model.rs`（`PersonQuery` / `ImportedRecordsQuery` 已有 household 拆分字段）
- Spec 参考：
  - `.trellis/spec/backend/database-guidelines.md`（明确禁止本地全量 filter、要求单事务 + prepared、说明 45 万 / 15 文件 benchmark）
  - `.trellis/spec/backend/tauri-contract.md`
  - `.trellis/spec/frontend/state-management.md`
- 既有 benchmark（spec 引用）：453,506-person first-page、15-file parallel parsing——任务产出时按 spec 要求在 research 中记录实测数。

## Research References

- [`research/perf-recon.md`](research/perf-recon.md) — 性能现状代码侦察：9 文件 / 5 大类瓶颈定位，无方案。
- [`research/sqlite-filter-indexability.md`](research/sqlite-filter-indexability.md) — SQLite 可索引性研究：FTS5 trigram 已可用 / 5 类慢路径方案矩阵 / migration 路径 / EXPLAIN QUERY PLAN 验证清单。

## Decision (ADR-lite)

**Context**: 5 类慢筛选全部源于非 sargable LIKE；FTS5 trigram 已可用；spec 允许清库重导入作 schema 演进。

**Decision**: 单次 v4 schema 升级一并落地（拆分列 + FTS5 trigram 虚拟表 + 死索引清理 + filter 重写），不拆成多个 schema 版本。

**Consequences**: 
- 用户侧一次性清库重导入（产品决策 Q1 已允许）
- 实现复杂度集中，但 PR 内聚
- search_text 1–2 字符 query 不走 FTS5，但仍正确
- fuzzy hotel name 语义通过 layered MATCH+LIKE 100% 保留
