# Upstream parity research

## Repository relationship

- Local `main` is React + Tauri/Rust and is 10 commits ahead of the common history.
- `origin/main` is a Python/Tkinter desktop application and has 55 commits not present locally.
- A Git merge would replace the current application. Required strategy is behavioral porting, not source merging.

## Latest upstream behavior to port

### Scoring and analysis scope

- Analysis time boundary is based on check-in time. Records outside it are excluded from totals, alerts, evidence, detail stays, and exports.
- Selected-window frequency and rolling frequency are mutually exclusive.
- Settings defaults: selected-window threshold `3`, rolling 7-day `3`, 30-day `12`, 365-day `144`.
- Overlap uses half-open intervals and substitutes check-in + 1 day when checkout is missing/invalid.
- Same-hotel/same-room overlap still alerts. Different hotel or room increases its score.
- Overlap score: `min(35, 20 + pair_count * 2 + different_place_count * 5)`.
- Same-day non-overlap count `N >= 4`: `min(45, 25 + (N - 4) * 5)`.
- Selected or rolling frequency count `C > T`: `min(80, 45 + (C - T) * 6)`.
- Person total is capped at 100; levels remain 80/55/30.
- Evidence ids include every contributing record in deterministic de-duplicated order.

### UI and workflow

- Analysis parameters are directly visible, split into selected check-in boundaries and 7/30/365-day thresholds.
- Editing parameters does not reanalyze until the user applies them.
- Result filters are explicitly applied, avoiding expensive recalculation/filtering on every keystroke.
- Hotel-name filtering supports fuzzy matching.
- Imported stay records have a dedicated tab.
- Result cells expose full values through hover tooltips.
- Date-time inputs have stable focus behavior and calendar/time selection.

### Performance

- Person records are scoped and sorted once.
- Normalized hotel/room strings are cached during overlap pair counting.
- Rolling-window counts use an ordered sliding-window path.
- Optimizations must preserve alert text, scores, evidence order, summary fields, exports, and history compatibility.

## Local gaps

- Rust currently supports only 30/365-day thresholds with defaults 6/24.
- Rust overlap requires different hotel/room and uses the older 45-60 point formula.
- Rust month/year frequency formulas differ from upstream.
- Local settings are modal and lack selected check-in boundaries and 7-day frequency.
- TypeScript alert kinds and result fields lack window/7-day frequency variants.
- Local UI lacks the upstream imported-record tab and explicit filter application workflow.

## Recommended approach

Port upstream contracts into the existing Tauri architecture:

1. Update shared Rust/TypeScript settings, result types, persistence normalization, export fields, and validation.
2. Port scoring rules and performance-safe helpers with parity tests based on upstream assertions.
3. Adapt the existing React UI to expose the parameters and workflows while retaining the current product design system.
4. Add regression tests at Rust domain and TypeScript filter/UI boundaries.

