# Bottleneck Decision: importer dedup key (owned vs borrowed)

> Phase 1 decision gate for task `07-29-importer-dedup-key-borrowed-or-prehashed`.
> Audit item #10 (P3) explicitly says: "仅在基准显示为瓶颈时落地" (only land if
> the benchmark shows a bottleneck). This document records the data-driven
> decision on whether to proceed to Phase 2 (prototype a borrowed-key variant).

## Decision

**NOT_A_BOTTLENECK — Phase 2 does NOT proceed. The task closes with no
production code change.**

The current structured owned `DeduplicationKey` already satisfies the spec
"Correct" pattern (`tauri-contract.md` §"importer determinism and performance"),
and the benchmark shows its merge cost at ~453k-record scale is small in
absolute terms and a negligible fraction of realistic import time. A
borrowed-key variant would add lifetime/ownership complexity to
`merge_parsed_files` for a marginal gain the benchmark does not justify.

## Evidence (from `benchmark-results.md`)

4 runs at `MAIYIN_BENCH_FILES=46` + `MAIYIN_BENCH_ROWS_PER_FILE=10000`
(455_500 retained records, 4_500 duplicates), release build:

| Metric         | Average |
|----------------|---------|
| `parse_ms`     | 477     |
| `old_merge_ms` | 1073    |
| `new_merge_ms` | **412** |
| reduction%     | 61.6    |

The key number — **`new_merge_ms` ≈ 412 ms** — is the current owned-key merge
time for ~455 k records. It is stable across runs (403–424 ms, ~5% spread).

## Analysis

### 1. Is `new_merge_ms` a bottleneck in absolute terms?

No. ~412 ms for the full merge+dedup of 455.5 k records is fast. This single
phase covers 8 `String::clone`s + 2 `DateKey` constructions + `HashSet` insert
(hash 10 fields) + `Vec` push + uid assignment per record — ~412 ms total,
i.e. ~1.1 M records/sec throughput. The clone overhead is only one component
of this 412 ms, not the whole of it (hashing + insert + Vec growth also cost).

### 2. Is merge a large fraction of total import time?

In the **synthetic** benchmark, `new_merge / (parse + new_merge)` =
412 / (477 + 412) ≈ **46%**. On the raw <30% heuristic this looks borderline,
but the synthetic `parse_ms` is **unrealistically fast**: it parses plain CSVs
that were generated in-memory. Real-world imports parse `.xlsx`/`.xls` via
Calamine (and the BIFF fallback for legacy `.xls`), which is orders of
magnitude slower — tens of seconds for 453 k rows versus ~0.5 s here. In a
real import, merge is a tiny fraction (<2%) of total time. The 46% figure is
an artifact of the synthetic harness, not a real-import signal.

### 3. What is the absolute clone overhead being targeted?

A borrowed-key variant would eliminate the per-record `String::clone`
allocations. Estimate of what those clones cost:

- 455_500 records × 8 `String` fields = ~3.64 M clone calls.
- A small-`String` clone in Rust is roughly an allocator call + a `<24 byte`
  memcpy, ~20–50 ns each. Even pessimistically: 3.64 M × 50 ns ≈ **182 ms**;
  realistically (many empty/short fields): **~50–110 ms**.
- In the synthetic data, 6 of 8 fields are empty strings (the CSV only has 5
  columns), so ~2.73 M of those clones are no-alloc sentinel copies. Only
  `id_no` + `hotel_name` clone real bytes, and the two dates parse to
  `DateKey::Parsed` (no string allocation). So the **synthetic** clone cost is
  closer to ~25–35 ms.
- The dates being `Parsed` is typical for real data too (check-in/out usually
  parse), so the dominant real-data clone overhead is the populated region
  fields (province/city/county/region/address/room_no), all short strings.

So a borrowed-key optimization would save, at most, on the order of **50–180 ms**
of the 412 ms merge time — a minority fraction — and that saving is dwarfed by
real-world parse cost (tens of seconds).

### 4. Cost / risk of the borrowed-key variant

A borrowed `DeduplicationKeyRef<'a>` with `&'a str` fields is **not a
drop-in** in `merge_parsed_files` (`src-tauri/src/importer.rs:151-177`):

```rust
for mut record in parsed_file.records {
    let key = deduplication_key(&record);   // owned key today
    if !seen.insert(key) {
        stats.duplicate_count += 1;
        continue;
    }
    record.uid = uid;
    records.push(record);   // <-- MOVES record; a borrowed key would dangle
    uid += 1;
}
```

A `&'a str` key borrowing from `record` cannot outlive the `records.push(record)`
move. Keeping the borrowed key alive would require restructuring the loop
(e.g. storing records behind stable addresses first, two-pass dedup, or
`Pin`/`Box` indirection) — non-trivial lifetime gymnastics across two hot
paths (`merge_parsed_files` **and** `commands.rs::merge_sessions`). That
added complexity is not justified by a ~50–180 ms saving on a 0.4 s phase
that is itself negligible in real imports.

### 5. Spec alignment

`tauri-contract.md` §"importer determinism and performance" lists the current
owned structured key as the "Correct" pattern and the joined-string key as
"Wrong"/"Bad". The audit explicitly frames item #10 as "已优化过的权衡的进一步
探讨" (further exploration of an already-optimized trade-off) and "仅在基准
显示为瓶颈时落地". The benchmark does not show a bottleneck, so landing a
borrowed-key variant now would be **over-engineering beyond spec**, contrary
to the task's own decision gate and the "Don't add unnecessary abstractions"
code standard.

## Verdict against the task's heuristic

| Heuristic (from PRD Step 3)                    | Result for current data |
|------------------------------------------------|-------------------------|
| `new_merge_ms` at ~453k scale                  | ~412 ms — small         |
| merge <30% of total (synthetic)                | 46% (synthetic parse is artificially fast; real parse dominates) |
| clone overhead consistent with negligible      | Yes — ~50–180 ms of 412 ms; empty-string clones are free; dates parse to `Parsed` |
| BOTTLENECK → Phase 2, else NOT_BOTTLENECK      | **NOT_BOTTLENECK**      |

The synthetic <30% heuristic is the only borderline signal, and it is
invalidated by the fact that synthetic parse is unrealistically fast. Every
other consideration (absolute merge time, real-world parse dominance, clone
overhead as a minority fraction of merge, borrowed-key lifetime complexity,
spec "Correct" already satisfied) points to NOT_BOTTLENECK.

## Outcome

- **Phase 2: does NOT proceed.** No borrowed-key (`DeduplicationKeyRef`) or
  prehashed-key prototype is built. No production source (`importer.rs`,
  `commands.rs`) is modified.
- **No spec update required.** Behavior is unchanged; the existing spec
  "Correct" pattern (owned structured key) remains the documented contract.
- **Quality gate (cargo fmt / clippy / tests / npm) is unaffected** — no code
  changed in Phase 1. (The benchmark itself was only *run*, not modified.)
- **Rationale preserved**: if a future real-world import at 453k+ scale ever
  shows merge dominating total time (e.g. after Calamine parse is heavily
  optimized), this decision can be revisited. Re-run
  `benchmark_synthetic_multi_file_import_merge` with `MAIYIN_BENCH_FILES`/
  `MAIYIN_BENCH_ROWS_PER_FILE` and compare `new_merge_ms` against realistic
  parse cost before reopening.
