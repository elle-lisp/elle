# Including a file

<!-- audited: 2026-11-19 -->

`include-file` splices a file's forms in place of the include, wherever the expander meets it.

`import` loads a module when it runs: it compiles and runs a file, and returns
a value ([modules.md](modules.md)). So a macro defined in an imported file is
not available to the importing file's compiler. By the time the import runs,
expansion is finished. An include puts the file's forms in front of the
expander instead. It is the one file a compile reads for the code it
compiles, and the compiler has to do that read itself. A macro body withholds
`:fs`, so no macro can read a file in its place, and a module cannot export a
macro ([warts.md](warts.md)).

```lisp
(include-file "../tests/modules/incspec.lisp")
(assert (= 28 (quadruple-it 7)) "a macro from an included file expands here")
```

## Where an include splices

When the expander meets an `include-file` form, it reads the file the form
names and puts the file's forms in its place, in the form that holds it. Then
it expands those forms as if they were written there:

- At the top level, the forms join the file. Included `defmacro` forms
  register in the expander, and `def` and `defn` forms bind in the file.
- In a body, such as a function's or a `let`'s, the forms join that body. A
  `def` among them binds its name from that `def` onward, as one written there
  does ([bindings](bindings.md)).
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
level, included code sees the definitions the including file makes before the
include. A definition after the include is not bound yet, so an included form
that reads it fails to compile with an undefined variable, as code written at
the include would.

An include that a macro's template writes carries that macro's intro scope.
The forms it splices are then hygienic, as the rest of the template is
([macros.md](macros.md)).

Error messages and stack traces point back to the included file and line. An
include inside an included file resolves against the file that holds it.

## Which file an include reads

`(include-file "path")` joins the path to the directory of the file that holds
the include, or to the working directory for code with no file. It searches
nothing else, as `import/load-file` takes a path as written.

No include resolves a spec as `import` does. The rules `import/resolve`
follows are Elle code that probes the filesystem through the scheduler, and a
compile calls no Elle code to find a file. `include` is an ordinary name, so a
spec form is a call that fails to compile:

```lisp
(def [ok? err] (protect (eval (read "(include \"std/base64\")"))))
(assert (not ok?) "include names no form")
(assert (string/contains? (get err :message) "undefined variable: include"))
```

## Which compiles splice

Every compile that expands splices its includes: a file, a REPL line, the datum
`eval` compiles, `compile/analyze`, and the loaders. An include reads its file
as an input of the compile, so a compile that a fiber started reads no included
file when that fiber withholds `:fs` ([macros.md](macros.md)).

## When to use include-file vs import

Use `import` when you want a module boundary: encapsulation, parameters,
independent state. The imported file runs in its own scope and returns a value.

Use `include-file` when you want definitions spliced into the current scope,
mostly to share macro definitions across files. An included file has no
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
| [include.rs](../src/pipeline/compile/include.rs) | The path's join, the circular include check, reading the file, and the WASM backend's splice of source text |
| [include.rs](../src/syntax/expand/include.rs) | The splice into the form that holds the include, and the scopes each included form takes |
| [include.lisp](../tests/lang/include.lisp) | Behavioral tests of a top-level include |
| [include-splice.lisp](../tests/lang/include-splice.lisp) | Behavioral tests of each position an include splices into, its scopes, and its cycles |
