# Coming from Other Languages

Quick orientation for programmers arriving from specific languages.
Each section highlights the key differences and maps familiar concepts
to their Elle equivalents.

## Contents

- [Python](#python)
- [JavaScript / TypeScript](#javascript--typescript)
- [Rust](#rust)
- [Go](#go)
- [Clojure](#clojure)
- [Common Lisp / Scheme](#common-lisp--scheme)
- [Erlang / Elixir](#erlang--elixir)
- [Janet](#janet)
- [C](#c)

---

## Python

**You'll feel at home with:** dynamic typing, first-class functions,
list comprehension patterns (via `map`/`filter`), REPL-driven
development, keyword arguments.

**Key differences:**

| Python | Elle | Notes |
|--------|------|-------|
| `def f(x):` | `(defn f [x] ...)` | Parens wrap the whole form |
| `x = 5` | `(var x 5)` | `var` is mutable, `def` is immutable |
| `x = 10` | `(assign x 10)` | Not `set` — `set` creates a set |
| `[x*2 for x in lst]` | `(map (fn [x] (* x 2)) lst)` | No comprehension syntax |
| `dict(a=1)` | `{:a 1}` | Keywords as keys, not strings |
| `d["key"]` | `d:key` or `(d :key)` | Structs are callable |
| `None` | `nil` | Falsy, but `()` (empty list) is truthy |
| `True / False` | `true / false` | Same semantics |
| `import os` | `(import "std/module")` | Returns a value, not a side effect |
| `#` comment | `##` comment | `#` is reserved for reader macros |
| `try/except` | `(protect ...)` or `(try ...)` | Signal-based, not exception-based |
| `async/await` | fibers | No function coloring — any function can yield |
| `pip install` | `(import "plugin/name")` | Plugins are .so files, not packages |

**Watch out for:**
- `nil` vs `()` — both exist, they're different. `nil` is falsy,
  `()` is truthy. Use `empty?` to test end-of-list.
- Semicolons are splice, not statement separators.
- No classes or inheritance — use structs + closures.

## JavaScript / TypeScript

**You'll feel at home with:** closures, first-class functions,
prototype-less objects (structs), `const`/`let` distinction.

**Key differences:**

| JS/TS | Elle | Notes |
|-------|------|-------|
| `const x = 5` | `(def x 5)` | Immutable binding |
| `let x = 5` | `(var x 5)` | Mutable binding |
| `{a: 1, b: 2}` | `{:a 1 :b 2}` | Keywords, not string keys |
| `obj.key` | `obj:key` | Colon accessor |
| `[1, 2, 3]` | `[1 2 3]` | No commas — spaces separate |
| `arr.map(f)` | `(map f arr)` | Function first, not method |
| `() => x` | `(fn [] x)` | No arrow syntax |
| `async/await` | fibers | Cooperative, not promise-based |
| `import x from` | `(def x ((import "std/...")))` | Module = closure → struct |
| `null / undefined` | `nil` | One bottom value |
| `===` | `=` | `=` is always value equality; `identical?` for reference |
| `// comment` | `## comment` | |

**Watch out for:**
- No implicit coercion. `(+ 1 "2")` is an error, not `"12"`.
- No `this` — closures capture explicitly.
- No prototype chain — structs are flat dictionaries.

## Rust

**You'll feel at home with:** the ownership mindset (immutable by
default, mutable opt-in), pattern matching, result-based error
handling, the compilation pipeline.

**Key differences:**

| Rust | Elle | Notes |
|------|------|-------|
| `let x = 5` | `(def x 5)` | Immutable |
| `let mut x = 5` | `(var x 5)` | Mutable |
| `match x { ... }` | `(match x ...)` | Similar but no borrow checker |
| `Result<T, E>` | Signal system | Errors propagate via `:error` signal |
| `async fn` | Just `fn` | No coloring — fibers handle suspension |
| `Vec<T>` | `@[...]` | Mutable array |
| `HashMap` | `@{:k v}` | Mutable struct |
| `trait` | Traits (`with-traits`) | Per-value, not per-type |
| `impl` | Closures | No methods — functions take the struct as arg |
| `use crate::` | `(import "std/...")` | |
| Ownership/borrowing | Rc + scope analysis | No borrow checker; Rc with scope-based reclamation |

**Watch out for:**
- No static types — type errors are runtime errors.
- No ownership transfer — values are Rc'd, not moved.
- `@` prefix means mutable, not dereference.

## Go

**You'll feel at home with:** goroutine-like concurrency (fibers),
simple error handling, practical stdlib.

**Key differences:**

| Go | Elle | Notes |
|----|------|-------|
| `x := 5` | `(var x 5)` | |
| `func f(x int) int` | `(defn f [x] ...)` | No type annotations |
| `go f()` | `(ev/spawn f)` | Fiber, not OS thread |
| `<-ch` | `(chan/recv ch)` | Channels exist too |
| `err != nil` | `(protect ...)` | No sentinel errors |
| `struct{}` | `{:field val}` | No methods on structs |
| `interface` | Closures/traits | |
| `import "fmt"` | `(import "std/...")` | |
| `// comment` | `## comment` | |

**Watch out for:**
- No goroutine preemption — fibers yield cooperatively.
- No zero values — uninitialized variables are `nil`.
- No struct methods — pass the struct to a function.

## Clojure

**You'll feel at home with:** persistent data structures, keyword
keys, functional style, REPL, macros, seq abstraction.

**Key differences:**

| Clojure | Elle | Notes |
|---------|------|-------|
| `(def x 5)` | `(def x 5)` | Same |
| `(defn f [x] ...)` | `(defn f [x] ...)` | Same |
| `{:a 1}` | `{:a 1}` | Same — but Elle structs are ordered |
| `[1 2 3]` | `[1 2 3]` | Immutable array (not a vector) |
| `(:key m)` | `m:key` | Keywords aren't callable — use accessor syntax |
| `(atom x)` | `(var x ...)` | `var` is mutable, `assign` updates |
| `@atom` | just `x` | No deref — mutable vars read directly |
| `(swap! a f)` | `(assign x (f x))` | |
| `nil` | `nil` | Same — but `()` is truthy in Elle |
| `(require '[...])` | `(import "std/...")` | |
| `core.async` | Fibers | Built-in, not a library |
| `;` comment | `##` comment | `;` is splice in Elle |
| `(seq coll)` | `(->list coll)` | |

**Watch out for:**
- `()` is truthy. Use `empty?` not `nil?` for end-of-list.
- `;` is splice, not comment.
- No lazy sequences — use streams (`stream/map`, `stream/filter`).
- Structs are ordered (BTreeMap), not hash maps.

## Common Lisp / Scheme

**You'll feel at home with:** S-expressions, `cons`/`car`/`cdr`
(called `first`/`rest`), macros, tail-call optimization, REPL.

**Key differences:**

| CL/Scheme | Elle | Notes |
|-----------|------|-------|
| `(defun f (x) ...)` | `(defn f [x] ...)` | Brackets for params |
| `(setf x 5)` | `(assign x 5)` | |
| `(car x)` | `(first x)` | |
| `(cdr x)` | `(rest x)` | |
| `#t / #f` | `true / false` | |
| `(lambda (x) ...)` | `(fn [x] ...)` | |
| `;` comment | `##` comment | `;` is splice |
| `(defmacro ...)` | `(defmacro ...)` | Hygienic (Racket-style scope sets) |
| Multiple return | Destructuring | `(def [a b] (f))` |
| `(values 1 2)` | `[1 2]` | Return an array, destructure it |
| Lisp-1 vs Lisp-2 | Lisp-1 | Single namespace |
| CLOS | Closures + traits | No object system |
| `call/cc` | Fibers | Structured, not arbitrary continuations |

**Watch out for:**
- `()` is truthy — this is intentional. `nil` is the false/absent value; `()` is an empty list (a valid value).
- `#` starts reader syntax, not booleans.
- No `set!` — it's `assign`. `set` creates a set literal.
- Macros are hygienic by default (scope sets, not `gensym` hacks).

## Erlang / Elixir

**You'll feel at home with:** the process model, message passing,
supervisors, pattern matching, immutable-by-default data.

**Key differences:**

| Erlang/Elixir | Elle | Notes |
|---------------|------|-------|
| `spawn(fun)` | `(ev/spawn f)` | Fibers, not OS processes |
| `Pid ! Msg` | `(send pid msg)` | Via process module |
| `receive ... end` | `(recv ...)` | Via process module |
| `gen_server` | `(process:gen-server ...)` | Pure Elle, in `lib/process.lisp` |
| `supervisor` | `(process:supervisor ...)` | Same |
| `=` (match) | `(match x ...)` | `=` is equality in Elle |
| `[H\|T]` | `(cons h t)` or `[h ; t]` | |
| `#{k => v}` | `{:k v}` | Structs, not maps |
| `fun(X) -> ...` | `(fn [x] ...)` | |
| `-module(m).` | `(fn [] {:f f ...})` | Modules are closures |
| Atoms | Keywords (`:atom`) | |
| Binary `<<>>` | `(bytes ...)` / `b[...]` | |
| Hot code reload | Not supported | |

**Watch out for:**
- Fibers are cooperative, not preemptive (but `:fuel` signal enables
  budget-based preemption via the scheduler).
- No distributed Erlang — single-process only.
- Process linking and monitoring work the same conceptually.

## Janet

**You'll feel at home with:** almost everything. Elle shares Janet's
philosophy: practical, batteries-included, modern Lisp syntax, struct
literals, mutable/immutable split, C FFI, single-binary deployment.
Elle started from a similar place and pushed further on static analysis,
concurrency, and compilation.

**Key differences:**

| Janet | Elle | Notes |
|-------|------|-------|
| `(def x 5)` | `(def x 5)` | Same |
| `(var x 5)` | `(var x 5)` | Same |
| `(set x 10)` | `(assign x 10)` | `set` creates a set in Elle |
| `(defn f [x] ...)` | `(defn f [x] ...)` | Same |
| `(fn [x] ...)` | `(fn [x] ...)` | Same |
| `@[1 2 3]` | `@[1 2 3]` | Same — mutable array |
| `[1 2 3]` | `[1 2 3]` | Same — immutable |
| `@{:a 1}` | `@{:a 1}` | Same — mutable struct |
| `{:a 1}` | `{:a 1}` | Same — immutable struct |
| `(get ds :k)` | `ds:k` | Colon accessor syntax |
| `(ev/spawn f)` | `(ev/spawn f)` | Both have structured concurrency |
| `(fiber/new f)` | `(fiber/new f)` | Both have fibers |
| `#` comment | `##` comment | Double hash |
| `(import mod)` | `(import "std/mod")` | String path, returns a value |
| PEG | `(import "plugin/regex")` | No built-in PEG; regex plugin |
| `(os/shell ...)` | `(subprocess/system ...)` | |
| Dynamic binding | `(make-parameter)` | Racket-style parameters |

**What Elle adds beyond Janet:**
- **Signal system.** Compile-time inference of which functions can error,
  yield, or do I/O. Janet has no equivalent — effects are invisible.
- **Hygienic macros.** Racket-style scope sets, not `gensym` discipline.
- **Deep static analysis.** Binding resolution, capture analysis, escape
  analysis, and lint passes before execution.
- **Deterministic memory.** No GC — scope-based reclamation + per-fiber
  heaps. Janet uses a tracing GC.
- **JIT compilation.** Silent functions compile to native code via
  Cranelift. Janet interprets bytecode.
- **WASM backend.** Entire modules compile to WebAssembly.
- **Process model.** Erlang-style GenServer/Supervisor/Actor in pure Elle.
- **FFI from the language.** `ffi/defbind` in the prelude, `ffi/call` as
  a primitive — no C glue code. Janet requires C extensions.

**Watch out for:**
- `set` creates a set literal, not mutation. Use `assign`.
- `##` for comments, not `#`. Single `#` is reader syntax.
- `;` is splice, not comment.
- Modules are closures that return structs — call them: `((import "std/x"))`.

## C

**You'll feel at home with:** FFI (Elle calls C directly), manual
memory management (when you need it), pointer arithmetic.

**Key differences:**

| C | Elle | Notes |
|---|------|-------|
| `int x = 5;` | `(var x 5)` | Dynamic typing |
| `malloc/free` | `(ffi/malloc n)` / `(ffi/free p)` | For FFI only |
| `struct` | `{:field val}` | No field declarations |
| `#include` | `(include "file.lisp")` | Compile-time splice |
| `printf` | `(println ...)` | `string/format` for formatting |
| `dlopen` | `(ffi/native "lib.so")` | |
| Function pointer | `(ffi/callback fn sig)` | |

Elle wraps C libraries directly via FFI — no binding generators,
no wrapper crates. See [docs/ffi.md](ffi.md) and the `lib/sqlite.lisp`,
`lib/compress.lisp`, `lib/git.lisp` modules for real-world examples.

```lisp
(def libc (ffi/native "libc.so.6"))
(ffi/defbind c-getpid libc "getpid" :int @[])
(println (c-getpid))
```
