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


---

## Code Review Checklist

<!-- What reviewers should check -->

(To be filled by the team)
