# Modules

<!-- audited: 2026-11-13 -->

Elle's modules: one form per file, loaded by a request that the enclosing handler answers, with every loader written in Elle over `eval`.

There is no module syntax, no export declaration, and no visibility modifier.
[modules-proposal.md](modules-proposal.md) describes the module system this
one is becoming, and [shape.md](modules-proposal/shape.md) states why a module
has the shape below. A module runs after its importer has expanded, so its
macros never reach the importer; [include.md](include.md) shares macros
between files.


## A module is one form

A module file holds one form after its `(elle/epoch N)` line and its version
declarations ([versioning.md](versioning.md)). A load evaluates that form and
answers its value. The form may build any value. Most modules return a
function, and a call of that function builds the exports:

```lisp
# greet.lisp
# (fn []
#   (defn format-greeting [name] (string "Hello, " name "!"))
#   {:greet format-greeting})
```

The caller imports, calls the function, and binds the result:

```lisp
# (let [g ((import "./greet"))]
#   (g:greet "world"))       # => "Hello, world!"
```

`g:greet` is qualified symbol syntax. The reader lexes `g:greet` as one token,
and the analyzer desugars it to `(get g :greet)`. What the function does not
return is private: encapsulation comes from lexical scope. A module that
returns a function and one that returns a struct look alike at the import, so
`get` on a function says that the module has to be called first:

```lisp
(let [[ok? err] (protect (get (fn [] {:x 1}) :x))]
  (assert (not ok?))
  (assert (string/contains? (get err :message) "called before")))
```

The form runs with every effect capability withheld, so it cannot import, read
a file, print or read ambient state, and its value may hold no mutable state.
Effects and dependencies go inside the function the value holds, and a call of
that function runs them:

```lisp
# lib/x.lisp, as one form:
# (letrec [table (build-table)]            # pure: built when the file loads
#   (fn [&named level]                     # a call: may be impure
#     (def b64 ((import "std/base64")))    # a dependency, imported when called
#     {:encode (fn [s] (b64:encode (lookup table s level)))}))
```

A program that `elle` runs is not a module. It keeps many forms and may be
impure, as the files under [tests/lang](../tests/lang) are.


## import

`import` is a prelude macro. `(import spec)` emits the `:import` signal with a
request, `{:spec spec :dir dir :loader loader}`. `dir` is the writer's
directory, or nil for code with no file, and `loader` is what `*loader*` holds
where the import runs. The request climbs to the nearest fiber whose mask
names `:import`, which answers `[ok? value]` as a join does. `import` answers
the value, or raises it when `ok?` is false. A program's fibers run under the
scheduler ([scheduler.md](scheduler.md)), which calls the request's loader on
a fiber of its own, so the loader's I/O parks as any fiber's does.

```lisp
(def b64 ((import "std/base64")))
(assert (= "aGVsbG8=" (b64:encode "hello")))
```

The default loader is `import/load`. It resolves the spec with
`import/resolve`, hands a path that ends in `.so`, `.dylib` or `.dll` to
`import/load-plugin`, and every other path to `import/load-file`.
`parameterize` replaces the loader as it replaces any parameter, and a spawned
fiber starts with its spawner's bindings. `import/with-loader` answers each
request of a thunk through the loader it is given, whatever the request names:

```lisp
(def stubbed (parameterize ((*loader* (fn [r] {:spec (get r :spec)})))
               (import "std/nothing")))
(assert (= "std/nothing" stubbed:spec))
(assert (= :handled (import/with-loader (fn [_] :handled) (fn [] (import "x")))))
(assert (= import/load (*loader*)) "the default loader")
```

`import` is a macro, so it is not a value. To import each spec of a list,
call `import` inside a function: `(map (fn [s] (import s)) specs)`.


## The loaders

Each loader is an ordinary function: a program may call one directly, or build
a loader of its own from the same parts. Only `import/load-plugin` is a
primitive.

| Function | Does |
|----------|------|
| `(import/load-file path)` | Reads the file with `file/read`, then `import/load-source` |
| `(import/load-source name source)` | Reads the forms of `source` with `compile/read-forms`, which migrates the epoch and strips the declarations; refuses more or fewer than one form; then `import/load-syntax` |
| `(import/load-syntax form)` | Evaluates one form, a datum or a syntax object, as a module, and answers its value |
| `(import/load-plugin path)` | Loads the shared library at `path`, runs its `elle_plugin_init`, and answers the struct of primitives it built. A later call with the same path answers that struct and loads nothing. Needs `:ffi` |

`import/load-syntax` compiles the form with `eval` on the calling fiber, as the
body of a function, and calls that function in a fiber. The compile resolves
the primitives, the prelude, the standard library and the bindings an
embedder registers, never what a REPL session defined, and expands macros and
includes as any compile does ([macros.md](macros.md)). A compile error names
the file and line the form came from. The fiber withholds the effect
capabilities, `:ambient` and `:import`, so a primitive the form calls that
needs one fails the load, naming the primitive. A refusal of the loader's own
is a `:module-error`:

```lisp
(assert (= 3 (import/load-syntax '(+ 1 2))) "a datum loads as a module")
(def m ((import/load-source "<doc>" "(fn [] {:x 1})")))
(assert (= 1 m:x) "a form read from text loads as a module")
(let [[ok? err] (protect (import/load-source "<doc>" "(def a 1) (fn [] a)"))]
  (assert (and (not ok?) (= :module-error (get err :error))) "two forms"))
(let [[ok? err] (protect (import/load-source "<doc>" "(fn [] (undefined-name))"))]
  (assert (and (not ok?) (string/contains? (get err :message) "<doc>:1:"))
          "a compile error names the file and line"))
(let [[ok? err] (protect (import/load-syntax '(sys/env "HOME")))]
  (assert (and (not ok?) (string/contains? (get err :message) "sys/env"))
          "an ambient read in the form is refused"))
(let [[ok? err] (protect (import/load-syntax '((import "std/base64"))))]
  (assert (and (not ok?) (string/contains? (get err :message) ":import"))
          "a form cannot import"))
```


## Purity: no mutable state in the value

After the form runs, the loader walks its value and refuses mutable state,
because a value a module answers may be shared by every importer. It refuses
a mutable collection, a `box`, a closure that captures a binding code
assigns, a fiber, a port, a thread or library handle, a managed pointer
and an external. The walk stops at a parameter and never reads it. The walk
reads a closure through `fn/captures`, which answers each captured slot as
`{:value v :mutable? b}`, with `:name` beside them when the compiler recorded
the binding. So a program can run the same check on any closure:

```lisp
(def counter (let [@n 0 k 5] (fn [] (assign n (+ n k)) n)))
(def slots (fn/captures counter))
(assert (= 2 (length slots)))
(assert (any? (fn [s] (get s :mutable?)) slots) "n can still be assigned")
(assert (not (all? (fn [s] (get s :mutable?)) slots)) "k cannot")
(let [[ok? err] (protect (import/load-syntax '@[1 2]))]
  (assert (and (not ok?) (= :module-error (get err :error))) "a mutable array"))
(let [[ok? err] (protect (import/load-syntax '(let [@n 0] (fn [] (assign n 1) n))))]
  (assert (and (not ok?) (= :module-error (get err :error)))
          "a closure over a binding it assigns"))
(assert (closure? (import/load-syntax '(fn [] (var n 0) (fn [] (assign n 1) n))))
        "state built inside the function is the caller's own")
```


## Resolution

`(import/resolve spec &opt dir)` returns the absolute, normalized path of the
file that `spec` names. It raises an `:io-error` when `spec` names no file.

Two virtual prefixes come first:

| Prefix | Resolves to | Example |
|--------|-------------|---------|
| `std/X` | `<root>/lib/X.lisp` | `(import "std/portrait")` |
| `plugin/X` | `<root>/target/<profile>/libelle_X.<suffix>` | `(import "plugin/regex")` |

The project root is `--home` (or `ELLE_HOME`), or the first directory above
the elle binary that holds a `Cargo.toml`. A plugin is looked for under the
profile of the running binary (release or debug) first, then under the other.
A prefix whose file does not exist falls through to the search below.

A spec that starts with `./` or `../` names a file relative to `dir`, and is
looked for there alone. Without `dir`, such a spec names nothing. An absolute
spec is looked for from the filesystem root alone. Any other spec is looked
for in these directories, in order:

1. Each `--path` / `ELLE_PATH` entry (colon-separated)
2. `--home` / `ELLE_HOME`, or the directory of the elle binary

A relative `--path` entry resolves against the working directory. The search
never visits the working directory or the writer's directory, so a file
beside a program cannot shadow a library of the same name. To search the
working directory, put it on the path: `ELLE_PATH=.`.

In each directory, `import/resolve` tries:
- `<dir>/<spec>.lisp`
- `<dir>/<spec>` (as it stands)
- `<dir>/<spec_dir>/libelle_<leaf>.<suffix>` (hierarchical plugin layout)
- `<dir>/libelle_<leaf>.<suffix>` (flat plugin layout)

The suffix is the platform's: `so`, `dylib` or `dll`. `import/resolve` reads
it and the two settings from `vm/config` ([config.md](config.md)).

```lisp
(def here (path/parent (get (meta/location) :file)))
(assert (= "base64.lisp" (path/filename (import/resolve "std/base64"))))
(assert (= (path/join here "modules.md") (import/resolve "./modules.md" here)))
(let [[ok? err] (protect (import/resolve "./modules.md"))]
  (assert (and (not ok?) (= :io-error (get err :error)))
          "without a directory, a ./ spec names nothing"))
```


## The writer's directory: meta/location

`(meta/location)` is a special form. It returns `{:file :line :col}` for the
form itself, as `(meta/origin f)` does for a closure. `:file` is an absolute
path, or nil for code with no file: the REPL, stdin, `-e`, and a datum that
`eval` compiles. The compiler fixes the value, so it does not depend on who
calls the code.

A form that a macro builds carries the location of the macro call. So the
`meta/location` inside the expansion of `import` names the file that called
`import`, and a relative spec follows the file that wrote it, whatever file
calls the function that holds the import:

```lisp
(def loc (meta/location))
(assert (= "modules.md" (path/filename (get loc :file))))
(assert (= loc:file (path/absolute loc:file)) "the file is an absolute path")
(let [importer ((import "../tests/modules/importer"))
      m (importer:load)]
  (assert (= m:test-var 42) "importer.lisp's import resolves beside importer.lisp"))
```


## Parametric modules

The function a module returns can accept arguments, which makes the module
configurable at import time. Each call captures its own configuration, so two
calls with different arguments produce independent instances.
[formatter.lisp](../tests/modules/formatter.lisp) takes `:prefix`, `:suffix`
and `:separator`:

```lisp
(def formatter (import "../tests/modules/formatter"))
(def fmt (formatter :prefix "[" :suffix "]" :separator " | "))
(assert (= "[hello]" (fmt:wrap "hello")))
(assert (= "1 | 2 | 3" (fmt:join [1 2 3])))
(def parens (formatter :prefix "(" :suffix ")"))
(def angles (formatter :prefix "<" :suffix ">"))
(assert (= ["(x)" "<x>"] [(parens:wrap "x") (angles:wrap "x")]))
(def {:wrap angle-wrap} (formatter :prefix "<" :suffix ">"))
(assert (= "<y>" (angle-wrap "y")) "a destructuring def pulls one export into scope")
```

This is ML's functor pattern without any dedicated syntax. A library module
that depends on a native plugin takes it the same way, so the library stays
pure Elle and a test hands it a mock:

```lisp
# (def mqtt ((import "std/mqtt") (import "plugin/mqtt")))
```

A native plugin returns a struct of its primitives from `elle_plugin_init`.
The primitives are reachable through that struct alone: a later compile does
not know their names. The plugin that `make doctest` builds shows both:

```lisp
(def my (import "plugin/myplugin"))
(assert (= "hello" (my:hello)))
(def [ok? _] (protect (eval '(myplugin/hello))))
(assert (not ok?) "a plugin's names do not reach a later compile")
```


## Architectural constraints

### A load evaluates the form again

Every load of a `.lisp` file compiles and evaluates its form. The loader is
Elle code and keeps no cache yet, so two loads of one file build two values,
and a load runs the text the file holds now:

```lisp
(with-temp-dir dir
  (let [p (path/join dir "counter.lisp")]
    (file/write p "(fn [] (var n 0) (fn [] (assign n (+ n 1)) n))\n")
    (let [a ((import/load-file p))
          b ((import/load-file p))]
      (a)
      (a)
      (assert (= 1 (b)) "two loads keep independent state"))
    (file/write p "(fn [] :edited)\n")
    (assert (= :edited ((import/load-file p)))
            "a load runs the text the file holds now")))
```

### An import cycle is the program's own recursion

A module's form cannot import, so a load never starts another load, and
nothing tracks loads in progress. A module's dependencies are imported when
its function is called. Two modules whose functions import and call each
other recurse as any two functions do: the calls end in `:stack-overflow`, or
never end when every call is a tail call.

### No signal crosses a file boundary

Within a file, the signals of definitions that call each other settle jointly
([pipeline.md](pipeline.md)). The analysis of a file reads nothing from the
files it imports, so a call into another file takes the unknown signal
(#1232). [signals/inference.md](signals/inference.md) owns the rule. Arity
checking, IDE completion and refactoring stop at an import for the same
reason; the [MCP knowledge graph](mcp.md) gives cross-file visibility.


## Implementation

| File | Role |
|------|------|
| [prelude.lisp](../src/prelude.lisp) | The `import` macro |
| [stdlib.lisp](../src/stdlib.lisp) | `import/resolve`, `import/load`, the three Elle loaders, `*loader*`, `import/with-loader`, and the scheduler's answer to `:import` |
| [modules.rs](../src/primitives/modules.rs) | `import/load-plugin` |
| [plugin.rs](../src/plugin.rs) | `.so` plugin loading: `dlsym`, `elle_plugin_init`, the struct of primitives |
| [special.rs](../src/hir/analyze/forms/special.rs) | Qualified symbol desugaring (`a:b` → `(get a :b)`) |
| [lexer.rs](../src/reader/lexer.rs) | Qualified symbol lexing (`a:b` as a single token) |
| [modules.lisp](../tests/lang/modules.lisp) | Behavioral tests for module patterns |
| [import-loaders.lisp](../tests/lang/import-loaders.lisp) | The loaders, the request, the handler and the purity walk |
| [modules/](../tests/modules) | Module fixtures (formatter, importer, test) |
