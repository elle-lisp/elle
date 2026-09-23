# Coming from Other Languages

<!-- audited: 2026-09-23 -->

Quick orientation for programmers arriving from specific languages.

Each section highlights the key differences and maps familiar concepts to
their Elle equivalents. The fence under each table runs the Elle column's
claims as assertions. All of the fences run as one program, top to bottom.

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
| `x = 5` | `(def @x 5)` | `@` makes a binding mutable; `(var x 5)` is the same |
| `x = 10` | `(assign x 10)` | Not `set` — `set` creates a set |
| `[x*2 for x in lst]` | `(map (fn [x] (* x 2)) lst)` | No comprehension syntax |
| `dict(a=1)` | `{:a 1}` | Keywords as keys, not strings |
| `d["key"]` | `d:key` or `(d :key)` | Structs are callable |
| `None` | `nil` | Falsy, but `()` (empty list) is truthy |
| `True / False` | `true / false` | Same semantics |
| `import os` | `(import "std/module")` | Returns a value, not a side effect |
| `#` comment | `#` comment | Same |
| `try/except` | `(protect ...)` or `(try ...)` | Signal-based, not exception-based |
| `async/await` | fibers | No function coloring — any function can yield |
| `pip install` | `(import "plugin/name")` | Plugins are Rust shared libraries, not packages |

```lisp
(let [@x 5]
  (assign x 10)
  (assert (= x 10)))
(assert (= (set 1 2) |1 2|))                        # set builds a set
(assert (= (map (fn [x] (* x 2)) [1 2 3]) [2 4 6]))
(let [d {:key 1}]
  (assert (= d:key 1))
  (assert (= (d :key) 1)))                          # a struct is callable
(assert (not nil))
(assert ())                                         # the empty list is truthy
(let [[ok? err] (protect (error {:error :oops :message "caught"}))]
  (assert (not ok?))
  (assert (= err:error :oops)))
```

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
| `let x = 5` | `(def @x 5)` | Mutable binding |
| `{a: 1, b: 2}` | `{:a 1 :b 2}` | Keywords, not string keys |
| `obj.key` | `obj:key` | Colon accessor |
| `[1, 2, 3]` | `[1 2 3]` | No commas — spaces separate |
| `arr.map(f)` | `(map f arr)` | Function first, not method |
| `() => x` | `(fn [] x)` | No arrow syntax |
| `async/await` | fibers | Cooperative, not promise-based |
| `import x from` | `(def x ((import "std/...")))` | Module = closure → struct |
| `null / undefined` | `nil` | One bottom value |
| `===` | `=` | `=` compares values; `identical?` is `=` without numeric coercion |
| `// comment` | `# comment` | |

```lisp
(assert (= {:a 1 :b 2} {:b 2 :a 1}))
(assert (= ((fn [] 42)) 42))
(let [[ok? err] (protect (+ 1 "2"))]
  (assert (not ok?))                                # no implicit coercion
  (assert (= err:error :type-error)))
(assert (= 1 1.0))                                  # = compares numbers by value
(assert (not (identical? 1 1.0)))
```

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
| `let mut x = 5` | `(def @x 5)` | Mutable |
| `match x { ... }` | `(match x ...)` | Similar but no borrow checker |
| `Result<T, E>` | Signal system | Errors propagate via `:error` signal |
| `async fn` | Just `fn` | No coloring — fibers handle suspension |
| `Vec<T>` | `@[...]` | Mutable array |
| `HashMap` | `@{:k v}` | Mutable struct |
| `trait` | Traits (`with-traits`) | Per-value, not per-type |
| `impl` | Closures | No methods — functions take the struct as arg |
| `use crate::` | `(import "std/...")` | |
| Ownership/borrowing | Regions | The compiler frees each value's region where the value dies ([regions.md](regions.md)) |

```lisp
(let [v @[1 2]]
  (push v 3)
  (assert (= v @[1 2 3])))
(let [m @{:k 1}]
  (put m :k 2)
  (assert (= m:k 2)))
(assert (= (match [1 2] [a b] (+ a b)) 3))
```

**Watch out for:**
- No static types — type errors are runtime errors.
- No borrow checker and no moves — a value lives in its region until the
  compiler's analysis says it dies, and a store, a return or a yield
  hands over the same value rather than a copy.
- `@` prefix means mutable, not dereference.

## Go

**You'll feel at home with:** goroutine-like concurrency (fibers),
simple error handling, practical stdlib.

**Key differences:**

| Go | Elle | Notes |
|----|------|-------|
| `x := 5` | `(def @x 5)` | |
| `func f(x int) int` | `(defn f [x] ...)` | No type annotations |
| `go f()` | `(ev/spawn f)` | Fiber, not OS thread |
| `<-ch` | `(chan/select @[rx])` | Waits; `(chan/recv rx)` answers `[:empty]` at once |
| `err != nil` | `(protect ...)` | No sentinel errors |
| `struct{}` | `{:field val}` | No methods on structs |
| `interface` | Closures/traits | |
| `import "fmt"` | `(import "std/...")` | |
| `// comment` | `# comment` | |

```lisp
(assert (= (ev/join (ev/spawn (fn [] 7))) 7))
(let [[tx rx] (chan)]
  (assert (= (chan/recv rx) [:empty]))             # chan/recv does not wait
  (chan/send tx 42)
  (assert (= (chan/select @[rx]) [0 42])))         # [index message]
```

**Watch out for:**
- No goroutine preemption — fibers yield cooperatively.
- No zero values — every binding takes a value when it is made.
- No struct methods — pass the struct to a function.

## Clojure

**You'll feel at home with:** persistent data structures, keyword
keys, functional style, REPL, macros, seq abstraction.

**Key differences:**

| Clojure | Elle | Notes |
|---------|------|-------|
| `(def x 5)` | `(def x 5)` | Same |
| `(defn f [x] ...)` | `(defn f [x] ...)` | Same |
| `{:a 1}` | `{:a 1}` | Same — but iteration order is not insertion order |
| `[1 2 3]` | `[1 2 3]` | Immutable array (not a vector) |
| `(:key m)` | `m:key` | Keywords aren't callable — use accessor syntax |
| `(atom x)` | `(def @x ...)` | A mutable binding; `assign` updates it |
| `@atom` | just `x` | No deref — mutable bindings read directly |
| `(swap! a f)` | `(assign x (f x))` | |
| `nil` | `nil` | Same — but `()` is truthy in Elle |
| `(require '[...])` | `(import "std/...")` | |
| `core.async` | Fibers | Built-in, not a library |
| `;` comment | `#` comment | `;` is splice in Elle |
| `(seq coll)` | `(->list coll)` | |

```lisp
(let [[ok? _] (protect (:key {:key 1}))]
  (assert (not ok?)))                               # a keyword is not callable
(let [@counter 0]
  (assign counter (inc counter))
  (assert (= counter 1)))
(assert (= (->list [1 2 3]) (list 1 2 3)))
(assert (= [1 ;[2 3]] [1 2 3]))                     # ; splices
```

**Watch out for:**
- `()` is truthy. Use `empty?` not `nil?` for end-of-list.
- `;` is splice, not comment.
- No lazy sequences — use streams (`stream/map`, `stream/filter`).
- A struct keeps its entries in key order, not insertion order: an
  immutable struct is a sorted array, a `@struct` a `BTreeMap`.

## Common Lisp / Scheme

**You'll feel at home with:** S-expressions, `cons`/`car`/`cdr`
(called `pair`/`first`/`rest`), macros, tail-call optimization, REPL.

**Key differences:**

| CL/Scheme | Elle | Notes |
|-----------|------|-------|
| `(defun f (x) ...)` | `(defn f [x] ...)` | Brackets for params |
| `(setf x 5)` | `(assign x 5)` | |
| `(cons a b)` | `(pair a b)` | |
| `(car x)` | `(first x)` | |
| `(cdr x)` | `(rest x)` | |
| `#t / #f` | `true / false` | |
| `(lambda (x) ...)` | `(fn [x] ...)` | |
| `;` comment | `#` comment | `;` is splice |
| `(defmacro ...)` | `(defmacro ...)` | Hygienic (Racket-style scope sets) |
| Multiple return | Destructuring | `(def [a b] (f))` |
| `(values 1 2)` | `[1 2]` | Return an array, destructure it |
| Lisp-1 vs Lisp-2 | Lisp-1 | Single namespace |
| CLOS | Closures + traits | No object system |
| `call/cc` | Fibers | Structured, not arbitrary continuations |

```lisp
(assert (= (first (pair 1 (list 2))) 1))
(assert (= (rest (list 1 2)) (list 2)))
(defn two-values [] [1 2])
(def [lo hi] (two-values))                          # destructure the "values"
(assert (= (+ lo hi) 3))
(let [list 5] (assert (= list 5)))                  # one namespace: list is shadowed
```

**Watch out for:**
- `()` is truthy — this is intentional. `nil` is the false/absent value; `()` is an empty list (a valid value).
- `#` starts a comment, so `#t` is a comment, not a boolean.
- No `set!` — it's `assign`. `set` creates a set literal.
- Macros are hygienic by default (scope sets, not `gensym` hacks).

## Erlang / Elixir

**You'll feel at home with:** the process model, message passing,
supervisors, pattern matching, immutable-by-default data.

**Key differences:**

| Erlang/Elixir | Elle | Notes |
|---------------|------|-------|
| `spawn(fun)` | `(process:spawn f)` | Fibers, not OS processes |
| `Pid ! Msg` | `(process:send pid msg)` | Via [lib/process.lisp](../lib/process.lisp) |
| `receive ... end` | `(process:recv)` | Via the process module; `process:recv-match` filters |
| `gen_server` | `(process:gen-server-start-link callbacks arg)` | Pure Elle ([behaviors.md](behaviors.md)) |
| `supervisor` | `(process:supervisor-start-link children)` | Same |
| `=` (match) | `(match x ...)` | `=` is equality in Elle |
| `[H\|T]` | `(pair h t)`, or `[h ;t]` for an array | |
| `#{k => v}` | `{:k v}` | Structs, not maps |
| `fun(X) -> ...` | `(fn [x] ...)` | |
| `-module(m).` | `(fn [] {:f f ...})` | Modules are closures |
| Atoms | Keywords (`:atom`) | |
| Binary `<<>>` | `(bytes ...)` / `b[...]` | |
| Hot code reload | Not supported | |

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (let [me (process:self)
        echo (process:spawn (fn []
               (match (process:recv)
                 [from msg] (process:send from [:echo msg]))))]
    (process:send echo [me :hi])
    (assert (= (process:recv) [:echo :hi])))))

(process:start (fn []
  (let [adder (process:gen-server-start-link
                {:init (fn [n] n)
                 :handle-call (fn [req from n] [:reply (+ n req) (+ n req)])}
                10)]
    (assert (= (process:gen-server-call adder 5) 15)))))

(assert (= (pair 1 (list 2 3)) (list 1 2 3)))       # [H|T]
```

**Watch out for:**
- A fiber from `ev/spawn` is cooperative. A process runs on the process
  scheduler, which preempts it when its fuel budget runs out
  ([processes.md](processes.md)).
- No distributed Erlang — single-process only.
- Links and monitors follow Erlang's model. Links and supervisor restarts
  have open defects under the
  [processes](https://github.com/elle-lisp/elle/issues?q=is%3Aopen+label%3Aprocesses)
  label.

## Janet

**You'll feel at home with:** almost everything. Elle shares Janet's
philosophy: practical, batteries-included, modern Lisp syntax, struct
literals, mutable/immutable split, C FFI, single-binary deployment.
Janet's fibers are the model Elle started from; Elle pushed further on
static analysis, concurrency, and compilation.

**Key differences:**

| Janet | Elle | Notes |
|-------|------|-------|
| `(def x 5)` | `(def x 5)` | Same |
| `(var x 5)` | `(var x 5)` | Same; `(def @x 5)` is the other spelling |
| `(set x 10)` | `(assign x 10)` | `set` creates a set in Elle |
| `(defn f [x] ...)` | `(defn f [x] ...)` | Same |
| `(fn [x] ...)` | `(fn [x] ...)` | Same |
| `@[1 2 3]` | `@[1 2 3]` | Same — mutable array |
| `[1 2 3]` | `[1 2 3]` | Same — immutable |
| `@{:a 1}` | `@{:a 1}` | Same — mutable struct |
| `{:a 1}` | `{:a 1}` | Same — immutable struct |
| `(get ds :k)` | `ds:k` | Colon accessor syntax; `get` works too |
| `(ev/spawn f)` | `(ev/spawn f)` | Both have structured concurrency |
| `(fiber/new f)` | `(fiber/new f mask)` | Both have fibers |
| `#` comment | `#` comment | Same |
| `(import mod)` | `(import "std/mod")` | String path, returns a value |
| PEG | `(import "plugin/regex")` | No built-in PEG; regex plugin |
| `(os/shell ...)` | `(subprocess/system ...)` | |
| Dynamic binding | `(make-parameter)` | Racket-style parameters |

```lisp
(let [ds @{:k 1}]
  (assert (= ds:k (get ds :k))))
(def depth (make-parameter 0))
(assert (= (parameterize ((depth 1)) (depth)) 1))
(assert (= (depth) 0))                              # the binding ended with its scope
```

**What Elle adds beyond Janet:**
- **Signal system.** Compile-time inference of which functions can error,
  yield, or do I/O. Janet has no equivalent — effects are invisible.
- **Hygienic macros.** Racket-style scope sets, not `gensym` discipline.
- **Deep static analysis.** Binding resolution, capture analysis, escape
  analysis, and lint passes before execution.
- **Deterministic memory.** No GC — every value lives in a region the
  compiler frees where the value dies. Janet uses a tracing GC.
- **JIT compilation.** Hot functions compile to native code via
  Cranelift. Janet interprets bytecode.
- **Process model.** Erlang-style GenServer/Supervisor/Actor in pure Elle.
- **FFI from the language.** `ffi/defbind` in the prelude, `ffi/call` as
  a primitive — no C glue code.

**Watch out for:**
- `set` creates a set literal, not mutation. Use `assign`.
- `;` is splice, not comment.
- Modules are closures that return structs — call them: `((import "std/x"))`.

## C

**You'll feel at home with:** FFI (Elle calls C directly), manual
memory management (when you need it), pointer arithmetic.

**Key differences:**

| C | Elle | Notes |
|---|------|-------|
| `int x = 5;` | `(def @x 5)` | Dynamic typing |
| `malloc/free` | `(ffi/malloc n)` / `(ffi/free p)` | For FFI only |
| `struct` | `{:field val}` | No field declarations |
| `#include` | `(include-file "file.lisp")` | Compile-time splice ([modules.md](modules.md)) |
| `printf` | `(println ...)` | `string/format` for formatting |
| `dlopen` | `(ffi/native "lib.so")` | `(ffi/native nil)` is the running process |
| Function pointer | `(ffi/callback sig fn)` | |

Elle wraps C libraries directly via FFI — no binding generators,
no wrapper crates. See [docs/ffi.md](ffi.md) and the
[lib/sqlite.lisp](../lib/sqlite.lisp), [lib/compress.lisp](../lib/compress.lisp)
and [lib/git.lisp](../lib/git.lisp) modules for real-world examples.

```lisp
(def libc (ffi/native nil))
(ffi/defbind c-getpid libc "getpid" :int @[])
(assert (= (c-getpid) (sys/pid)))

(let [buf (ffi/malloc 16)]
  (ffi/free buf))
```
