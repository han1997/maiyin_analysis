# 性能现状代码侦察报告

> 仅描述当前实现与潜在慢路径，不提出方案。
> 来源：pre-implementation 探查（explore sub-agent），覆盖 9 个核心文件。

## 概览

- **Spec 合规度整体良好**：前后端均遵守 "后端分页 + SQLite 端 filter/count/sort/page" 契约；本地 filterPeople 只存在于 browser preview。
- **真正的性能瓶颈集中在 storage.rs 的非 sargable（不可走索引）LIKE 模式**，而不是架构层面的"前端 filter 太多"。

---

## 1. 前端 (src/App.tsx, src/api/*, src/lib/filter.ts)

- 每个 `[snapshot, query]` 变化触发一次 `appApi.queryPeople`，effect cleanup 正确丢弃过期响应。
- 搜索 / level 编辑不自动 apply，需要点击"应用筛选"——无 debounce 缺口。
- Pagination / pageSize 变化直接 mutate `query` 立即发请求——预期行为。
- `src/lib/filter.ts` 的 `filterPeople` / `recordMatchesImportedFilter` **只被 browserApi.ts 使用**，production 未泄漏。
- **前端无显著性能问题。**

---

## 2. Tauri 命令层 (src-tauri/src/commands.rs)

- `import_paths` / `import_folders` / `merge_sessions` / `reanalyze` / `query_people` / `get_imported_records` / `get_person_detail` / `export_result` **全部使用 `spawn_blocking`**，重活都在工作线程。
- `bootstrap_workspace` 是**唯一同步命令**（非 `async`），跑在 Tauri 主线程上。当前工作只是 `store.list()`（按 session 数 bounded），便宜；但 spec 明确警告主线程 DB 工作会导致白屏——属于结构性脆弱点。
- 每个命令做 `lock(&state)?` 克隆 `SessionStore`（仅 `PathBuf + PathBuf`），锁释放后再 `spawn_blocking`，锁竞争可忽略。
- **`merge_sessions` / `reanalyze`** 调用 `store.load`，会 deserialize 整个 session 的 `record_json`（数十万行 JSON decode）。spec 明确允许（merge/reanalyze/export 需要全量 record），但这是耗时源，且没有进度反馈。

---

## 3. 导入路径 (src-tauri/src/importer.rs)

- 文件级 Rayon `par_iter` 并行解析（符合 spec 的 "15-file parallel parsing" benchmark）。
- **跨文件去重 + uid 分配是单线程**（importer.rs:117-133），构建 450k 项的 `HashSet<String>`，每项 join 10 个字段（\u{1f} 分隔）——这是 import 的主要内存/CPU 热点。
- `analyze_records` 自己用 `into_par_iter` 按 person 并行分析（analysis.rs:19）——OK。
- 没有批量化、没有 prepared statement——importer 不直接写 DB，所有 DB 写入在 `SessionStore::save`。
- `read_workbook` 把整个 sheet 物化为 `Vec<Vec<String>>`——内存峰值 = 整 sheet 以字符串形式。
- `read_workbook` 对所有 sheet 最多扫 2 遍以检测模板——多 sheet 文档付出双倍代价。
- `legacy_xls` 路径用 `rxls::Workbook`，过度分配稀疏的 dense 2D 表，仅在 calamine 失败时触发。

---

## 4. SQLite 持久层 (src-tauri/src/storage.rs) — 性能关注核心

### 4.1 连接管理

- `SessionStore` 是 `Clone`（只有两个 PathBuf），**每次方法调用都 `Connection::open` + 执行 4 条 PRAGMA**（`connection()` at line 605）。
- 无连接池，无 `Arc<Mutex<Connection>>`。
- WAL 模式在 `initialize_schema` 中设置一次；后续每次连接只付 `foreign_keys/busy_timeout/synchronous/temp_store` 四条 PRAGMA 开销。
- 对 query_people / get_imported_records / get_person_detail 而言是**每次请求的固定 overhead**（虽然单次不大）。

### 4.2 query_people (line 375)

- `OFFSET` 分页（`LIMIT ? OFFSET ?`）——深页扫描并丢行；`idx_people_sort` 覆盖 sort key，sort 走索引。
- COUNT(*) + paged SELECT 两次 prepare（每次 `format!` 生成 SQL 字符串，`prepare_cached` 未使用）。
- 多酒店名 AND：每个 term 一个 `EXISTS (... LIKE '%...%')` 相关子查询，每个用 `idx_person_hotels_lookup (session_id, person_key, hotel_name_norm)`——多 term 时 O(terms × persons)。
- 酒店区域：`EXISTS (... province_norm LIKE ? OR region_norm LIKE ?)`——OR 击穿索引。
- **户籍地 include/exclude：`household_region_norm LIKE '%x%'`** —— 前导通配符 LIKE，**不可走任何索引**，session 分区内全表扫描。453k 行时每条 clause 每 query 453k 次字符串比较。
- 每行 `summary_json` decode；page size clamped ≤500——bounded。

### 4.3 query_imported_records (line 420)

- `ORDER BY check_in ASC, uid ASC` 走 `idx_records_check_in`——OK。
- 每次 page 请求都调用 `ensure_session_exists` + `metadata_from`（两条独立 point lookup）。
- `metadata_from` 每次 decode 4 列 JSON（`settings_json`, `stats_json`, `import_stats_json`, `source_session_ids_json`）——但这里**只需要 `settings` 用于时间窗**。
- 字段级 filter：
  - `search_text LIKE '%x%'`（非 sargable）
  - `hotel_name_norm LIKE fuzzy_pattern('%a%b%c%')`（前导 + 字符间 `%`，完全非 sargable）
  - `hotel_province_norm LIKE '%...%'` / city / county（非 sargable；`idx_records_hotel_region` 多列索引仅在左列等值时有用，被 LIKE 击穿）
  - `household_region_norm LIKE '%x%'`（非 sargable）
- 每行 decode `record_json`；page ≤500 bounded。

### 4.4 save (line 100)

- **单一 transaction + prepared statements**：records / people / alerts / person_hotels / person_hotel_regions 全在一个事务里。
- `DELETE FROM sessions WHERE listed = 0 AND session_id <> ?1` 清理瞬时合并会话（line 104）——OK。
- 失败回滚到 previous 完整 session。
- Spec 完全合规。

### 4.5 load (line 314)

- 全量 deserialize：所有 `record_json` + `alert_json` + `summary_json`。
- 仅被 `reanalyze` / `merge_sessions` / `export_result` 调用——spec 允许。
- **不用于浏览路径**——好。

### 4.6 索引现状（line 719-731）

10 个索引，覆盖 sort / session 分区 / 部分等值前缀。**没有任何索引缺失**。性能断崖来自 sargability，不是缺索引。

---

## 5. 横切观察（事实陈述）

1. **Spec 合规度高**，前后端没有"本地 filter 全量数据"反模式。
2. **导入并行度仅在文件粒度**；跨文件去重 + uid 分配单线程，是 import path 的内存/CPU 瓶颈。
3. **筛选性能瓶颈集中在 storage.rs 的 `build_person_filter` / `build_records_filter`**——每条 text/region/search 子句都编译成 `LIKE '%...%'`，session 分区内强制全扫描。现有索引只能加速 sort 和分区裁剪，无法加速子串匹配。
4. **每次方法调用都新建 Connection + 4 条 PRAGMA**，给每个查询加固定 overhead；无连接池。
5. **`bootstrap_workspace` 同步跑在主线程**——当前只调用 `list()` 还便宜，但属于 spec 警告的"主线程 DB 工作"结构性风险。
6. **`metadata_from` 过度 decode**——`query_imported_records` 只需要 `settings` 却 decode 4 列 JSON。
7. **跨层 filter 逻辑重复**——`src/lib/filter.ts` 与 `build_person_filter` / `build_records_filter` 语义平行。非性能问题，是维护风险。
8. save 路径事务/批处理/prepared 都合规；importer 不写 DB；未见 N+1。
9. **没有缺失的索引**——性能断崖是 sargability，不是索引缺失。
