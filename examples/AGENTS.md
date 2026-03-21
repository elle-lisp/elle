# Examples — Agent Guide

## Role

Every `.lisp` file in this directory is an integration test. CI runs each
one with a 10-second timeout. A failure here means the full compilation
pipeline (reader → expander → analyzer → lowerer → emitter → VM) produced
wrong results or panicked.

Read [`QUICKSTART.md`](../QUICKSTART.md) for the complete language reference.

## Style rules

Follow these when writing or editing examples:

- Start every file with `(elle/epoch N)` where N is the current epoch (check
  `CURRENT_EPOCH` in `src/epoch/rules.rs`).
- Use `(assert expr msg)` for all assertions. No assertion library imports.
  ```lisp
  (elle/epoch 1)
  (assert (= actual expected) "description")
  (assert (not (nil? val)) "value exists")
  ```
- `defn` with `[bracket params]` and a docstring as the first body form.
- Literal syntax: `@[...]` @arrays, `[...]` arrays, `{...}` structs,
   `@{...}` @structs, `@"..."` @strings.
- `#` for comments, `true`/`false` for booleans (not `#t`/`#f`).
- `empty?` to test end-of-list (not `nil?`, not `(= (length x) 0)`).
- `case`/`cond` over nested `if`. `when`/`unless` for one-armed conditionals.
- `each x in coll` for iteration. `->` / `->>` for pipelines.
- `try`/`catch`/`protect`/`defer` for error handling.
- `&opt`/`&keys`/`&named` where appropriate.
- No `(println "=== Section ===")` headers. Use `(print ...)` (no newline) /
  `(println ...)` (with newline) to show computed values — the program should
  visibly *do things* when run. 2-5 output lines per section showing
  interesting results. For stderr, use `(eprint ...)` / `(eprintln ...)`.
- Each file starts with a header comment listing what it demonstrates.
- Each file should be a cohesive "application" or themed demonstration,
  not a bag of unrelated unit tests.

## Files

All example files follow the style rules above. See `README.md` for the
complete file inventory with themes and coverage.

## Assertions

Use the built-in `(assert expr msg)` primitive directly:

```lisp
(assert (= (+ 1 2) 3) "addition works")
(assert (not ok?) "expected error")
```

For testing that an expression signals an error, use `protect`:

```lisp
(def [ok? _] (protect (/ 1 0)))
(assert (not ok?) "division by zero errors")
```

## Gotchas

- **`nil` vs empty list**: `(list)` returns `EMPTY_LIST`, which is truthy.
   `nil` is falsy. `nil?` only matches `nil`. Use `empty?` for end-of-list.
- **`string/join` accepts any sequence** (list, array, or @array).
- **`string/split` returns an array**.
- **`[...]` in `match`** matches arrays (not @arrays). `@[...]` matches @arrays.
- **`put` on immutable types** returns a new copy. On mutable types it
   returns the same mutated object.
- **String iteration** is grapheme-cluster based. `(length "👋🏽")` is 1.
