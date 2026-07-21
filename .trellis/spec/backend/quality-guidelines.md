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


---

## Code Review Checklist

<!-- What reviewers should check -->

(To be filled by the team)
