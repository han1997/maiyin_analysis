# Quality Guidelines

> Code quality standards for frontend development.

---

## Overview

The analysis workspace is a dense desktop tool. Keep the primary path visible and move infrequent choices behind standard progressive-disclosure controls. UI simplification must preserve every business capability; production React must not own scoring or full-collection filtering.

---

## Forbidden Patterns

- Do not expose the same editable analysis setting in both the sidebar and the settings panel.
- Do not render one persistent toolbar button per export format. Use one labelled export entry point and list formats inside it.
- Do not hide risk level text or rely on color alone.
- Do not introduce custom modal flows when an existing inline region or right-side inspector can support the task.

---

## Required Patterns

- Keep person search and the primary risk-level filter directly visible above the results table.
- Put hotel search, alert-state filtering, and other secondary criteria in a labelled `details` disclosure or equivalent accessible control.
- Treat hotel jurisdiction, household include/exclude, age, and gender as result filters. They belong to `PersonQuery`, not `AnalysisSettings`, and must never trigger risk reanalysis.
- Multiple hotel-name terms use a familiar separated text input; explain the AND behavior beside the field instead of introducing a custom selector.
- Applied result filters call `AppApi.queryPeople` and replace one `PersonPage`; they never require a full people collection in `WorkspaceSnapshot`.
- Show a count on the secondary-filter trigger when non-default criteria are active.
- Keep analysis settings editable in `SettingsPanel`; the sidebar may show a read-only summary and a single entry point.
- Interactive controls require visible focus, hover, active, disabled, and loading feedback.
- Analysis-mode choices use native radio semantics. The inactive parameter group remains
  visible for comparison but is disabled and visually de-emphasized.
- Data-table column widths use table-specific semantic classes. Do not use global
  `th:nth-child(...)` rules because column additions silently shift unrelated widths.
- The person-detail inspector offers a maximize/restore toggle that widens the panel over the
  main region; `Escape` while maximized exits maximize without closing the panel, and closing
  the panel resets maximize. A maximize button uses `aria-pressed` and a `data-maximized`
  state on the inspector.
- Clicking an alert in the detail inspector filters the evidence list to that alert's
  `evidenceIds` (matched against `EvidenceRecord.uid`) purely in React — it never calls
  `AppApi.getPersonDetail` again. A "全部证据" control clears the filter; an alert whose
  `evidenceIds` is empty shows an explicit empty-evidence message instead of a silent empty
  list. The selected alert is reset when the inspected person changes.
- Secondary toolbar popovers (filter, export) anchor so their right edge never exceeds the
  viewport; the filter popover is right-anchored to its trigger on desktop and left-anchored
  where the toolbar wraps on narrow windows, preserving internal vertical scroll.

---

## Testing Requirements

- Run `npm test`, `npm run lint`, and `npm run build` for every frontend interaction change.
- Tests for progressive disclosure must assert that the trigger starts closed, opens on activation, and exposes the expected controls or actions.
- Toolbar disclosure tests also cover outside-pointer close, `Escape`, mutual exclusion,
  and `aria-expanded` state.
- Existing workspace smoke tests must continue to assert that the table and person-detail entry points render.
- SQLite query tests cover multi-hotel AND matching, same-stay jurisdiction matching, and person filters. Frontend tests cover age validation, asynchronous page rendering, and the absence of result filters from `SettingsPanel`.

---

## Code Review Checklist

- Primary actions remain visible without opening a menu.
- Secondary controls remain keyboard reachable and have plain-language labels.
- Narrow-window rules keep controls usable without compressing data-table columns.
- Empty and no-result states tell the user what to do next.
- No Tauri API, DTO, or scoring behavior changed as part of a visual-only task.
