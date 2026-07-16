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
