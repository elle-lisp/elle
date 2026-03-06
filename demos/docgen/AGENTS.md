# elle-doc

Documentation site generator: Elle program that generates static HTML documentation.

## Responsibility

Generate the Elle documentation site by:
1. Reading JSON input files (primitive metadata, stdlib docs)
2. Generating static HTML pages
3. Applying CSS styling
4. Organizing documentation by category

Does NOT:
- Parse Elle source (that's the main compiler)
- Execute Elle code (that's the VM)
- Manage the build system (that's Cargo)

## Key files

| File | Purpose |
|------|---------|
| `generate.lisp` | Main generator script (741 lines) |
| `lib/` | Library modules for generation |
| `docs/` | Generated documentation output |

## Architecture

The generator is written in Elle and exercises the runtime:

1. **HTML generation utilities** — Escape HTML, format markdown, apply formatting
2. **CSS stylesheet generation** — Generate inline CSS for styling
3. **Documentation parsing** — Read JSON input files
4. **Page generation** — Generate HTML pages for each category
5. **Site assembly** — Combine pages into a complete site

## Important invariants

### List termination

**Critical**: Lists terminate with `EMPTY_LIST`, not `NIL`.

- `nil?` only matches `Value::NIL` (absence)
- `empty?` matches `Value::EMPTY_LIST` (end of list)
- `(rest (list 1))` returns `EMPTY_LIST`, not `NIL`

This distinction is essential for recursive list functions:

```janet
(def sum-list (fn (lst)
  (if (empty? lst)  ;; Check for EMPTY_LIST, not nil
    0
    (+ (first lst) (sum-list (rest lst))))))
```

Using `nil?` instead of `empty?` causes infinite loops because `(rest (list 1))` returns `EMPTY_LIST` (truthy), not `NIL` (falsy).

### String operations

- `string-split` — Split string by delimiter
- `string-replace` — Replace substring
- `substring` — Extract substring by position
- `length` — Get string length

### Collection operations

- `first` — Get first element of list/array
- `rest` — Get rest of list/array (returns `EMPTY_LIST` for exhausted lists)
- `append` — Concatenate strings or lists
- `fold` — Reduce over collection

### Conditional forms

- `if` — Conditional expression
- `when` — Conditional without else
- `unless` — Conditional with negated test

### Functional forms

- `fn` — Define anonymous function
- `def` — Define global variable
- `var` — Define mutable variable
- `map` — Apply function to each element
- `filter` — Keep elements matching predicate
- `fold` — Reduce over collection

## Running the generator

The generator is run during CI as part of the docs job:

```bash
cargo build --release
./target/release/elle elle-doc/generate.lisp
```

This generates the documentation site in `elle-doc/docs/`.

## Common issues

### Infinite loops

If the generator hangs, check for:
- Using `nil?` instead of `empty?` for list termination
- Recursive functions that don't check for `EMPTY_LIST`
- Infinite recursion in helper functions

### Missing output

If the generator produces no output, check for:
- Missing input JSON files
- Incorrect file paths
- Errors in HTML generation functions

### Malformed HTML

If the generated HTML is malformed, check for:
- HTML escaping issues (use `html-escape`)
- Unclosed tags
- Incorrect string concatenation

## Files

| File | Lines | Content |
|------|-------|---------|
| `generate.lisp` | ~740 | Main generator script |
| `lib/` | — | Library modules (if any) |
| `docs/` | — | Generated documentation output |

## Invariants

1. **Lists terminate with `EMPTY_LIST`.** Use `empty?` to check for end-of-list, not `nil?`.

2. **String operations are UTF-8 safe.** Use `string-split` and `substring` for UTF-8 boundary safety.

3. **HTML must be escaped.** Use `html-escape` to prevent injection attacks.

4. **CSS is inline.** Styles are generated as inline CSS, not external stylesheets.

5. **Generator is deterministic.** Same input always produces same output.

## When to modify

- **Adding new documentation categories**: Update `generate.lisp` to handle new JSON input files
- **Changing HTML structure**: Update the page generation functions
- **Changing CSS styling**: Update `generate-css`
- **Adding new formatting**: Update the markdown formatting functions

## Common pitfalls

- **Using `nil?` for list termination**: Use `empty?` instead
- **Not escaping HTML**: Use `html-escape` for all user-provided content
- **Assuming lists are nil-terminated**: Lists terminate with `EMPTY_LIST`
- **Not handling missing input files**: Check file existence before reading
- **Infinite recursion**: Ensure recursive functions have a base case that checks for `EMPTY_LIST`

## Testing the generator

To test the generator locally:

```bash
cargo build --release
./target/release/elle elle-doc/generate.lisp
# Check elle-doc/docs/ for generated output
```

If the generator fails, check:
1. The error message for the specific failure
2. The input JSON files in `elle-doc/`
3. The list termination logic (use `empty?`, not `nil?`)
4. The string operations (use `string-split`, `substring`, etc.)
