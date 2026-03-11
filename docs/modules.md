# Modules

Elle has no module system. It has closures, structs, keyword arguments, and
a single primitive — `import` — that compiles and executes a file. Everything
else is a consequence of these four ingredients.

This document explains how modules work, why they work this way, and what
the design trades away.


## The Primitive

```
(import "path/to/file.lisp")   → value
(import "path/to/plugin.so")   → value
```

`import` (aliases: `import-file`, `module/import`) takes a string path and
returns a value. That is the entire module system API.

For `.lisp` files: the file is read, compiled via `compile_file`, and executed
on the current VM. The return value is the file's last expression.

For `.so` files: the shared library is loaded, its `elle_plugin_init` function
is called, and its return value is passed back to the caller.

Both code paths return a value. What that value is — and how the caller uses
it — is convention, not mechanism.


## Convention: Closure-as-Module

A module file defines private bindings, then exports a subset by returning a
closure that produces a struct:

```lisp
# greet.lisp
(def greeting "Hello")

(defn format-greeting [name]
  (-> greeting (append ", ") (append name) (append "!")))

(fn [] {:greet format-greeting})
```

The caller imports, calls the closure, and binds the result:

```lisp
(let ([g ((import "greet.lisp"))])
  (g:greet "world"))       # => "Hello, world!"
```

`g:greet` is qualified symbol syntax — the reader lexes `g:greet` as a single
token, and the analyzer desugars it to `(get g :greet)`. No special module
resolution; it's a struct field access.

What the closure does not return is private. `greeting` and `format-greeting`
are not visible to the caller. Encapsulation comes from lexical scope, not
from access modifiers or export declarations.


## Parametric Modules

The closure can accept arguments, making the module configurable at import
time:

```lisp
# formatter.lisp
(fn (&keys {:prefix prefix :suffix suffix :separator separator})
  (let* ([prefix    (if (nil? prefix) "" prefix)]
         [suffix    (if (nil? suffix) "" suffix)]
         [separator (if (nil? separator) ", " separator)])

    (defn wrap [s]
      (-> prefix (append s) (append suffix)))

    (defn join [items]
      (string/join (map string items) separator))

    {:wrap wrap :join join}))
```

```lisp
(let ([fmt ((import "formatter.lisp") :prefix "[" :suffix "]" :separator " | ")])
  (fmt:wrap "hello")          # => "[hello]"
  (fmt:join [1 2 3]))         # => "1 | 2 | 3"
```

Each call to the closure captures its own configuration. Two imports of the
same module with different arguments produce independent instances:

```lisp
(let ([parens  ((import "formatter.lisp") :prefix "(" :suffix ")")]
      [angles  ((import "formatter.lisp") :prefix "<" :suffix ">")])
  (parens:wrap "x")           # => "(x)"
  (angles:wrap "x"))          # => "<x>"
```

This is ML's functor pattern without any dedicated syntax.


## Import Styles

### Qualified (namespaced)

Bind the whole module, access via `mod:name`:

```lisp
(let ([json ((import "json.lisp") :pretty-indent 4)])
  (json:pretty (json:parse input)))
```

### Destructured (flat)

Pull specific names into scope:

```lisp
(def {:parse parse :pretty pretty} ((import "json.lisp") :pretty-indent 4))
(pretty (parse input))
```

### Side-effect (flat, implicit)

If a file defines top-level functions without returning a closure, `import`
executes the file for its side effects. The definitions enter the VM's global
scope:

```lisp
# helpers.lisp
(defn double [x] (* x 2))
```

```lisp
(import "helpers.lisp")
(double 21)                   # => 42
```

This is how the microgpt demo loads its helper files. It works, but it pollutes
the caller's namespace and provides no encapsulation. The closure pattern is
preferred for library code.

### Plugin (shared object)

Native plugins return a struct from `elle_plugin_init`:

```lisp
(import "target/release/libelle_random.so")
(random/int 1 100)
```

Plugins also register their primitives globally, so both qualified and
unqualified access work. The return value from `import` is a struct with
short-name keys (`:int`, `:float`, etc.), enabling the same qualified pattern:

```lisp
(let ([rng (import "target/release/libelle_random.so")])
  (rng:int 1 100))
```


## How It Works

The `import` primitive (`src/primitives/modules.rs`) does the following:

1. Read the file from disk (or load the `.so`)
2. For `.lisp`: compile via `compile_file` and execute on the current VM
   using `execute_bytecode_saving_stack` (the caller's stack frame is
   preserved and restored)
3. Return the file's last expression value (or the plugin init return value)
4. For `.lisp`: the file is compiled as a single synthetic letrec — all
   top-level forms are analyzed together, enabling mutual recursion within
   the module

Qualified symbols (`a:b:c`) are desugared by the analyzer
(`src/hir/analyze/forms.rs`) to nested `get` calls: `(get (get a :b) :c)`.
The first segment resolves as a variable; subsequent segments become keyword
arguments to `get`. This happens at compile time — no runtime symbol
resolution.


## Virtues

**No new concepts.** Modules are closures. Exports are structs. Configuration
is keyword arguments. Namespacing is struct field access. A programmer who
understands closures, structs, and destructuring already understands the
module system completely.

**Parametric by default.** Modules that accept configuration are not a special
feature — they are closures that take arguments. No functor syntax, no
module-type declarations, no type-level parameterization.

**Encapsulation from scope.** Privacy is not declared; it is structural. If a
binding is not in the returned struct, it is not accessible. There is no
`private` keyword, no visibility modifier, no friend access — and none is
needed.

**Selective import.** Destructuring gives you exactly the names you want, with
renaming for free: `(def {:parse my-parse} ((import "json.lisp")))`.

**First-class modules.** A module is a value. You can store it in a variable,
pass it to a function, put it in a data structure, return it from another
module. There is no distinction between "module" and "value" — because there
is no distinction.

**Uniform native/Elle treatment.** `.so` plugins and `.lisp` files both go
through `import` and both return values. The caller does not know or care
whether a module is implemented in Rust or in Elle.

**Replaceable in userspace.** Because `import` is just a function and modules
are just values, you can build any module discipline you want on top: module
registries, version negotiation, lazy loading, dependency injection. The
primitive does the hard part (file I/O, compilation, execution); policy is
the caller's business.


## Tensions and Trade-offs

### No caching

Every call to `import` recompiles and re-executes the file. If two modules
both `(import "utils.lisp")`, the file runs twice. This is intentional:
caching would suppress side effects and create shared mutable state between
independent callers.

The consequence is that stateful modules (those using `var` and `assign`)
get independent state per import. This is demonstrated by
`tests/modules/counter.lisp` — two imports of the same counter module
produce two independent counters. Whether this is a virtue or a cost depends
on what you are doing. It is at least predictable: `import` always means
"compile and run this file, right now."

For projects where recompilation cost matters, the solution is structural:
import once at the top level and pass the module value to the functions that
need it. This is what you would do with any expensive initialization.

### Circular import detection is runtime-only

The VM tracks which files are currently being loaded
(`vm.mark_module_loading` / `vm.is_module_loading`). If file A imports file B
which imports file A, the second `import` of A signals an error:

```
import: circular dependency detected for 'a.lisp'
```

This detection is runtime, not static. There is no compile-time module
dependency graph, no topological sort of imports. The error occurs when the
cycle is actually traversed, not when the code is compiled.

This is a direct consequence of `import` being a runtime primitive rather
than a compile-time declaration. A static module graph would require a
syntactic `import` form that the compiler processes before execution — which
would mean `import` is no longer a function, modules are no longer values,
and the compositional properties described above are lost.

The trade-off is deliberate. Circular dependencies are a design error in any
module system; Elle detects them at the point of failure rather than at the
point of declaration.

### Side-effect imports are uncontrolled

The "flat" import style (`(import "helpers.lisp")` without binding the result)
executes the file for its side effects. Top-level `defn` forms define globals.
This works but provides no encapsulation and no control over what enters the
caller's namespace.

There is no mechanism to prevent a flat import from overwriting an existing
global. There is no warning when it happens. If two flat imports define the
same name, the second wins silently.

The closure pattern eliminates this class of problem entirely: nothing enters
the caller's scope unless the caller explicitly binds it. Flat imports are
a convenience for scripts and demos, not a foundation for library code.

### No path resolution

Import paths are literal strings, resolved relative to the working directory.
There is no module search path, no package registry, no `ELLEPATH` environment
variable. `(import "lib/utils.lisp")` means exactly that file, right there.

This is simple and unambiguous but means there is no mechanism for relocatable
libraries. If you move a file, you fix every import that references it. For
the current scale of Elle projects, this is appropriate. If the language grows
a package ecosystem, path resolution becomes necessary — but building that
infrastructure before the need exists would be premature.

### Static analysis cannot see through imports

The analyzer processes one file at a time. When it encounters `(import ...)`,
it sees a function call that returns an unknown value. It cannot infer the
types, arities, or effects of the imported module's exports.

This means:
- No cross-file effect tracking (the import call itself has effect `errors()`)
- No cross-file arity checking
- No completion or hover for imported symbols in the LSP (the LSP operates on
  a single document)

This is the fundamental trade-off of runtime imports vs. static module
declarations. A static module system would enable these analyses at the cost
of the compositional properties above.


## Test Coverage

The module system is exercised by `tests/elle/modules.lisp`, which covers:

1. Basic parametric import with qualified symbol access
2. Two independent instances with different configurations
3. Default parameters (no keyword arguments)
4. Selective destructuring import
5. Module as first-class value (passed to a function)
6. Existing fixtures: value exports (`test.lisp`) and stateful modules
   (`counter.lisp`)

Additional module fixtures live in `tests/modules/`.


## Implementation

| File | Role |
|------|------|
| `src/primitives/modules.rs` | `import` primitive: file I/O, compilation, execution, circular import detection |
| `src/plugin.rs` | `.so` plugin loading: `dlsym`, `elle_plugin_init`, primitive registration |
| `src/hir/analyze/forms.rs` | Qualified symbol desugaring (`a:b` → `(get a :b)`) |
| `src/reader/lexer.rs` | Qualified symbol lexing (`a:b` as single token) |
| `src/pipeline/compile.rs` | `compile_file`: file-as-letrec compilation |
| `tests/elle/modules.lisp` | Behavioral tests for module patterns |
| `tests/modules/` | Module fixtures (formatter, counter, test) |
