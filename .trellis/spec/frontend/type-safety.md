# Type Safety

> Type safety patterns in this project.

---

## Overview

<!--
Document your project's type safety conventions here.

Questions to answer:
- What type system do you use?
- How are types organized?
- What validation library do you use?
- How do you handle type inference?
-->

## Current convention

TypeScript strict mode is enabled with `noUncheckedIndexedAccess`. DTOs consumed by the UI live in `src/domain/types.ts` and use camelCase names matching Rust serde output. The `AppApi` interface in `src/api/contract.ts` is the only boundary used by React components.

## Validation

Native command errors are treated as unknown values and narrowed to their structured `message` field before display. Components do not cast command payloads to unrelated shapes.

### Cross-layer settings validation

When a frontend pre-check duplicates a backend validation rule (e.g. `applySettings`
pre-validating `AnalysisSettings` before calling `reanalyze`), the backend is the
source of truth and the frontend must align to it, not the other way around.

- Extract the frontend mirror into `src/domain/validation.ts` so the rule lives in
  one place on the WebView side.
- Export the shared constants (`THRESHOLD_MIN`, `THRESHOLD_MAX`, `THRESHOLD_LABELS`)
  alongside a `validateAnalysisSettings(settings): string | null` helper; do not
  hardcode the bounds or labels at the call site.
- Error messages must be byte-identical to the backend `validate_settings` output
  (including label interpolation, range suffix, and no trailing period where the
  backend omits it). The UI shows the same string whether the toast fires locally
  or the `CommandError` arrives from Rust.
- Annotate the file with `// 须与 src-tauri/src/commands.rs::validate_settings 同步`
  and keep a unit test (`validation.test.ts`) that pins both the rule semantics and
  the message format, so backend drift surfaces as a failing frontend test.
- The backend validation remains the final guard; the frontend check is an
  early-feedback toast only and does not replace the Rust call.

## Forbidden patterns

- Do not duplicate Rust risk rules in React or browser fixtures.
- Do not use `any` to bypass a DTO mismatch.
- Do not format or reinterpret date strings as a business decision in a component.

---

## Type Organization

<!-- Where types are defined, shared types vs local types -->

(To be filled by the team)

---

## Validation

<!-- Runtime validation patterns (Zod, Yup, io-ts, etc.) -->

(To be filled by the team)

---

## Common Patterns

<!-- Type utilities, generics, type guards -->

(To be filled by the team)

---

## Forbidden Patterns

<!-- any, type assertions, etc. -->

(To be filled by the team)
