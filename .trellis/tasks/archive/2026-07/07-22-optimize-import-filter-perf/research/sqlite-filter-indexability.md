# SQLite Filter Indexability Research

- **Query**: Which SQLite techniques can move the 5 slow non-sargable LIKE clause types off the full-session scan path? Decision matrix for implement agent.
- **Scope**: mixed (internal code read + external SQLite/rusqlite docs)
- **Date**: 2026-07-22

## TL;DR (single most important fact)

The project's existing `rusqlite = "0.32.1" features = ["bundled"]` **already compiles the SQLite amalgamation with `-DSQLITE_ENABLE_FTS5`** (verified in `libsqlite3-sys 0.30.1/build.rs:129`). FTS5 + the `trigram` tokenizer are available with **zero Cargo.toml changes**. The trigram tokenizer gives indexed substring match for queries ≥3 chars — *but* it cannot optimize the existing `LIKE ? ESCAPE '\\'` clauses (ESCAPE defeats trigram LIKE-opt), so the implementation must rewrite the filter to either `MATCH` syntax or raw `LIKE` without ESCAPE.

Schema migration is free: spec allows "clear data + re-import" as the only migration path, and `storage.rs:623` already wipes v1/v2 → v3 by `DROP TABLE`. A v4 schema would just add `|| version == 3` to that branch and rebuild.

---

## Verdict per slow-clause type

| Slow clause | Recommended technique | Cost | Notes |
|---|---|---|---|
| `household_region_norm LIKE '%x%'` | **Switch to existing split columns** `household_province_norm = ?` / `_city_norm = ?` / `_county_norm = ?` (already populated + indexable as `=`/`prefix LIKE`) — index `(session_id, household_province_norm, household_city_norm, household_county_norm)`. | None — re-use existing columns, add one index, rewrite 4 filter call-sites. | Re-reads importer-populated splits (storage.rs:188-190). `household_region_norm` (concatenated) becomes unused for filter (keep for display / search_text). |
| `search_text LIKE '%x%'` | **FTS5 trigram external-content table** over `records.search_text` and `people.search_text`. | ~3× write-time for those two columns at import (small — text is 60–200 chars). +1–2 shadow tables. | Trigram ≥3 chars only. 1–2 char Chinese queries (rare for `search_text`) fall back to scan. Existing `idx_records_search` is dead weight; remove. |
| `hotel_name_norm LIKE '%a%b%c%'` (fuzzy ordered subseq) | **Keep `fuzzy_pattern` AND overlay an FTS5 trigram table** as a pre-filter: `WHERE hotel_name_norm MATCH ? AND hotel_name_norm LIKE ? ESCAPE '\\'`. MATCH narrows via trigram; LIKE applies ordered-subseq on the small candidate set. | ~3× write for hotel_name_norm column. | Trigram MATCH semantics ≠ ordered subsequence — MUST keep the LIKE for correctness. Trigram reduces the scan from N rows to typically <<1%. Verify with EXPLAIN QUERY PLAN. |
| `hotel_province_norm / _city / _county LIKE '%x%'` | **Relax to prefix `LIKE 'x%'`** (no leading `%`) on indexed columns. Existing `idx_records_hotel_region ON (session_id, hotel_province_norm, hotel_city_norm, hotel_county_norm)` already serves it. | None. | Region names: user types province like `浙江` — prefix match `浙江省` covers it. Substring semantic was overkill. Requires product sign-off (see Open Questions). |
| `person_hotel_regions` OR of two LIKEs | **Drop the redundant `region_norm LIKE ? OR column LIKE ?`** by indexing `(session_id, person_key, province_norm, city_norm, county_norm)` and using equality/prefix per column. The second `region_norm` LIKE is a fallback for concatenated text — replace by querying the 3 specific columns. | One covering index. | Eliminates the OR-of-LIKE anti-pattern that defeats any index. |

---

## Technique deep-dives

### 1. FTS5 + trigram tokenizer

**Availability**: ✅ Available with current `rusqlite = "0.32.1" features = ["bundled"]`. Verifiable by `SELECT sqlite_compilepoint_used()` / `PRAGMA compile_options` — should list `ENABLE_FTS5`. The `trigram` tokenizer is a built-in FTS5 tokenizer free of any feature flag (added SQLite 3.34, 2020-12). rusqlite 0.32.1's bundled amalgamation is far newer.

**Substring semantics**: The trigram tokenizer treats every 3-char window as a token. `LIKE '%cdefg%'` and `MATCH 'cdefg'` both result in intersecting doclists for the trigrams `cde`, `def`, `efg` — i.e. **substring containment**, semantically equivalent to `LIKE '%x%'` for patterns ≥3 chars.

**Critical gotcha — ESCAPE clause defeats LIKE optimization**: SQLite FTS5 docs explicitly state: *"The index cannot be used to optimize LIKE patterns if the LIKE operator has an ESCAPE clause."* The codebase uses `LIKE ? ESCAPE '\\'` everywhere (storage.rs:823, 852, 873, 893, 909, 943, 960, 972, 983, 999). Two implementation paths:
- **Path A (preferred)**: rewrite filter to use FTS5 `MATCH` syntax directly: `WHERE search_text_fts MATCH ?`. The pattern is the raw user string (after `normalize`). For substring contains, trigram's default MATCH does substring. Quote the query string to avoid FTS5 boolean parsing (e.g. wrap user text in double quotes).
- **Path B**: keep `LIKE '%x%'` but remove the `ESCAPE '\\'` clause — only safe if the input cannot contain `%`, `_`, `\` after `normalize()`. Need to verify `normalize` strips these (currently it just lowercases + collapses whitespace — see storage.rs explicitly).

**Minimum pattern length**: Trigram tokenizer requires ≥3 unicode chars. 1-char and 2-char queries fall back to **linear scan of the FTS table** (still far smaller than the main table) — but if the user types 2 chars `浙江` for province filtering, trigram will NOT help. This is one of the reasons region columns should use prefix `=`/`LIKE 'x%'` on B-tree indexes rather than FTS5.

**Chinese-specific behavior**: Trigram is character-based, not whitespace-based — works for Han text. Diacritics (Latin) don't matter for Chinese. No ICU dependency.

**External-content pattern** (recommended for our case — we don't want to duplicate the source text into FTS5 shadow):
```sql
CREATE VIRTUAL TABLE records_search_fts USING fts5(
    search_text,
    content='records',
    content_rowid='rowid'  -- but records PK is (session_id, uid), no integer rowid
);
```
Problem: `records` has a composite PK `(session_id, uid)` and no integer rowid. External content FTS5 needs a single `content_rowid` column. Two options:
- **Option (a) – contentless**: `content=''` + manually run `INSERT INTO records_search_fts(rowid, search_text) VALUES (?, ?)` at import. Need an integer rowid — either `rowid` auto (records has implicit rowid since not declared WITHOUT ROWID) or assign uid. Manual sync only — no triggers required.
- **Option (b) – add integer surrogate**: add `id INTEGER PRIMARY KEY AUTOINCREMENT` to records and let triggers populate FTS5.

Option (a) is simpler given we already control all inserts (single importer.rs path). For deletes, contentless-delete tables (3.43+) support DELETE without rebuild — check rusqlite bundled version is ≥3.43 (it is, far newer).

### 2. FTS5 + unicode61

**Suitability**: NO — wrong semantic. unicode61 tokenizes by Unicode letter categories. Chinese text has **no word boundaries** so unicode61 produces one token per contiguous Han run — useless for substring search (`MATCH 'zhejiang'` won't match `浙江省` text). It's the right choice for whitespace-delimited natural language (English), not for Chinese sub-string matching. Skip.

There IS a `categories` option to extend token classes, but it doesn't change the fundamental "token = run of token chars" rule that breaks substring search.

### 3. Generated columns (`GENERATED ALWAYS AS`)

**Useful for**: pre-computing a coarse bucket to narrow scans where FTS5 is overkill. Example: `hotel_first2 TEXT GENERATED ALWAYS AS (substr(hotel_name_norm, 1, 2)) STORED` + index → fast lookup of hotels starting with `如家`.

**For our 5 clauses**: Not the primary fix — region splits + FTS5 cover the cases better. Could be a fallback for 1–2 char hotel queries if FTS5 trigram floor blocks them.

**rusqlite support**: Pure SQL — `GENERATED ALWAYS AS` works since SQLite 3.31 (2020). Bundled SQLite supports it. No Cargo changes.

### 4. Reverse 2/3-gram child table

A hand-rolled alternative to FTS5 trigram: a child table `record_search_tokens(session_id, uid, token)` populated with every 2-gram of `search_text`. Queries become `EXISTS (SELECT 1 FROM record_search_tokens t WHERE t.session_id = r.session_id AND t.uid = r.uid AND t.token = ?)`.

**Cost**: storage multiplier ~N (string length) rows per record. For 200-char search_text × 1M records = 200M rows — explodes DB size. Index lookups still cheap per query.

**Verdict**: skip — FTS5 trigram does the same thing with smaller storage and battle-tested merge logic. Only choose this if FTS5 trigram-specific behavior is unacceptable.

### 5. Region column denormalization (split `_region_norm` → `_province/_city/_county`)

**Already populated by importer.rs** (lines 253-258: `household_province`/`_city`/`_county` from row or `area.{city,county}` fallback). Already inserted into `records` (storage.rs:188-190 → cols 14-16) and `people` (single `household_region_norm` — but people also has its own region columns available; check if the people INSERT populates splits — currently `people` only stores `household_region_norm`, NOT the 3 splits; the 3 splits exist on `records` only).

**People side gap**: `people` table schema (storage.rs:676-693) has only `household_region_norm`, no `_province_norm`/`_city_norm`/`_county_norm`. If you want `=` matching on people too, you must add those 3 columns to the `people` schema (v4 migration), and the person-aggregation pipeline in storage.rs:229-265 must propagate them from `summary` (model.rs confirms `PersonSummary` has `household_province/city/county` — model.rs:124-126).

**Index needed**: `CREATE INDEX idx_records_household_split ON records(session_id, household_province_norm, household_city_norm, household_county_norm);` — and a `people` analogue.

**Query rewrite**: replace `household_region_norm LIKE '%A%'` (line 893, 983) with `household_province_norm LIKE ? ESCAPE '\\'` where `?` = `contains_pattern(value)` if substring is still wanted, or `?` = `pattern_with_prefix` for prefix search. Recommended: switch to prefix search (`LIKE 'x%'`) so the new B-tree index can use it (vector seek). Existing `idx_records_hotel_region` already follows this pattern with substring matching — but currently wasted because substring is non-sargable.

### 6. Covering / partial indexes to merge COUNT + paged SELECT

COUNT(`*`) at storage.rs:387 (people) and 433 (records) re-runs the entire filter on every page request. The paged SELECT then re-runs filter + ORDER BY.

**For people** (sort = `score DESC, total_records DESC, name ASC, person_key ASC`, already indexed by `idx_people_sort`): A covering index `idx_people_filter_sort ON people(session_id, level, alert_count, age, gender, score DESC, total_records DESC, name ASC, person_key ASC)` could let the planner seek directly into the sorted slice for the common filter subset. But the LIKE filter (search_text), even after the FTS5/trigram rewrite, is a `MATCH` predicate — not directly indexable in a multi-column B-tree. So the planner will still have to scan the FTS candidates then sort.

**For records** (sort = `check_in ASC, uid ASC`, paginated): existing `idx_records_check_in ON (session_id, check_in, uid)` is good but only useful if filter doesn't force a sort. The filter has OR-EXISTS subqueries — the planner typically chooses a full scan + filter + sort.

**Honest assessment**: covering indexes help ONLY AFTER the LIKE clauses become index-sargable. They are an accelerator, not the primary fix. Without fixing the LIKE clauses, adding more B-tree indexes won't change COUNT(*); the planner will still scan.

### 7. rusqlite bundled feature flags needed — **NONE**

Confirmed by inspection of `libsqlite3-sys 0.30.1/build.rs:120-141` (the bundled-amalgamation branch, gated by `cfg(feature = "bundled")`):

```text
.flag("-DSQLITE_ENABLE_FTS3")
.flag("-DSQLITE_ENABLE_FTS3_PARENTHESIS")
.flag("-DSQLITE_ENABLE_FTS5")          # <-- FTS5 is ON
.flag("-DSQLITE_ENABLE_JSON1")
.flag("-DSQLITE_ENABLE_RTREE")
.flag("-DSQLITE_ENABLE_STAT4")
```

This flag list is **unconditional** (no cargo feature gate) — it applies to every `libsqlite3-sys/bundled` build. Therefore:
- **No Cargo.toml change needed.**
- **No `rusqlite` feature upgrade needed** (no `fts5` feature exists on the rusqlite crate side; FTS5 SQL is invoked via plain `Connection::execute_batch`, which rusqlite already supports).
- `trigram` tokenizer ships with FTS5 — no separate enable.

ICU tokenizer is NOT available (would require linking ICU libs — not bundled). Skip ICU; trigram + B-tree is the path.

---

## Semantic preservation notes

### `household_region_norm LIKE '%x%'` (substring contains)
- Was already semantically weaker than what the user wants (province-level filter). The 3 separate split columns support both substring (`LIKE '%x%'` — non-sargable) and prefix (`LIKE 'x%'` — sargable on B-tree). Recommend **prefix** semantics: matches `浙江省` from `浙江`. Acceptable product behavior.

### `search_text LIKE '%x%'` (substring contains)
- Trigram MATCH preserves exact substring semantic for patterns ≥3 chars. **Different behavior for ≤2 chars** — falls back to scan (still correct, just slower). No semantic regression.

### `hotel_name_norm LIKE '%a%b%c%'` (ordered subsequence — `fuzzy_pattern` storage.rs:1068)
- **No indexable technique preserves ordered-subsequence exactly.** Trigram MATCH is substring-contains, not subsequence.
- **Retention strategy**: layer trigram as a **pre-filter** (fast narrowing), keep the existing `LIKE ? ESCAPE` as the **post-filter** for correctness. The combination is: trigram on the `M` characters of the query produces ~`M-2` trigrams; each trigram's doclist intersect narrows the rows to those containing all 3-character runs. The ordered-subseq LIKE then accepts a tiny subset. Result: same false-positive rate as today (zero — LIKE is the final judge), much smaller scan.
- For a 2-char query (e.g., `如家`), only one trigram exists — narrow happens but less aggressive.
- For a 1-char query (`京`), trigram can't help — falls back to LIKE scan of full column. Acceptable for hotel name (rare 1-char query).

### OR-of-LIKEs in `person_hotel_regions` (line 873)
- Was `(column LIKE ? OR region_norm LIKE ?)` — the OR defeated indexing; `region_norm` was a redundant concatenation. Switching to per-column `=`/prefix removes the OR and is sargable. Material numeric/string equality is strictly *cleaner* than substring contains; product fit is better.

### Migration impact on stored data
- v4 schema is allowed by spec ("clear old + re-import"). Bump `DATABASE_VERSION` to 4 (storage.rs:14).
- Update `initialize_schema` (storage.rs:623) — change `if version == 1 || version == 2 { reset... }` to `if version in {1,2,3} { reset... }`. v3 users must clear and re-import once.
- Existing `idx_records_household` (session_id, household_region_norm) and `idx_records_search` (session_id, search_text) become **dead code** — drop them; their seek cost is non-zero and they can never be used with leading `%`.

---

## Implementation verification protocol

### EXPLAIN QUERY PLAN queries to run after each rewrite

For each new path, run `EXPLAIN QUERY PLAN <sql>` and assert the plan shows:
- **`SEARCH ... USING INDEX`** or **`SEARCH ... USING COVERING INDEX`** (not `SCAN`).
- For FTS5 paths: the FTS5 virtual table access should appear as `SEARCH ... VIRTUAL TABLE INDEX` (the FTS5 shadow index).
- For the fuzzy_pattern hotel path: confirm `<hotel_name_norm_fts>` appears as a `SEARCH` and `hotel_name_norm LIKE ?` appears as a filter on a small candidate set.

Specifically compare plans before/after:
1. `SELECT COUNT(*) FROM records WHERE session_id = ? AND search_text LIKE '%hangzhou%' ESCAPE '\\'` (before, scan) vs. `... JOIN records_search_fts ON ... MATCH 'hangzhou' ...` (after, FTS5 search).
2. `SELECT COUNT(*) FROM records WHERE session_id = ? AND household_province_norm LIKE 'Zhe%'` (after, seek).
3. `SELECT COUNT(*) FROM records r WHERE session_id = ? AND EXISTS(SELECT 1 FROM person_hotel_regions phr WHERE phr.session_id = r.session_id AND phr.person_key = r.person_key AND phr.province_norm = ? AND phr.city_norm = ?)` (after, covering index seek on the EXISTS).

### Test cases for semantic preservation

1. **Region substring→prefix regression test**: Select a household region like `安徽省黄山市祁门县`, query `household_province="安徽"` → must match (prefix). Query `household_city="黄山"` → must match. Query `household_county="祁门"` → must match.
2. **search_text trigram floor regression**: Query with 1-char input (e.g., `浙`) — must still return correct rows (no false negatives; allowed to be slow). Query with 2-char input (e.g., `浙江`) — same. Query with 3-char input — must use FTS5.
3. **Ordered-subsequence**: hotel query `如家` should still match `如家酒店` (substring) and `如家精选酒店` (substring). Hotel query `如酒` should still match `如家酒店` (subsequence: `如` … `酒`). Run the existing test fixtures (storage.rs:1213, 1339, 1376, 1510) and confirm `fuzzy_pattern` results unchanged.
4. **Excluded region filter**: `NOT (... LIKE ...)` clauses (lines 907-916, 996-1006) must keep semantics: NOT over the new prefix-match set == NOT over the previous substring set, modulo the prefix-vs-substring difference. If you change substring→prefix, NEGATED rows where the user typed a middle-substring (e.g. excluding `"陵"` expecting to drop `铜陵市`) will no longer exclude unless the user types the province prefix. **This is a semantic change** — flag for product sign-off.
5. **Hotel region substring (records/people)**: rows where `hotel_province` stored as `浙江省` and user types `浙江` — prefix match. But user typing `江省` (substring of province, unthinkable for province filter) would no longer match — acceptable.
6. **Migration test**: take a v3 DB, run `initialize_schema`, assert `PRAGMA user_version == 4` and that all v3 tables are dropped and recreated.
7. **Index usage smoke test**: `EXPLAIN QUERY PLAN SELECT COUNT(*) FROM records WHERE session_id=? AND search_text MATCH 'hangzhou'` — must NOT show `SCAN records`.

### 453k-row benchmark protocol
- Use the existing import test fixture scaled to 453k records (or use real session export).
- Before refactor: time each of (a) `query_imported_records` page 1 with empty filter (sanity), (b) page 1 with `search_text` filter non-trivial (3+ char Chinese), (c) page 1 with `hotel_search` fuzzy 3-char, (d) page 1 with `household_province` filter, (e) corresponding `COUNT(*)`.
- After refactor: repeat same 5, expect (b) and (c) at least 10× faster, (d) becomes <10ms.
- Record numbers as benchmark baseline in `.trellis/tasks/07-22-optimize-import-filter-perf/` (separate file, not this research doc).
- Stretch: 1M row version — re-run (b), (c), (d); ensure no path exceeds 200ms.

---

## Open questions for main agent / user

1. **Prefix vs substring for region filters**: switching `household_province_norm LIKE '%A%'` → `LIKE 'A%'` (prefix) breaks the case where the user types a substring of the middle. For a Chinese province/city/county, this is almost certainly OK (users type the leading characters). But for the **excluded region** filters (lines 907-909, 998-999), the user might rely on substring today. Confirm that **prefix-only** semantics is acceptable for both `household` filter and `hotel_region` filter.
2. **Trigram 3-char floor**: For `search_text`, are there realistic 1-2 char Chinese queries? Most queries will be names (`张三`, 2-char) — these fall back to scan. Worth special-casing ≤2-char to also use prefix on `name_norm` (already indexed?). Decision needed: keep the slow path for short queries, or route them to a different column.
3. **FTS5 write cost during import**: trigram index roughly 3× the text length in index rows. For 453k records × 200-char `search_text`, that's ~91M trigram postings — adds ~50–150MB to the DB file. Acceptable for the local-analysis use case?
4. **Schema migration UX**: bumping to v4 will force users to clear + re-import their v3 sessions. Coordinate with the user-facing announce (app_meta flag or UI notice)?
5. **External content vs contentless for FTS5**: `records` uses composite PK `(session_id, uid)` with implicit rowid (no `WITHOUT ROWID`). Contentless FTS5 needs an integer rowid; using SQLite's implicit `rowid` is OK. Should we use contentless-delete (`content='' contentless_delete=1`, SQLite ≥3.43) for deletion support, or plain contentless with manual FTS5 `'delete'` command? Need to verify the bundled SQLite version.

---

## File references (internal)

| Symbol/Location | Description |
|---|---|
| `src-tauri/Cargo.toml:23` | `rusqlite = "0.32.1" features = ["bundled"]` — verified sufficient for FTS5 |
| `src-tauri/src/storage.rs:14` | `DATABASE_VERSION: i64 = 3` (bump to 4) |
| `src-tauri/src/storage.rs:145-196` | `records` INSERT — populates `household_province_norm/_city_norm/_county_norm` already (cols 14-16) but `household_region_norm` is the only one queried |
| `src-tauri/src/storage.rs:247-265` | `people` INSERT — only populates `household_region_norm`, NOT the 3 splits |
| `src-tauri/src/storage.rs:654-674` | `records` schema — has all split columns, no `WITHOUT ROWID` |
| `src-tauri/src/storage.rs:676-693` | `people` schema — only `household_region_norm`, no splits (must extend in v4) |
| `src-tauri/src/storage.rs:702-708` | `person_hotels` schema — already has `(session_id, person_key, hotel_name_norm)` PK |
| `src-tauri/src/storage.rs:709-718` | `person_hotel_regions` schema — has `province_norm`/`city_norm`/`county_norm`/`region_norm` already |
| `src-tauri/src/storage.rs:720-731` | Index list — `idx_records_household` and `idx_records_search` are dead (never seekable due to leading `%`) |
| `src-tauri/src/storage.rs:817-918` | `build_person_filter` — all slow clauses in people path |
| `src-tauri/src/storage.rs:920-1009` | `build_records_filter` — all slow clauses in records path |
| `src-tauri/src/storage.rs:1064-1086` | `contains_pattern` / `fuzzy_pattern` / `escape_like` helpers |
| `src-tauri/src/storage.rs:1213-1520` | test fixtures using `household_province`/`_city`/`_county` — useful as semantic-preservation tests |
| `src-tauri/src/importer.rs:20-22, 253-258, 622-624` | importer populates the 3 split region fields from CSV columns `户籍省`/`户籍市`/`户籍县区` |
| `src-tauri/src/model.rs:124-126, 246-251, 331-336` | `PersonSummary` / `PersonQuery` / `ImportedRecordsQuery` already carry `household_province/city/county` fields — UI is already wire-ready |

## External references (footnotes)

- SQLite FTS5 doc (tokenizers, trigram section, external content, contentless-delete): https://www.sqlite.org/fts5.html — key facts: trigram gives indexed LIKE/GLOB substring match for patterns ≥3 unicode chars; **LIKE optimization breaks if ESCAPE clause present**; external content requires single `content_rowid`; contentless-delete tables available 3.43+.
- SQLite compile options: https://www.sqlite.org/compile.html — `SQLITE_ENABLE_FTS5` is the FTS5 compile flag.
- rusqlite 0.32.1 feature manifest: https://crates.io/api/v1/crates/rusqlite/0.32.1 — no rusqlite-side `fts5` feature exists; FTS5 is purely a libsqlite3-sys build flag.
- libsqlite3-sys 0.30.1 `build.rs:120-141` (extracted from crates.io tarball) — confirms `bundled` unconditionally passes `-DSQLITE_ENABLE_FTS5`.
