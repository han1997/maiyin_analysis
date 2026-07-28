# Quality Guidelines

> Code quality standards for backend development.

---

## Overview

<!--
Document your project's quality standards here.

Questions to answer:
- What patterns are forbidden?
- What linting rules do you enforce?
- What are your testing requirements?
- What code review standards apply?
-->

(To be filled by the team)

---

## Forbidden Patterns

<!-- Patterns that should never be used and why -->

(To be filled by the team)

---

## Required Patterns

<!-- Patterns that must always be used -->

(To be filled by the team)

---

## Testing Requirements

### Legacy spreadsheet compatibility

The import layer must treat `.xls` as BIFF compatibility work, not merely as
another extension handled by the primary workbook reader.

- Keep Calamine as the primary reader for normal `.xls`/`.xlsx` files.
- If Calamine opens an `.xls` but returns no non-empty worksheet cells, retry
  with the bounded BIFF reader before returning `AppError::Empty`.
- Convert only non-empty cells into rows. Do not allocate from the workbook's
  declared formatted range because legacy exporters may declare tens of
  thousands of styled empty rows.
- Feed fallback rows into the same header detection, inference, validation,
  deduplication, and stay-duration logic as every other format.

Error contract:

| Condition | Result |
|---|---|
| Primary reader returns usable rows | Use primary rows; do not invoke fallback |
| Primary reader is empty and BIFF fallback returns rows | Continue normal import |
| Both readers contain no data | `AppError::Empty("<file> 中没有可读取的数据工作表")` |
| BIFF fallback cannot parse the workbook | `AppError::Parse` with the source filename |

Required tests:

- Unit-test sparse cell-to-row reconstruction and assert formatted empty tails
  do not appear as data rows.
- For a reported compatibility bug, run a local integration check against the
  untouched source workbook and assert its headers/data rows are recovered.

Wrong: require users to rename or resave an otherwise readable source file.

Correct: isolate the compatibility fallback inside `read_workbook` and keep
all downstream business rules format-independent.

### Centralized sheet selection

Per-sheet scoring/picking (template detection → header id_no/check_in match
→ core-field inference → best-score fallback, with early return and per-sheet
error short-circuit) is centralized in `score_and_pick_sheet(sheets: impl
Iterator<Item = Result<Vec<Vec<String>>, AppError>>) -> Result<Option<Vec<Vec<String>>>, AppError>`.
`read_workbook` (Calamine) and `read_legacy_xls` (rxls) each build a lazy
`Result<rows, AppError>` iterator from their sheet source and delegate; the
BIFF fallback stays isolated in `read_workbook`. Changes to sheet selection
go in `score_and_pick_sheet` only — do not re-duplicate the loop per reader.

### Test-gated timing helpers

Test-only timing/telemetry inside production functions must be externalized
into a zero-production-cost helper rather than inlined as `#[cfg(test)]`
blocks. The pattern: define a small struct whose fields are all
`#[cfg(test)]`-gated (zero-sized in release), with a `mark(label)` method
that is a no-op outside `cfg(test)` and emits the observed metric inside it.
`SessionStore::save` uses `SaveTimer::start()` + `timer.mark("<stage>")` for
its `MAIYIN_SAVE_TIMINGS` stage timing; the helper owns the env gate and the
`save_stage={} elapsed_ms={}` stderr format, and `save`'s body stays free of
`#[cfg(test)]` annotations so the production control flow reads cleanly. New
test-gated telemetry in hot production paths should follow the same shape.

### Threshold-switched hybrid for dense overlap detection

`analyze_person` overlap detection switches between an exact O(n²) path and
a fast sweep-line path based on per-day record count (`DENSE_OVERLAP_THRESHOLD
= 32`). Days with `end - start ≤ 32` stay on the original nested loop with
`break` early-exit and exact `add_pair` calls — evidence output is bit-for-bit
unchanged. Days exceeding the threshold dispatch to `detect_dense_day_overlaps`,
which produces the same four evidence fields (`pair_count`,
`different_place_count`, `pair_labels`, `evidence_ids`) via:

- **pair_count**: sweep-line with `BTreeMap<NaiveDateTime, Vec<usize>>` keyed by
  `effective_end`. Expire intervals via `.range(..=check_in)` before counting;
  accumulate `active_count` as each second record enters (count first, then
  insert — the current record never counts itself).
- **different_place_count**: accommodation grouping via
  `HashMap<(String, String), usize>` (key = `(compact(hotel_name),
  compact(room_no))`). `same_place = active same-group count`;
  `different = active_count - same_place`. This is **exact when hotel/room are
  populated**; empty fields are grouped as a distinct key, slightly
  overestimating `different_place_count`. The overestimate is conservative
  (severity leans 高) and only affects >32 records/day pathological paths —
  acceptable per the task ADR; ≤32 days use the exact path.
- **pair_labels**: bounded nested loop over the first
  `min(DENSE_OVERLAP_THRESHOLD, day_len)` second records, collecting ≤4 labels
  then breaking. O(threshold²) = O(1024), constant.
- **evidence_ids**: all-overlap shortcut — if the day's first record
  `effective_end > last second.check_in`, every record participates → emit all
  uids in `0..end` (O(n)). Otherwise, track an `involved: HashSet<u64>` during
  the sweep (second + all overlapping active firsts).

The original O(n²) loop skips dense days via `dense_day_set.contains(...)` to
avoid double-counting. Overlap ownership stays on `days[day_index].overlap`
(second record's check_in date), matching existing semantics. The
`overlap_score` formula (`min(35, 20 + P*2 + D*5)`) and all DTOs are
unchanged. Benchmark `benchmark_dense_overlap_analysis` asserts
`evidence_count == record_count` and `overlap_days == 1` — both paths must
keep these green.

Generalize this pattern when an O(n²) hot path must also produce rich
evidence (not just counts): keep the exact path for normal inputs, add a
threshold-switched fast path for pathological inputs, and document which
evidence fields are approximated and why the approximation is safe
(conservative direction, capped score, or untested edge case).

### Tauri 2 Channel-based progress reporting

Long-running Tauri commands (`import_paths`, `import_folders`, `reanalyze`,
`merge_sessions`) stream phase/percent updates to the WebView via
`tauri::ipc::Channel<ProgressPayload>` injected as a command parameter. The
pattern keeps the domain layer Tauri-agnostic and the throttle in the command
layer:

- **Domain layer stays Tauri-agnostic**: `analyze_records` and
  `importer::import_paths` accept
  `on_progress: Option<&(dyn Fn(usize, usize) + Send + Sync)>` (raw
  `(current, total)` callback, no Tauri types). The callback is `&dyn Fn` (not
  `FnMut`) so it can be captured by reference inside Rayon `par_iter` closures.
  When `None`, the `if let Some(f) = on_progress { f(...) }` guard is zero-cost.
  Inside the parallel loop, an `AtomicUsize` counter increments via
  `fetch_add(1, Ordering::Relaxed)` and calls the callback; the FIRST call
  passes `(0, total)` so the channel learns the real total before items start.
- **Command layer owns Tauri specifics + throttle**: `commands.rs` defines the
  serializable `ProgressPayload { phase, current, total, label }` (serde
  camelCase) and a `make_progress_callback(channel, phase, total, template)`
  helper returning `Arc<dyn Fn(usize, usize) + Send + Sync>`. The closure
  captures an `Instant`/`AtomicU64` last-emit timestamp; it emits when
  `elapsed > 50ms` OR `current == total` OR first call (`last == 0`). This
  caps IPC at ~20 events/sec regardless of Rayon throughput.
- **Validation before emit**: `validate_settings` (reanalyze) and
  `session_ids.len() < 2` (merge) run BEFORE any `channel.send(...)`. Error
  paths produce no progress events — the frontend never sees a progress bar
  for a request that will immediately fail validation.
- **Phase sequence**: each command emits a start `{current: 0, total, label}`
  per phase, intermediate throttled updates, then an end
  `{current: total, total, label}`. `total = 0` marks an indeterminate phase
  (e.g. "saving") — the frontend shows the label with an indeterminate bar.
- **Frontend companion state**: `progress: useState<Progress | null>(null)` is
  a companion to the existing `busy` action enum. `busy` remains the single
  loading-state coordinator (gates button disabling); `progress` only feeds
  the progress UI. Both are cleared together in the `runSnapshotAction`
  `finally` block. Determinate bar renders when `total > 0`; indeterminate
  fallback when `total === 0` or `progress === null`.

Generalize this pattern when a Tauri command wraps a long `spawn_blocking`:
inject a `Channel<T>` parameter, keep the domain callback Tauri-agnostic,
throttle in the command layer, and emit validation-before-progress so error
paths stay silent.


---

## Code Review Checklist

<!-- What reviewers should check -->

(To be filled by the team)
