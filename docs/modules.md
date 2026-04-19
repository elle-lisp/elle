# Modules

Elle's module system is built from Elle, plus one primitive: `import-file`.
Conventions — closures, structs, keyword arguments, destructuring — provide
the rest. No special module syntax, no export declarations, no visibility
modifiers.


## The Primitive: import-file

`import-file` takes a path string and does two things:

1. For `.lisp` files: read the file, compile it, execute it, return the last
   expression. Essentially `(eval (slurp path))` — the file runs as a
   single letrec, and whatever its last expression evaluates to becomes
   the return value.

2. For shared libraries (`.so` on Linux, `.dylib` on macOS): load the
   library, call `elle_plugin_init`, cache the result. Subsequent
   `import-file` calls for the same library return the cached value
   without re-loading. (Only plugins are cached — `.lisp` files are always
   recompiled and re-executed.)

That is the entire mechanism. Everything else in the module system is
convention built on top of this one primitive.

```lisp
# (import-file "lib/http.lisp")           — value (last expr of file)
# (import-file "target/release/libelle_regex.so")  — value (plugin struct)
```


## The Convenience: import

The standard library provides `import`, which composes `import-file` with
search-path resolution and virtual prefixes:

| Prefix | Resolves to | Example |
|--------|-------------|---------|
| `std/X` | `<root>/lib/X.lisp` | `(import "std/portrait")` |
| `plugin/X` | `<root>/target/<profile>/libelle_X.so` | `(import "plugin/regex")` |

The project root is `ELLE_HOME`, or auto-detected by walking up from the
binary to find `Cargo.toml`. Plugin resolution prefers the same build
profile as the running binary and falls back to the other.

When no virtual prefix matches, `import` searches:

1. Current working directory
2. `ELLE_PATH` entries (colon-separated)
3. `ELLE_HOME` (or directory of the elle binary)

For each directory, it tries `<dir>/<spec>.lisp`, `<dir>/<spec>` as-is,
and `<dir>/libelle_<leaf>.so`.

```lisp
# (import "std/portrait")         — virtual prefix: std/portrait → lib/portrait.lisp
# (import "plugin/regex")         — virtual prefix: libelle_regex.so
# (import "my/local/utils.lisp")  — search path resolution
```

Virtual prefixes are the preferred import style. They decouple module
references from filesystem layout.


## Convention: Closure-as-Module

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
# (let ([g ((import "greet.lisp"))])
#   (g:greet "world"))       # => "Hello, world!"
```

`g:greet` is qualified symbol syntax — the reader lexes `g:greet` as a
single token, and the analyzer desugars it to `(get g :greet)`. It's a
struct field access, not special module syntax.

What the closure does not return is private. `greeting` and
`format-greeting` are not visible to the caller. Encapsulation comes from
lexical scope, not from access modifiers.

**This convention is load-bearing.** The compiler's signal projection
system depends on the return expression being a struct literal (or a
closure whose body is a struct literal). When a file follows this
convention, the compiler can extract a signal profile for each exported
closure — enabling cross-file signal inference. If a file returns a
dynamic or computed value, cross-file signal inference falls back to the
conservative Polymorphic. See [signals/inference.md](signals/inference.md)
for details.


## Parametric Modules

The closure can accept arguments, making the module configurable at import
time:

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
# (let ([fmt ((import "formatter.lisp") :prefix "[" :suffix "]" :separator " | ")])
#   (fmt:wrap "hello")          # => "[hello]"
#   (fmt:join [1 2 3]))         # => "1 | 2 | 3"
```

Each call to the closure captures its own configuration. Two imports with
different arguments produce independent instances:

```lisp
# (let ([parens  ((import "formatter.lisp") :prefix "(" :suffix ")")]
#       [angles  ((import "formatter.lisp") :prefix "<" :suffix ">")])
#   (parens:wrap "x")           # => "(x)"
#   (angles:wrap "x"))          # => "<x>"
```

This is ML's functor pattern without any dedicated syntax.


## Plugin-as-Parameter

A common pattern: a library module depends on a native plugin but doesn't
import it directly. Instead, the caller imports the plugin and passes it
to the library:

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

This decouples the library from the plugin's path — the library is pure
Elle, the plugin is a native dependency injected by the caller. The same
library works with mock plugins for testing.


## Import Styles

### Qualified (namespaced)

Bind the whole module, access via `mod:name`:

```lisp
# (def portrait ((import "std/portrait")))
# (portrait:function analysis :my-fn)
```

### Destructured (flat)

Pull specific names into scope:

```lisp
# (def {:parse parse :pretty pretty} ((import "json.lisp") :pretty-indent 4))
# (pretty (parse input))
```

### Side-effect only

`import` always returns the file's last expression. If the caller ignores it,
the file runs for its side effects. But because files are compiled as a single
letrec, top-level `defn` forms are local to the file — no definitions leak
into the caller's scope:

```lisp
# (import "helpers.lisp")
# (double 21)                   # error: undefined variable: double
```

The only way to use a file's definitions is to have the file return them
explicitly (closure pattern) and bind the result.

### Plugin (shared object)

Native plugins return a struct from `elle_plugin_init` and register their
primitives into the compilation cache (available to all subsequent compilations):

```lisp
# (import "plugin/random")
# (random/int 1 100)
```

The return value is also a struct, so the qualified pattern works:

```lisp
# (let ([rng (import "plugin/random")])
#   (rng:int 1 100))
```


## Compile-Time Inclusion

`import` is a runtime operation — it compiles and executes a file, returning a
value. This means macros defined in an imported file are not available to the
importing file's compiler. By the time `import` runs, expansion is finished.

`include` and `include-file` solve this by splicing a file's source forms
directly into the including file at compile time, before macro expansion:

```lisp
# (include-file "macros.lisp")      — relative to current file
# (include "lib/macros")            — uses search-path resolution
```

### How it works

When `compile_file` encounters an `include` or `include-file` form, it:

1. Reads and parses the target file (producing syntax objects with the
   included file's source locations intact)
2. Splices the parsed forms into the current file's form list at that position
3. Continues expanding — included `defmacro` forms register in the expander,
   `def`/`defn` forms enter the file's letrec, everything else expands normally

The included forms become part of the including file as if they were written
inline. Error messages and stack traces point back to the original file and
line.

### include vs include-file

| Form | Resolution | Parallel to |
|------|-----------|-------------|
| `(include-file "path")` | Relative to including file's directory | `import-file` |
| `(include "spec")` | Search paths (CWD, ELLE_PATH, ELLE_HOME), `.lisp` probing | `import` |

### When to use include vs import

Use `import` when you want a module boundary — encapsulation, parameterization,
independent state. The imported file runs in its own scope and returns a value.

Use `include` when you want definitions spliced into the current file's scope —
primarily for sharing macro definitions across files. Included files have no
encapsulation: every definition becomes part of the including file's letrec.

### Circular inclusion

Circular includes are detected at compile time. The compiler tracks which files
have been included (including the root file) and signals an error if a file
appears twice:

```
include: circular dependency on 'macros.lisp'
```

Unlike runtime circular import detection, this happens during compilation —
the cycle is caught before any code executes.


## Why This Works

**No new concepts.** Modules are closures. Exports are structs. Configuration
is keyword arguments. Namespacing is struct field access. A programmer who
understands closures, structs, and destructuring already understands the
module system.

**Parametric by default.** Modules that accept configuration are closures
that take arguments. No functor syntax, no module-type declarations.

**Encapsulation from scope.** If a binding is not in the returned struct, it
is not accessible. No `private` keyword needed.

**Selective import.** Destructuring gives you exactly the names you want,
with renaming: `(def {:parse my-parse} ((import "json.lisp")))`.

**First-class modules.** A module is a value. Store it in a variable, pass
it to a function, put it in a data structure, return it from another module.

**Uniform native/Elle treatment.** `.so` plugins and `.lisp` files both go
through `import` and both return values.

**One primitive.** The entire mechanism is `import-file` — read, compile,
execute, return. Everything else is Elle code and conventions.


## Architectural Constraints

These are design choices that enable the elegance of the module system. They are constraints, not limitations.

### No .lisp caching

Every `import` of a `.lisp` file recompiles and re-executes it. If two modules both `(import "utils.lisp")`, the file runs twice.

**Why**: Caching would create shared mutable state between independent callers and suppress side effects. Stateful modules (those using `var` and `assign`) must get independent state per import. This is a feature: two imports of the same module file get independent instances, just as two calls to a factory function create independent objects.

**Consequence**: Recompilation cost. For projects where this matters, import once at the top level and pass the module value down the call stack.

### Circular import detection is runtime-only

The VM tracks which files are currently being loaded. If file A imports file B which imports file A, the second import signals an error:

```text
import: circular dependency detected for 'a.lisp'
```

**Why**: `import` is a runtime primitive (compiles and executes a file). Circular dependency detection happens when the cycle is actually triggered, not preemptively. This is consistent with how the module system treats all imports as dynamic: the return value is computed at runtime, so dependency analysis is runtime-only.

**Consequence**: Circular import bugs surface at runtime, not compile-time. This is acceptable because circular imports are design errors, not programming mistakes—they should never happen in correct code.

### Cross-file signal inference via projection

Signal inference within a file uses the fixpoint loop (mutual recursion
converges). Cross-file signal inference uses a different mechanism:
**signal projection**.

When a file returns a struct of closures — the standard closure-as-module
convention — the compiler extracts a signal projection: a mapping from
keyword field names to the signals of the closures they hold. This is the
**load-bearing convention**: signal projection depends on the file's return
expression being a struct literal (or a closure whose body is a struct
literal). Dynamic or computed return values fall back to Polymorphic.

When an importing file sees `((import "std/math"))` with a literal string
argument, the compiler looks up (or compiles and caches) the target file's
projection. Qualified access like `math:add` then uses the projected
signal instead of the conservative Polymorphic fallback.

See [signals/inference.md](signals/inference.md) for the full mechanism,
including composition with compile-time squelch.

**Consequence**: Cross-file signal inference works automatically for
modules following the closure-as-module convention. Modules returning
dynamic values are treated conservatively.

**Mutual recursion across files**: Fixpoint convergence is per-file.
Mutual recursion across file boundaries does not benefit from cross-form
convergence — each import is a separate compilation.

### Static analysis is limited across imports

The analyzer processes one file at a time. Signal projection provides
cross-file signal data for qualified access. Other static features
(arity checking, IDE completion, refactoring) do not cross import
boundaries.

**Why**: Imports are fully dynamic. The return value depends on runtime
parameters and computation. Signal projection works because it only
requires analyzing the return expression's shape, not executing the file.

**Solution for agents**: The [MCP knowledge graph](../mcp.md) provides
complete cross-file visibility via SPARQL queries. See
[Agent Reasoning in Elle](../analysis/agent-reasoning.md) for cross-file
reasoning patterns.


## Implementation

| File | Role |
|------|------|
| `src/primitives/modules.rs` | `import-file` primitive: file I/O, compilation, execution, circular import detection, plugin caching |
| `src/plugin.rs` | `.so` plugin loading: `dlsym`, `elle_plugin_init`, primitive registration |
| `src/hir/analyze/forms.rs` | Qualified symbol desugaring (`a:b` → `(get a :b)`), projection lookup for cross-file signal inference |
| `src/hir/analyze/call.rs` | Import pattern detection, compile-time squelch inference |
| `src/hir/analyze/fileletrec.rs` | `compute_signal_projection`: extracts keyword→signal mapping from struct-returning files |
| `src/pipeline/cache.rs` | Signal projection cache (`PROJECTION_CACHE`), `get_or_compile_projection` |
| `src/reader/lexer.rs` | Qualified symbol lexing (`a:b` as single token) |
| `src/pipeline/compile.rs` | `compile_file`: file-as-letrec compilation, `include`/`include-file` splicing, projection threading |
| `tests/integration/projection.rs` | Signal projection and compile-time squelch tests |
| `tests/elle/modules.lisp` | Behavioral tests for module patterns |
| `tests/elle/include.lisp` | Behavioral tests for compile-time inclusion |
| `tests/modules/` | Module fixtures (formatter, counter, test) |
