# Including a file

<!-- audited: 2026-10-21 -->

`include` and `include-file` splice a file's forms into the including file at compile time, before macro expansion.

`import` loads a module when it runs: it compiles and runs a file, and returns
a value ([modules.md](modules.md)). So a macro defined in an imported file is
not available to the importing file's compiler. By the time the import runs,
expansion is finished. An include puts the file's forms in front of the
expander instead:

```lisp
(include-file "../tests/modules/incspec.lisp")
(assert (= 28 (quadruple-it 7)) "a macro from an included file expands here")
```

## How it works

When a file compile meets an `include` or `include-file` form at the top level,
it:

1. Reads and parses the target file, keeping the included file's source
   locations
2. Splices the parsed forms into the current file's form list at that position
3. Continues expanding: included `defmacro` forms register in the expander,
   `def`/`defn` forms enter the file's letrec, and everything else expands
   normally

The included forms become part of the including file as if they were written
inline. Error messages and stack traces point back to the original file and
line. An include inside an included file resolves against the file that holds
it.

## include vs include-file

| Form | Resolution | Parallel to |
|------|-----------|-------------|
| `(include-file "path")` | Relative to the including file's directory | `import-file` |
| `(include "spec")` | `import/resolve`, given the including file's directory | `import` |

The compiler calls the standard library's `import/resolve` to resolve an
`include`, so `include` and `import` share one set of rules
([modules.md](modules.md)).

## When to use include vs import

Use `import` when you want a module boundary: encapsulation, parameters,
independent state. The imported file runs in its own scope and returns a value.

Use `include` when you want definitions spliced into the current file's scope,
mostly to share macro definitions across files. An included file has no
encapsulation: every definition becomes part of the including file's letrec.

## Circular inclusion

The compiler detects a circular include. It tracks which files it has included,
the root file among them, and fails when a file appears twice:

```text
main.lisp:3:1: include: circular dependency on 'macros.lisp'
```

## Implementation

| File | Role |
|------|------|
| [include.rs](../src/pipeline/compile/include.rs) | Resolution and splicing, and the circular include check |
| [include.lisp](../tests/lang/include.lisp) | Behavioral tests |
