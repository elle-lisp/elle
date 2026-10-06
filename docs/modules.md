# Modules

<!-- audited: 2026-10-06 -->

Elle's modules: three raw loaders, the `import-file` form over them, an `import` macro in Elle, and conventions built from closures.

There is no module syntax, no export declaration, and no visibility modifier.
[modules-proposal.md](modules-proposal.md) describes the module system this
one is becoming.


## Loading a file: import-file

`(import-file path)` loads the file that `path` names, and nothing else. A
relative path names a file in the directory of the source file that holds the
form, the writer's directory. An absolute path is used as it stands. Code with
no file, at the REPL, on stdin or under `-e`, uses the working directory.

`import-file` adds no prefix, searches no directory and tries no suffix. A
path that ends in `.so`, `.dylib` or `.dll` loads as a plugin through
`import/load-plugin`. Any other path loads as Elle source through
`import/load-file`.

```lisp
(def m ((import-file "../tests/modules/test.lisp")))
(assert (= 42 m:test-var) "a relative path resolves against this document's directory")
```

`import-file` is a special form, so it is not a value. With a literal
argument, the compiler joins the path to the writer's directory and picks the
loader, so it knows the file before anything runs. A computed argument follows
the same rule when the form runs. The result does not depend on where the
process started, or on which file calls the function that holds the form.


## The loaders

Three primitives load a module. Each takes a path as `slurp` does: a relative
path resolves against the working directory. Code calls `import-file` or
`import` instead, so that a relative path follows the writer.

| Primitive | Loads |
|-----------|-------|
| `import/load-file` | Elle source: it compiles the file and runs it |
| `import/load-plugin` | A shared library: it runs the library's `elle_plugin_init` |
| `import/load-syntax` | One form: it compiles the form as a module and runs it |

`import/load-file` reads the file as UTF-8 source. It compiles the file, runs
it on the current fiber, and returns the file's last expression. The file runs
as a single letrec. Every call compiles and runs the file again.

`import/load-plugin` loads the library, calls `elle_plugin_init`, and returns
the struct of primitives that the init builds. A later call with the same path
returns that struct and loads nothing. It is the one loader that requires
`:ffi`, because the init is foreign code.

`import/load-syntax` takes one form, compiles it as a module, runs it, and
returns its value. The form keeps its own source locations.

A module's top level must not suspend. The loader runs it on the current fiber
and cannot hold a suspension of that fiber, so a top level that sleeps or
reads a port fails the load with an `unexpected signal` error. The lambda a
module returns is an ordinary closure, and it may suspend when called.
[park.md](impl/region/park.md) owns the rule.


## import

`import` is a macro in the prelude, written in Elle over `import-file`.
`(import spec)` expands to `(import-file (import/resolve spec dir))`, where
`dir` is the writer's directory. `import/resolve` is a function in the
standard library. Nothing in `import` is built in, so a program can write its
own resolver and its own macro the same way.

```lisp
(def b64 ((import "std/base64")))
(assert (= "aGVsbG8=" (b64:encode "hello")))
```

`import` is a macro, so it is not a value. To import each spec of a list, call
`import` inside a function: `(map (fn [s] (import s)) specs)`.


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

Virtual prefixes are the preferred import style. They decouple module
references from the filesystem layout.


## The writer's directory: meta/location

`(meta/location)` is a special form. It returns `{:file :line :col}` for the
form itself, as `(meta/origin f)` does for a closure. `:file` is an absolute
path, or nil for code with no file: the REPL, stdin, `-e`, and a datum that
`eval` compiles. The compiler fixes the value, so it does not depend on who
calls the code.

A form that a macro builds carries the location of the macro call. So the
`meta/location` inside the expansion of `import` names the file that called
`import`.

```lisp
(def loc (meta/location))
(assert (= "modules.md" (path/filename (get loc :file))))
(assert (= loc:file (path/absolute loc:file)) "the file is an absolute path")
```


## Convention: closure-as-module

A module file defines private bindings, then exports a subset by returning a
closure that produces a struct:

```lisp
# greet.lisp
(def greeting "Hello")

(defn format-greeting [name]
  (string greeting ", " name "!"))

(fn [] {:greet format-greeting})
```

The caller imports, calls the closure, and binds the result:

```lisp
# (let [g ((import "./greet"))]
#   (g:greet "world"))       # => "Hello, world!"
```

`g:greet` is qualified symbol syntax. The reader lexes `g:greet` as a single
token, and the analyzer desugars it to `(get g :greet)`. It is a struct field
access, not special module syntax.

What the closure does not return is private. `greeting` and
`format-greeting` are not visible to the caller. Encapsulation comes from
lexical scope, not from access modifiers.

The compiler's signal projection reads this convention: a file that returns a
struct literal, or a closure whose body ends in one, gets a signal for each
field ([signals/inference.md](signals/inference.md)).


## Parametric modules

The closure can accept arguments, which makes the module configurable at
import time:

```lisp
# formatter.lisp
# (fn [&named @prefix @suffix @separator]
#   (default prefix "")
#   (default suffix "")
#   (default separator ", ")
#   (defn wrap [s] (string prefix s suffix))
#   (defn join [items] (string/join (map string items) separator))
#   {:wrap wrap :join join})
```

```lisp
# (let [fmt ((import "./formatter") :prefix "[" :suffix "]" :separator " | ")]
#   (fmt:wrap "hello")          # => "[hello]"
#   (fmt:join [1 2 3]))         # => "1 | 2 | 3"
```

Each call to the closure captures its own configuration. Two imports with
different arguments produce independent instances:

```lisp
# (let [parens  ((import "./formatter") :prefix "(" :suffix ")")
#       angles  ((import "./formatter") :prefix "<" :suffix ">")]
#   (parens:wrap "x")           # => "(x)"
#   (angles:wrap "x"))          # => "<x>"
```

This is ML's functor pattern without any dedicated syntax.


## Plugin-as-parameter

A library module can depend on a native plugin without importing it. The
caller imports the plugin and passes it to the library:

```lisp
# lib/mqtt.lisp — takes the mqtt plugin as a parameter
# (fn [plugin]
#   (defn connect [host port &keys opts] ...)
#   (defn subscribe [conn topics] ...)
#   (defn recv [conn] ...)
#   (defn close [conn] ...)
#   {:connect connect :subscribe subscribe :recv recv :close close})
```

```lisp
# Caller: import the plugin, pass it to the library
# (def mqtt-plugin (import "plugin/mqtt"))
# (def mqtt ((import "std/mqtt") mqtt-plugin))
#
# (let [conn (mqtt:connect "broker.example.com" 1883
#                          :client-id "elle-client")]
#   (mqtt:subscribe conn [["test/#" 0]])
#   (println "got:" (mqtt:recv conn))
#   (mqtt:close conn))
```

This decouples the library from the plugin's path. The library is pure Elle,
and the plugin is a native dependency the caller injects. The same library
works with a mock plugin in a test.


## Import styles

### Qualified (namespaced)

Bind the whole module, and access a field with `mod:name`:

```lisp
# (def portrait ((import "std/portrait")))
# (portrait:function analysis :my-fn)
```

### Destructured (flat)

Pull specific names into scope:

```lisp
# (def {:parse parse :pretty pretty} ((import "./json") :pretty-indent 4))
# (pretty (parse input))
```

### Side-effect only

`import` always returns the file's last expression. If the caller ignores it,
the file runs for its side effects. A file compiles as a single letrec, so its
top-level `defn` forms are local to the file and none leaks into the caller's
scope:

```lisp
# (import "./helpers")
# (double 21)                   # error: undefined variable: double
```

The only way to use a file's definitions is to have the file return them
explicitly (the closure pattern) and bind the result.

### Plugin (shared object)

A native plugin returns a struct of its primitives from `elle_plugin_init`.
The primitives are reachable through that struct alone: a later compile does
not know their names. The plugin that `make doctest` builds shows both:

```lisp
(def my (import "plugin/myplugin"))
(assert (= "hello" (my:hello)))
(def [ok? _] (protect (eval '(myplugin/hello))))
(assert (not ok?) "a plugin's names do not reach a later compile")
```


## Compile-time inclusion

`import` loads a module when it runs: it compiles and runs a file, and returns
a value. So a macro defined in an imported file is not available to the
importing file's compiler. By the time the import runs, expansion is finished.

`include` and `include-file` splice a file's source forms into the including
file at compile time, before macro expansion:

```lisp
# (include-file "macros.lisp")      — relative to the including file
# (include "./macros")              — resolved by import/resolve
```

### How it works

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
line.

### include vs include-file

| Form | Resolution | Parallel to |
|------|-----------|-------------|
| `(include-file "path")` | Relative to the including file's directory | `import-file` |
| `(include "spec")` | `import/resolve`, given the including file's directory | `import` |

The compiler calls the standard library's `import/resolve` to resolve an
`include`, so `include` and `import` share one set of rules.

### When to use include vs import

Use `import` when you want a module boundary: encapsulation, parameters,
independent state. The imported file runs in its own scope and returns a value.

Use `include` when you want definitions spliced into the current file's scope,
mostly to share macro definitions across files. An included file has no
encapsulation: every definition becomes part of the including file's letrec.

### Circular inclusion

The compiler detects a circular include. It tracks which files it has included,
the root file among them, and fails when a file appears twice:

```text
main.lisp:3:1: include: circular dependency on 'macros.lisp'
```


## Why this works

**No new concepts.** Modules are closures. Exports are structs. Configuration
is keyword arguments. Namespacing is struct field access. A programmer who
understands closures, structs, and destructuring already understands the
module system.

**Parametric by default.** A module that accepts configuration is a closure
that takes arguments. There is no functor syntax and no module-type
declaration.

**Encapsulation from scope.** If a binding is not in the returned struct, it
is not accessible. No `private` keyword is needed.

**Selective import.** Destructuring gives you exactly the names you want,
with renaming: `(def {:parse my-parse} ((import "./json")))`.

**First-class modules.** A module is a value. Store it in a variable, pass
it to a function, put it in a data structure, return it from another module.

**Uniform native and Elle treatment.** `.so` plugins and `.lisp` files both go
through `import` and both return values.

**Replaceable resolution.** `import` is a macro and `import/resolve` a
function, both written in Elle. A program that wants another module system
writes its own over `import-file` and the loaders.


## Architectural constraints

The module system makes four design choices, and each has a cost.

### No .lisp caching

Every load of a `.lisp` file compiles and runs it again. If two modules both
import `./utils`, the file runs twice. A literal `import-file`,
`((import-file "literal"))`, compiles its target twice: once in the importer's
analysis for its projection, and once when the load runs (#881).

**Why**: A cache would share state between independent callers and suppress
side effects. A stateful module, one that holds a mutable `@` binding and
`assign`s it, gets independent state per import. Two imports of one module
file make independent instances, as two calls to a factory function make
independent objects.

**Consequence**: The compile repeats. Where that matters, import once at the
top level and pass the module value down the call stack.

### Circular import detection is at run time

The VM tracks which files are being loaded. If file A loads file B, which
loads file A, the second load raises an error:

```text
import/load-file: circular dependency detected for '/home/me/a.lisp'
```

The set holds only the loads that are in progress. Every way out of a load
releases its mark, so a file that failed — a compile error, a read failure, an
error it raised — reports that same failure again when it is loaded once
more, and only a load that is still on the stack reads as a cycle.

A cycle of literal `import-file` forms at the top level never reaches that
check. The analyzer compiles the target of each one for its projection, and
the cycle overflows the stack at compile time (#1323).

**Why**: Loading happens at run time, so the check runs when a load re-enters
a file that is still loading.

### Cross-file signal inference via projection

Signal inference within a file converges by a fixpoint loop, so mutual
recursion inside a file is exact. Across files the compiler uses a **signal
projection** instead.

When a file returns a struct of closures, the compiler records a projection: a
map from each keyword field to the signal of the closure it holds. When an
importing file binds `((import-file "literal"))`, the analyzer compiles the
target file, or finds it in the instance's cache, and reads its projection.
[signals/inference.md](signals/inference.md) owns the mechanism.

The compiler knows the file of a literal `import-file` alone. `import` is a
macro over a resolver the program may replace, so `((import "std/x"))` is an
ordinary call, and the analyzer reads no projection for it.

A qualified access such as `math:add` gives its `get` node the projected
signal. A call through it still takes the unknown signal, because the callee is
a call expression (#1232). So today a projection does not narrow a call into
another file.

Convergence is per file. Mutual recursion across a file boundary does not
converge, because each load is a separate compilation.

### Static analysis is limited across imports

The analyzer processes one file at a time. Arity checking, IDE completion and
refactoring do not cross an import boundary.

**Why**: Imports are dynamic. The return value depends on runtime
parameters and computation. A projection reads only the shape of the return
expression, so it needs the target's analysis and never runs it.

**Solution for agents**: The [MCP knowledge graph](mcp.md) gives cross-file
visibility through SPARQL queries. See
[Agent Reasoning in Elle](analysis/agent-reasoning.md) for cross-file
reasoning patterns.


## Implementation

| File | Role |
|------|------|
| [modules.rs](../src/primitives/modules.rs) | The loaders: file I/O, compilation, execution, circular load detection, plugin caching |
| [prelude.lisp](../src/prelude.lisp) | The `import` macro |
| [stdlib.lisp](../src/stdlib.lisp) | `import/resolve` |
| [plugin.rs](../src/plugin.rs) | `.so` plugin loading: `dlsym`, `elle_plugin_init`, the struct of primitives |
| [registry.rs](../src/hir/analyze/forms/registry.rs) | The `import-file` and `meta/location` special forms |
| [special.rs](../src/hir/analyze/forms/special.rs) | Qualified symbol desugaring (`a:b` → `(get a :b)`), and the projected signal of a qualified `get` |
| [call.rs](../src/hir/analyze/call.rs) | Literal `import-file` detection, compile-time squelch inference |
| [fileletrec.rs](../src/hir/analyze/fileletrec.rs) | `compute_signal_projection`: extracts the keyword→signal map from a struct-returning file |
| [cache.rs](../src/pipeline/cache.rs) | The per-instance signal projection cache, `get_or_compile_projection` |
| [lexer.rs](../src/reader/lexer.rs) | Qualified symbol lexing (`a:b` as a single token) |
| [compile.rs](../src/pipeline/compile.rs) | `compile_file`: file-as-letrec compilation, `include`/`include-file` splicing, projection threading |
| [transforms.rs](../src/pipeline/compile/transforms.rs) | `include` and `include-file` resolution, and the circular include check |
| [projection.rs](../tests/integration/projection.rs) | Signal projection and compile-time squelch tests |
| [modules.lisp](../tests/lang/modules.lisp) | Behavioral tests for module patterns |
| [include.lisp](../tests/lang/include.lisp) | Behavioral tests for compile-time inclusion |
| [modules/](../tests/modules) | Module fixtures (formatter, counter, test) |
