# Including a file

<!-- audited: 2026-10-22 -->

`include` and `include-file` splice a file's forms in place of the include, wherever the expander meets it.

`import` loads a module when it runs: it compiles and runs a file, and returns
a value ([modules.md](modules.md)). So a macro defined in an imported file is
not available to the importing file's compiler. By the time the import runs,
expansion is finished. An include puts the file's forms in front of the
expander instead:

```lisp
(include-file "../tests/modules/incspec.lisp")
(assert (= 28 (quadruple-it 7)) "a macro from an included file expands here")
```

## Where an include splices

When the expander meets an `include` or `include-file` form, it reads the file
the form names and puts the file's forms in its place, in the form that holds
it. Then it expands those forms as if they were written there:

- At the top level, the forms join the file. Included `defmacro` forms
  register in the expander, and `def` and `defn` forms enter the file's letrec.
- In a body, such as a function's or a `let`'s, the forms join that body. Its
  definitions see one another, forward and mutual references included.
- In any other list, array, struct or set, the forms take the include's place
  as elements. A position that holds one form, such as the datum `eval` compiles
  or an unquoted expression, takes them inside a `begin`.

A macro may expand to an include. The expander then splices the file where the
macro call stood.

```lisp
(defn scaled [factor x]
  (include-file "../tests/modules/incbody.lisp")
  (scale x))
(assert (= 30 (scaled 3 10)) "an included definition joins the body")
```

## What an included form sees

Each included form takes the scopes of the include form, so it resolves names
as code written at the include would. In the example above, `scale` reads
`factor`, the parameter of the function that holds the include. At the top
level, included code sees the including file's definitions.

An include that a macro's template writes carries that macro's intro scope.
The forms it splices are then hygienic, as the rest of the template is
([macros.md](macros.md)).

Error messages and stack traces point back to the included file and line. An
include inside an included file resolves against the file that holds it.

## include vs include-file

| Form | Resolution | Parallel to |
|------|-----------|-------------|
| `(include-file "path")` | Relative to the including file's directory | `import-file` |
| `(include "spec")` | `import/resolve`, given the including file's directory | `import` |

The compiler calls the standard library's `import/resolve` to resolve an
`include`, so `include` and `import` share one set of rules
([modules.md](modules.md)).

## Which compiles splice

Every compile that expands splices its includes: a file, a REPL line, the datum
`eval` compiles, `compile/analyze`, and the loaders. An include reads its file
as an input of the compile, so a compile that a fiber started reads no included
file when that fiber withholds `:fs` ([macros.md](macros.md)).

## When to use include vs import

Use `import` when you want a module boundary: encapsulation, parameters,
independent state. The imported file runs in its own scope and returns a value.

Use `include` when you want definitions spliced into the current scope, mostly
to share macro definitions across files. An included file has no
encapsulation: every definition joins the file or the body that includes it.

## Circular inclusion

An include of a file that is already being included is a cycle. The compile
fails, and the error names each file from the repeated one back to itself:

```text
main.lisp:3:1: include: circular dependency: /src/main.lisp -> /src/macros.lisp -> /src/main.lisp
```

A file may be included more than once, as long as no include of it lies inside
its own forms. Two function bodies may each include one helper file.

## Implementation

| File | Role |
|------|------|
| [include.rs](../src/pipeline/compile/include.rs) | Resolution, the circular include check, reading the file, and the WASM backend's splice of source text |
| [include.rs](../src/syntax/expand/include.rs) | The splice into the form that holds the include, and the scopes each included form takes |
| [include.lisp](../tests/lang/include.lisp) | Behavioral tests of a top-level include |
| [include-splice.lisp](../tests/lang/include-splice.lisp) | Behavioral tests of each position an include splices into, its scopes, and its cycles |
