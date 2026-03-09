# Examples — Agent Guide

## Role

Every `.lisp` file in this directory is an integration test. CI runs each
one with a 10-second timeout. A failure here means the full compilation
pipeline (reader → expander → analyzer → lowerer → emitter → VM) produced
wrong results or panicked.

## Style rules

Follow these when writing or editing examples:

- Import assertions at the top using the closure import pattern, destructuring
  only the names the file uses. Never define assertions inline.
  ```lisp
  (def {:assert-eq assert-eq :assert-true assert-true} ((import-file "./examples/assertions.lisp")))
  ```
- `defn` with `[bracket params]` and a docstring as the first body form.
- Literal syntax: `@[...]` arrays, `[...]` tuples, `{...}` structs,
  `@{...}` tables, `@"..."` buffers.
- `#` for comments, `true`/`false` for booleans (not `#t`/`#f`).
- `empty?` to test end-of-list (not `nil?`, not `(= (length x) 0)`).
- `case`/`cond` over nested `if`. `when`/`unless` for one-armed conditionals.
- `each x in coll` for iteration. `->` / `->>` for pipelines.
- `try`/`catch`/`protect`/`defer` for error handling.
- `&opt`/`&keys`/`&named` where appropriate.
- No `(print "=== Section ===")` headers. Use `(display ...)` / `(print ...)`
  to show computed values — the program should visibly *do things* when run.
  2-5 display lines per section showing interesting results.
- Each file starts with a header comment listing what it demonstrates.
- Each file should be a cohesive "application" or themed demonstration,
  not a bag of unrelated unit tests.

## Files

All example files follow the style rules above. See `README.md` for the
complete file inventory with themes and coverage.

## Assertions

`assertions.lisp` provides: `assert-eq`, `assert-true`, `assert-false`,
`assert-list-eq`, `assert-not-nil`, `assert-string-eq`. All print
expected-vs-actual on failure and `(exit 1)`.

`assert-eq` uses `=` (numeric-aware equality) for all comparisons. For list
comparison use `assert-list-eq` (element-wise, handles length mismatch).
For strict identity checks (no numeric coercion), use `identical?`.

## Gotchas

- **`nil` vs empty list**: `(list)` returns `EMPTY_LIST`, which is truthy.
  `nil` is falsy. `nil?` only matches `nil`. Use `empty?` for end-of-list.
- **`string/join` accepts any sequence** (list, tuple, or array).
- **`string/split` returns a tuple**.
- **`[...]` in `match`** matches tuples (not arrays). `@[...]` matches arrays.
- **`put` on immutable types** returns a new copy. On mutable types it
  returns the same mutated object.
- **String iteration** is grapheme-cluster based. `(length "👋🏽")` is 1.
