# Error Handling

> How errors are handled in this project.

---

## Overview

<!--
Document your project's error handling conventions here.

Questions to answer:
- What error types do you define?
- How are errors propagated?
- How are errors logged?
- How are errors returned to clients?
-->

(To be filled by the team)

---

## Error Types

<!-- Custom error classes/types -->

(To be filled by the team)

---

## Error Handling Patterns

<!-- Try-catch patterns, error propagation -->

### Startup entry point

The Tauri entry point (`lib.rs::run()`) must not panic on startup failure.
Replace the conventional `.run(...).expect(...)` with structured error
handling that logs the full `tauri::Error` Display to stderr and exits with a
nonzero code:

```rust
.run(tauri::generate_context!())
    .unwrap_or_else(|error| {
        eprintln!("failed to run maiyin analysis: {error}");
        std::process::exit(1);
    });
```

Rationale: a raw `.expect()` triggers panic unwinding, which on Windows can
surface as the OS-level "application has stopped working" dialog with no
actionable detail. `eprintln!` + `exit(1)` gives developers a complete error
chain when launching from a terminal and exits cleanly otherwise. GUI
dialog feedback at this point is infeasible without an `AppHandle` (the event
loop has not started), so stderr is the pragmatic channel. Keep `run()` as
`pub fn run()` (no return type) and preserve the `mobile_entry_point`
attribute.

---

## API Error Responses

<!-- Standard error response format -->

(To be filled by the team)

---

## Common Mistakes

<!-- Error handling mistakes your team has made -->

(To be filled by the team)
