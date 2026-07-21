# Quality Guidelines

> Code quality standards for frontend development.

---

## Overview

The analysis workspace is a dense desktop tool. Keep the primary path visible and move infrequent choices behind standard progressive-disclosure controls. UI simplification must preserve every business capability and must not duplicate scoring or filtering rules in React.

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
- Show a count on the secondary-filter trigger when non-default criteria are active.
- Keep analysis settings editable in `SettingsPanel`; the sidebar may show a read-only summary and a single entry point.
- Interactive controls require visible focus, hover, active, disabled, and loading feedback.

---

## Testing Requirements

- Run `npm test`, `npm run lint`, and `npm run build` for every frontend interaction change.
- Tests for progressive disclosure must assert that the trigger starts closed, opens on activation, and exposes the expected controls or actions.
- Existing workspace smoke tests must continue to assert that the table and person-detail entry points render.
- Result-filter tests must cover multi-hotel AND matching, same-stay jurisdiction matching, age validation, and the absence of result filters from `SettingsPanel`.

---

## Code Review Checklist

- Primary actions remain visible without opening a menu.
- Secondary controls remain keyboard reachable and have plain-language labels.
- Narrow-window rules keep controls usable without compressing data-table columns.
- Empty and no-result states tell the user what to do next.
- No Tauri API, DTO, or scoring behavior changed as part of a visual-only task.
