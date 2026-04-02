# Elle

[![CI](https://github.com/elle-lisp/elle/actions/workflows/main.yml/badge.svg)](https://github.com/elle-lisp/elle/actions/workflows/main.yml)

Elle is a Lisp. What separates it from other Lisps is the depth of its static analysis: full binding resolution, capture analysis, and signal inference happen at compile time, before any code runs. This gives Elle a sound signal system, fully hygienic macros, colorless concurrency via fibers, and deterministic memory management — all derived from the same analysis pass.

## Contents

- [What Makes Elle Different](#what-makes-elle-different)
- [Language](#language)
- [Types](#types)
- [Control Flow](#control-flow)
- [Concurrency](#concurrency)
- [Memory](#memory)
- [Execution Backends](#execution-backends)
- [FFI](#ffi)
- [Modules](#modules)
- [Plugins](#plugins)
- [Epochs](#epochs)
- [Tooling](#tooling)
- [Getting Started](#getting-started)
- [License](#license)

## What Makes Elle Different

- **Fibers are the concurrency primitive.** A fiber is an independent execution context — its own stack, call frames, signal mask, and heap. Fibers are cooperative and explicitly resumed. The parent drives execution by calling `fiber/resume`. When a fiber emits a signal, it suspends and the parent decides what to do next.

  Fibers run as coroutines. A parent spawns a child, drives it step by step, and reads each yielded value:

  ```lisp
  (defn produce []
    (emit :yield 1)
    (emit :yield 2)
    (emit :yield 3))

  (def f (fiber/new produce |:yield|))

  (fiber/resume f) (print (fiber/value f))  # => 1
  (fiber/resume f) (print (fiber/value f))  # => 2
  (fiber/resume f) (print (fiber/value f))  # => 3
  ```

  When a fiber finishes, its entire heap is freed in O(1) — no GC pause, no reference counting.

- **Signals are typed, cooperative flow-control interrupts.** A signal is a keyword — `:error`, `:log`, `:abort`, or any user-defined name — that a fiber emits to its parent. The parent's signal mask determines which signals surface; unmasked signals propagate further up. The compiler infers which functions can emit signals and enforces that silent contexts don't call yielding ones.

  **Error handling** — a fiber signals an error; the parent catches it:

  ```lisp
  (defn risky [x]
    (if (< x 0)
      (error {:error :bad-input :message "negative input"})
      (* x x)))

  (def f (fiber/new (fn () (risky -1)) |:error|))
  (fiber/resume f)

  (if (= (fiber/status f) :paused)
    (print "caught:" (fiber/value f))   # => caught: {:error :bad-input ...}
    (print "result:" (fiber/value f)))
  ```

  **Yielding** — a fiber yields progress updates; the parent drives it to completion:

  ```lisp
  (defn process-items [items]
    (each item items
      (emit :progress {:item item :result (* item item)})))

  (def f (fiber/new (fn () (process-items [1 2 3])) |:progress|))

  (forever
    (fiber/resume f)
    (if (= (fiber/status f) :paused)
      (print "progress:" (fiber/value f))
      (break)))
  ```

  **Parent/child** — a fiber spawns a child and collects its log signals:

  ```lisp
  (defn child []
    (emit :log "child starting")
    (emit :log "child done")
    42)

  (defn parent []
    (def f (fiber/new child |:log|))
    (forever
      (fiber/resume f)
      (match (fiber/status f)
        (:paused (print "log:" (fiber/value f)))
        (_ (break))))
    (fiber/value f))  # => 42

  (parent)
  ```

  See [docs/signals/](docs/signals/) for the full signal system: user-defined signals, `silence` for callback sandboxing, and composed signal masks.

- **Static analysis is a first-class feature.** The compiler performs full binding resolution, capture analysis, signal inference, and lint passes before any code runs. This is not optional tooling bolted on — it is the compilation pipeline. Most Lisps are dynamic; Elle knows at compile time what every binding refers to, what every closure captures, and what signals every function can emit.

- **A sound signal system, inferred not declared.** Every function is automatically classified as `Silent`, `Yields`, or `Polymorphic`. The compiler enforces this: a silent context cannot call a yielding function. No annotations required.

  ```lisp
  # Silent — inferred automatically
  (defn add (a b) (+ a b))

  # Yields — inferred from emit call
  (defn fetch-data (url)
    (emit :http-request url)
    (emit :http-wait))

  # Polymorphic — signal depends on the callback
  (defn map-signal (f xs)
    (map f xs))  # signal = signal of f
  ```

- **Fully hygienic macros that operate on syntax objects, not text or s-expressions.** Macros receive and return `Syntax` objects carrying scope information (Racket-style scope sets). Name capture is structurally impossible, not just conventionally avoided.

  ```lisp
  (defmacro my-swap (a b)
    `(let ((tmp ,a)) (assign ,a ,b) (assign ,b tmp)))

  (let ((tmp 100) (x 1) (y 2))
    (my-swap x y)
    tmp)  # => 100, not 1
  ```

  The `tmp` introduced by the macro does not shadow the caller's `tmp`. This is guaranteed by scope sets, not by convention.

- **Functions are colorless.** Any function can be called from a fiber. There is no `async`/`await` annotation that marks a function as suspending and forces all its callers to be marked too. Whether something runs concurrently is decided at the call site, not baked into the function definition. In Rust/JS/Python, a suspending `fetch` forces every caller to be `async` too; in Elle, the signal is inferred by the compiler and callers are unaffected.

- **Erlang-style processes fall out of the fiber model.** The same fibers that drive coroutines and I/O compose into a full process system: mailboxes, links, monitors, named registration, supervisors, and GenServers — implemented entirely in Elle as [`lib/process.lisp`](lib/process.lisp). No VM changes, no special runtime support. A supervisor is a process that traps exits and restarts children; a GenServer is a process in a receive loop with call/cast dispatch. The signal system makes this possible: `yield` delivers scheduler commands, `:error` propagates crashes through links, `:fuel` enables preemptive scheduling, and `:io` lets processes do async I/O without blocking the scheduler.

  ```lisp
  (def process ((import "lib/process")))

  (process:start (fn []
    # Start a supervised key-value server
    (process:supervisor-start-link
      [{:id :kv :restart :permanent
        :start (fn []
          (process:gen-server-start-link
            {:init        (fn [_] @{})
             :handle-call (fn [req _from state]
               (match req
                 ([:get k]   [:reply (get state k) state])
                 ([:put k v] (put state k v)
                             [:reply :ok state])))}
            nil :name :kv))}]
      :name :sup
      :max-restarts 3)

    (process:gen-server-call :kv [:put :lang "elle"])
    (process:gen-server-call :kv [:get :lang])))  # => "elle"
  # If the kv server crashes, the supervisor restarts it automatically.
  ```

  This is what Elle's design is for: fibers provide the mechanism, signals provide the control flow, and user-space libraries provide the policy. See [`docs/processes.md`](docs/processes.md) for the full API.

- **The Rust ecosystem.** FFI without ceremony. Native plugins as Rust cdylib crates. Values are marshalled directly to C types via libffi — no intermediate serialization format, no separate process, no generated bindings.

## Language

- **Modern Lisp syntax with no parser ambiguity.** Macros operate on syntax trees, not text. See [`prelude.lisp`](prelude.lisp) for hygienic macros and standard forms.

- **Collection literals with mutable/immutable split.** Bare delimiters are immutable: `[1 2 3]` (array), `{:key val}` (struct), `"hello"` (string). `@`-prefixed are mutable: `@[1 2 3]` (@array), `@{:key val}` (@struct), `@"hello"` (@string).

   ```lisp
   # Immutable
   (def a [1 2 3])           # array
   (def s {:name "Bob"})     # struct
   (def str "hello")         # string
   (def s |1 2 3|)           # set

   # Mutable
   (def a @[1 2 3])          # @array
   (def tbl @{:name "Bob"})  # @struct
   (def buf @"hello")        # @string
   (def ms @|1 2 3|)         # @set

   # Bytes and @bytes
   (def b b[1 2 3])           # bytes
   (def bl @b[1 2 3])         # @bytes
   ```

- **Strings are sequences of grapheme clusters.** `length`, slicing, indexing, and iteration all count grapheme clusters — not bytes, not codepoints.

  ```lisp
  (length "café")           # => 4, not 5 bytes
  (get "café" 3)              # => "é"
  (slice "café" 0 2)        # => "ca"
  (first "café")            # => "c"
  (rest "café")             # => "afé"
  (length "👨‍👩‍👧")   # => 1
  ```

- **Destructuring in all binding positions.** `def`, `let`, `let*`, `var`, `fn` parameters, `match` patterns — missing values and wrong types are runtime errors.

  ```lisp
  (def (head & tail) (list 1 2 3 4))
  (def [x _ z] [10 20 30])
  (def {:name n :age a} {:name "Bob" :age 25})
  (def {:config {:db {:host h}}}
    {:config {:db {:host "localhost"}}})
  ```

- **Closures with automatic capture analysis.** The compiler tracks which variables each closure captures. Mutable captures use cells automatically. Enables escape analysis for scope-level memory reclamation.

  ```lisp
  (defn make-counter [start]
    (var n start)
    (fn []
      (assign n (+ n 1))
      n))

  (def c (make-counter 0))
  (c)  # => 1
  (c)  # => 2
  ```

   <details><summary>More: Automatic LBox Wrapping</summary>

   The closure captures `n` by value. The compiler detects that `n` is mutated, so it wraps it in an lbox automatically. No explicit `box` or `ref` needed.
   </details>

- **Full tail-call optimisation.** All tail calls are optimised — not just self-recursion. Mutually recursive functions, continuation-passing style, and trampolining all work without stack overflow.

- **Splice operator for array spreading.** `;expr` marks a value for spreading at call sites and in data constructors. `(splice expr)` is the long form.

  ```lisp
  (def args @[2 3])
  (+ 1 ;args)  # => 6, same as (+ 1 2 3)

  (def items @[1 2])
  @[0 ;items 3]  # => @[0 1 2 3]
  ```

- **Reader macros for quasiquote and unquote.** `` ` `` for quasiquote, `,` for unquote, `,;` for unquote-splice (inside quasiquote).

- **Parameters for dynamic binding.** `parameter` creates a parameter, `parameterize` sets it in a scope, child fibers inherit parent parameter frames.

  ```lisp
  (def *port* (parameter :stdout))

  (parameterize ((*port* :stderr))
    (print "to stderr"))  # uses *port* = :stderr

  (print "to stdout")     # uses *port* = :stdout
  ```

## Types

Immediates (nil, booleans, integers, floats, symbols, keywords, empty list) fit inline with no allocation. Everything else is a raw pointer into a slab-allocated `HeapObject` owned by the fiber's heap.

### Immediate types

| Type | Literal | Notes |
|------|---------|-------|
| nil | `nil` | Absence of a value. Falsy. |
| boolean | `true`, `false` | `false` is falsy; `true` is truthy. |
| integer | `42`, `-17` | Full-range i64. No auto-coercion to float. Overflow wraps. |
| float | `3.14`, `1e10` | IEEE 754 double. NaN/Infinity are heap-allocated. |
| symbol | `foo`, `'foo` | Interned identifier. |
| keyword | `:foo` | Self-evaluating interned name. Used for keys and tags. |
| empty list | `()`, `'()` | Terminates proper lists. **Truthy** — not the same as nil. |
| pointer | — | Raw C pointer (FFI only). NULL becomes nil. |

### Collections

#### Design principle: mutable/immutable split

Every collection type has an immutable variant and a mutable variant. Bare literal syntax is immutable; the `@` prefix makes it mutable.

| Immutable | Mutable | Literal | `@`-literal |
|-----------|---------|---------|-------------|
| array | @array | `[1 2 3]` | `@[1 2 3]` |
| struct | @struct | `{:a 1}` | `@{:a 1}` |
| string | @string | `"hello"` | `@"hello"` |
| bytes | @bytes | `b[1 2 3]` | `@b[1 2 3]` |
| set | @set | `\|1 2 3\|` | `@\|1 2 3\|` |

The `@` prefix means "mutable version of this literal." The types within each pair share the same logical structure but differ in mutability.

**string** — interned text. Equality is O(1) via interning. Indexing and length count grapheme clusters, not bytes.

```lisp
(def s "café")
(length s)              # => 4 (grapheme clusters, not bytes)
(get s 3)               # => "é"
(slice s 0 2)           # => "ca"
(concat s "!")          # => "café!"
```

**array** — fixed-length sequence.

```lisp
(def a [1 2 3])
(get a 0)               # => 1
(length a)              # => 3
(concat a [4 5])        # => [1 2 3 4 5]
```

**struct** — ordered dictionary. Keys are typically keywords.

```lisp
(def s {:name "Bob" :age 25})
(get s :name)           # => "Bob"
(keys s)                # => (:name :age)
(values s)              # => ("Bob" 25)
(has? s :name)          # => true
```

**set** — ordered collection of unique values. Mutable values are frozen on insertion.

```lisp
(def s |1 2 3|)
(contains? s 2)         # => true
(add s 4)               # => |1 2 3 4|
(del s 1)               # => |2 3|
(union |1 2| |2 3|)     # => |1 2 3|
(intersection |1 2| |2 3|)  # => |2|
(difference |1 2| |2 3|)    # => |1|
```

**bytes** — immutable binary data. Literal syntax: `b[1 2 3]`. Displays as `#bytes[hex ...]`.

```lisp
(def b b[1 2 3])
(def b2 (string->bytes "hello"))
(get b 0)               # => 1
(length b)              # => 5
(bytes->hex b2)         # => "68656c6c6f"
```

**@bytes** — mutable binary data. Literal syntax: `@b[1 2 3]`. Displays as `#@bytes[hex ...]`.

```lisp
(def b @b[1 2 3])
(def b2 (string->@bytes "hello"))
(get b 0)               # => 1
(length b)              # => 5
(bytes->hex b2)         # => "68656c6c6f"
```

### Lists

Singly-linked cons cells. Proper lists terminate with `()` (empty list), **not** `nil`.

```lisp
(list 1 2 3)            # => (1 2 3)
(cons 1 (list 2 3))     # => (1 2 3)
(first (list 1 2 3))    # => 1
(rest (list 1 2 3))     # => (2 3)
(rest (list 1))          # => ()  — empty list, not nil
```

> **nil vs empty list** — this is the most common gotcha. `nil` represents absence and is **falsy**. `()` is the empty list and is **truthy**. Lists terminate with `()`. Use `empty?` to check for end-of-list, not `nil?`. `nil?` only matches `nil`.

```lisp
(nil? nil)              # => true
(nil? ())               # => false  — empty list is not nil
(empty? ())             # => true
(empty? nil)            # => false  — nil is not an empty list
```

Lists are linked; tuples and arrays are contiguous in memory. They are not interchangeable.

### Functions

**Closures** — compiled functions with captured environment. Captures are by value; mutable captures use compiler-managed cells automatically.

```lisp
(fn (x) (+ x 1))           # anonymous
(defn add1 (x) (+ x 1))    # named (macro)
```

**Native functions** — Rust primitives (`+`, `-`, `cons`, etc.). Not constructible from Elle.

### Concurrency types

**Fiber** — independent execution context with its own stack, call frames, signal mask, and heap. See [Memory](#memory).

```lisp
(fiber/new (fn () body) mask)
(fiber/resume f value)
(fiber/status f)
```

**Parameter** — dynamic binding. `(parameter default)` creates one; calling it reads the current value. `parameterize` sets it within a scope. Child fibers inherit parent parameter frames.

**Box** — mutable box. User boxes are explicit (`box`/`unbox`/`rebox`). Local boxes are compiler-created for mutable captures and auto-unwrapped — users never see them.

### Truthiness

Exactly two values are falsy. Everything else is truthy.

| Value | Truthy? |
|-------|---------|
| `nil` | **No** |
| `false` | **No** |
| `()`, `0`, `""`, `[]`, `@[]` | Yes |

### Equality

`=` is structural for collections, interned for strings/symbols/keywords (O(1) comparison), and pointer identity for other heap objects.

### Type predicates

| Predicate | Matches |
|-----------|---------|
| `nil?` | `nil` only |
| `boolean?` | `true` or `false` |
| `number?` | integer or float |
| `integer?` | integer only |
| `float?` | float only |
| `symbol?` | symbol |
| `keyword?` | keyword |
| `string?` | string |
| `pair?` | cons cell |
| `list?` | cons cell or empty list |
| `empty?` | empty list, empty @array, empty array, empty @struct, empty struct, empty @string |
| `array?` | array (immutable or @array) |
| `struct?` | struct (immutable or @struct) |
| `set?` | set (immutable or @set) |
| `bytes?` | bytes (immutable or @bytes) |
| `function?` | closure or native function |
| `closure?` | closure only |
| `primitive?` | native function only |
| `fiber?` | fiber |
| `box?` | box (mutable box) |
| `parameter?` | dynamic parameter |
| `mutable?` | any mutable value (@array, @string, @bytes, @struct, @set, box, parameter) |
| `ptr?` / `pointer?` | raw or managed C pointer |
| `zero?` | zero (integer or float) |
| `type` / `type-of` | returns type as keyword (`:integer`, `:string`, etc.) |

### Display format

| Type | Display |
|------|---------|
| nil | `nil` |
| boolean | `true` / `false` |
| integer | `42` |
| float | `3.14` |
| symbol | `'foo` |
| keyword | `:foo` |
| empty list | `()` |
| string | `hello` (no quotes) |
| cons | `(1 2 3)` or `(a . b)` for improper |
| array | `[1 2 3]` |
| @array | `@[1 2 3]` |
| struct | `{:a 1}` |
| @struct | `@{:a 1}` |
| set | `\|1 2 3\|` |
| @set | `@\|1 2 3\|` |
| bytes | `#bytes[01 02 03]` |
| @bytes | `#@bytes[01 02 03]` |
| closure | `<closure>` |
| native fn | `<native-fn>` |
| fiber | `<fiber:status>` |
| box | `<box value>` |
| @string | `@"hello"` |
| pointer | `<pointer 0x...>` |

## Control Flow

- **Conditionals: `if`, `cond`, `when`, `unless`, `case`.** `if` is the primitive, others are macros or sugar.

  ```lisp
  (if (> x 0) "positive" "non-positive")

  (cond
    ((< x 0) "negative")
    ((= x 0) "zero")
    (true "positive"))

  (case x
    (1 "one")
    (2 "two")
    ("other"))
  ```

- **Pattern matching with `match`.** Type guards, element extraction, nested patterns, wildcard `_`, and guard clauses.

  ```lisp
  (match value
    (0                    "zero")
    (n when (< n 0)       "negative")
    (n when (> n 0)       "positive")
    ([a b]                (+ a b))
    ({:x x :y y}          (+ x y))
    (_                    "no match"))
  ```

- **Error handling: `try`/`catch`, `protect`, `defer`.** Built on fibers and signals, not exceptions.

  ```lisp
  (try
    (if (< x 0) (error "negative"))
    (+ x 1)
    (catch e
      (print "error:" e)
      0))

  (protect
    (do-something))  # => [success? value]

  (defer (cleanup)
    (do-work))  # cleanup runs after do-work
  ```

- **Loops: `while`, `forever`, `break`.** `while` is the primitive, `forever` is a macro, `break` exits a block.

  ```lisp
  (while (< i 10)
    (print i)
    (assign i (+ i 1)))

  (forever
    (if (done?) (break) (step)))

  (block :outer
    (each x in xs
      (if (found? x) (break :outer x))))
  ```

## Concurrency

Elle has three concurrency layers, each built on the one below:

1. **Fibers** — cooperative execution contexts with signal masks. The mechanism.
2. **Structured concurrency** — `ev/spawn`, `ev/join`, `ev/race`, `ev/scope`. Safe fork/join.
3. **Processes** — Erlang-style actors with mailboxes, supervision, and GenServers. The full model.

### Structured concurrency

```lisp
# Parallel work with automatic error propagation
(ev/scope (fn [spawn]
  (let ([users    (spawn (fn [] (fetch-users)))]
        [settings (spawn (fn [] (fetch-settings))])
    {:users (ev/join users) :settings (ev/join settings)})))

# Race: first to complete wins, rest are aborted
(ev/race [(ev/spawn (fn [] (fetch-from-primary)))
          (ev/spawn (fn [] (fetch-from-replica)))])
```

### Processes

[`lib/process.lisp`](lib/process.lisp) provides a complete Erlang/OTP-style
process system: lightweight processes with mailboxes, links, monitors,
named registration, GenServer, Actor, Task, Supervisor, and EventManager.

```lisp
(def process ((import "lib/process")))

(process:start (fn []
  # Supervisor manages worker processes
  (process:supervisor-start-link
    [{:id :cache :restart :permanent
      :start (fn []
        (process:gen-server-start-link
          {:init        (fn [_] @{})
           :handle-call (fn [req _from state]
             (match req
               ([:get k]   [:reply (get state k) state])
               ([:put k v] (put state k v) [:reply :ok state])))}
          nil :name :cache))}]
    :name :app-sup
    :max-restarts 5
    :logger (fn [event] (println "sup:" event)))

  (process:gen-server-call :cache [:put :version 1])
  (process:gen-server-call :cache [:get :version])))  # => 1
```

Supervisors can also manage OS subprocesses:

```lisp
(process:supervisor-start-link
  [(process:make-subprocess-child :nginx "/usr/sbin/nginx" ["-g" "daemon off;"])
   (process:make-subprocess-child :redis "/usr/bin/redis-server" [])]
  :name :daemon-sup :max-restarts 3)
```

See [`docs/processes.md`](docs/processes.md) for the full API including
GenServer callbacks, Actor state management, Task async/await, supervision
strategies, restart intensity limits, and structured logging.

See [`docs/concurrency.md`](docs/concurrency.md) for the structured
concurrency layer.

## Memory

- **No garbage collector.** Memory is reclaimed deterministically through three mechanisms, all derived from the same static analysis that drives the signal system:

  - **Per-fiber heaps:** Each fiber owns a slab allocator (`FiberHeap`) with 256-slot chunks and an intrusive free list. When a fiber finishes, its entire heap is freed — no traversal, no mark phase, no sweep. Slab allocation is O(1) with strong cache locality.

  - **Zero-copy inter-fiber sharing:** The compiler knows at fiber-creation time whether a fiber can yield (signal inference). Yielding fibers route all allocations to a `SharedAllocator` owned by the parent — the parent reads yielded values directly from shared memory. Silent fibers skip this entirely and allocate into their own slab with no indirection. No deep copy, no serialization, no runtime decision.

  - **Escape-analysis-driven scope reclamation:** The compiler analyzes every `let`, `letrec`, `block` scope. When it can prove no allocated value escapes — no captures, no suspension, no outward mutation — it emits `RegionEnter`/`RegionExit` bytecodes that return slab slots to the free list at scope exit, recycling memory without waiting for fiber death.

- **Long-running fiber schedulers don't accumulate garbage.** Each fiber's heap dies with it. Scope reclamation recycles memory within a fiber's lifetime. The ownership topology — private slab per fiber, shared slab per yield boundary — is the minimal structure that gives per-fiber lifecycle management and zero-copy yield simultaneously. See [`docs/memory.md`](docs/memory.md) for the full model.

## Execution Backends

Elle has two execution backends. Both share the same front end (reader →
expander → analyzer → HIR → LIR); they diverge at code generation.

### Bytecode VM + Cranelift JIT (default)

The default backend emits bytecode from LIR and runs it on a stack-based
VM. Hot functions are automatically compiled to native code via Cranelift
at runtime — no annotations, no opt-in. The compiler's signal system
identifies eligible functions; the JIT fires transparently.

### WASM backend (experimental)

The WASM backend compiles the entire program (stdlib + user code) into a
single WebAssembly module and executes it via Wasmtime. It supports
closures, fibers, tail calls, I/O, and the async scheduler — everything
except `eval`.

```bash
ELLE_WASM=1 elle script.lisp
```

A tiered mode compiles individual hot closures to WASM during bytecode
VM execution:

```bash
ELLE_WASM_TIER=1 elle script.lisp
```

See [`docs/impl/wasm.md`](docs/impl/wasm.md) for details.

## FFI

- **Call C without ceremony.** Load a library, bind a symbol, call it.

  ```lisp
  (def libc (ffi/native nil))
  (ffi/defbind sqrt libc "sqrt" :double @[:double])
  (sqrt 2.0)  # => 1.4142135623730951
  ```

- **Struct marshalling, variadic calls, callbacks, manual memory management all work.**

  ```lisp
  (def point-type (ffi/struct @[:double :double]))
  (def p (ffi/malloc (ffi/size point-type)))
  (ffi/write p point-type @[1.5 2.5])
  (def point-val (ffi/read p point-type))
  (ffi/free p)

  # Variadic: snprintf
  (def snprintf-ptr (ffi/lookup libc "snprintf"))
  (def snprintf-sig (ffi/signature :int @[:ptr :size :string :int] 3))
  (def out (ffi/malloc 128))
  (ffi/call snprintf-ptr snprintf-sig out 128 "answer: %d" 42)
  (ffi/free out)

  # Callbacks: qsort with Elle comparison function
  (def cmp (ffi/callback cmp-sig
    (fn [a b] (- (ffi/read a :i32) (ffi/read b :i32)))))
  (ffi/call qsort-ptr qsort-sig arr 5 4 cmp)
  (ffi/callback-free cmp)
  ```

- **FFI calls are tagged in the signal system.** Compiler knows where Elle's safety guarantees end and C's begin.

## Modules

- **Module system is minimal by design.** `import` loads a file — Elle source or native `.so` plugin — compiles and executes it, returns the last expression's value. No module declarations, no export lists, no special import form. It's a function call.

- **Source modules return their last expression.** A module that defines functions via `def` makes them available as globals; a module that ends with a struct or function hands that value back to the caller.

  ```lisp
  # math.lisp
  (fn [scale]
    {:add (fn (a b) (* (+ a b) scale))
     :mul (fn (a b) (* (* a b) scale))})

  # Usage
  (def {:add add :mul mul} ((import "std/math") 2))
  (add 1 2)  # => 6
  ```

- **Module system is user-replaceable.** `import` is an ordinary primitive. You can wrap it with caching, path resolution, sandboxing, or shadow it entirely.

## Plugins

- **Native plugins are Rust cdylib crates.** Link against `elle`, export an init function. Plugins register primitives through the same `PrimitiveDef` mechanism as builtins — same signal declarations, same doc strings, same arity checking. Work directly with `Value`. No intermediate serialization format, no separate process, no generated bindings.

  ```lisp
  (def re (import "plugin/regex"))
  (def pat (re:compile "\\d+"))
  (re:find-all pat "a1b2c3")
  # => ({:match "1" ...} {:match "2" ...} ...)
  ```

- **29 plugins ship with Elle:**

  | Plugin | Description |
  |--------|-------------|
  | `arrow` | Apache Arrow columnar data and Parquet serialization |
  | `base64` | Base64 encoding/decoding |
  | `clap` | Declarative CLI argument parsing |
  | `compress` | Compression (gzip, zstd, etc.) |
  | `crypto` | SHA-2 hashing and HMAC |
  | `csv` | CSV reading and writing |
  | `git` | Git repository operations |
  | `glob` | Filesystem glob patterns |
  | `hash` | Universal hashing (MD5, SHA-1/2/3, BLAKE2/3, CRC32, xxHash) |
  | `jiff` | Date, time, and duration arithmetic |
  | `msgpack` | MessagePack serialization |
  | `oxigraph` | RDF graph database (SPARQL) |
  | `polars` | Polars DataFrame operations (eager and lazy APIs) |
  | `protobuf` | Protocol Buffers serialization |
  | `random` | Pseudo-random number generation |
  | `regex` | Regular expressions |
  | `selkie` | Mermaid diagram rendering |
  | `semver` | Semantic version parsing and comparison |
  | `sqlite` | SQLite database |
  | `syn` | Rust source code parsing |
  | `tls` | TLS client and server via rustls |
  | `toml` | TOML parsing and generation |
  | `tree-sitter` | Multi-language parsing and structural queries |
  | `uuid` | UUID generation |
  | `xml` | XML parsing and generation |
  | `yaml` | YAML parsing and generation |

## Epochs

- **Breaking changes are versioned.** Each source file can declare an epoch — `(elle/epoch N)` — to pin the syntax version it was written for. The compiler transparently rewrites old-epoch syntax before macro expansion. Files without an epoch declaration target the current epoch.

- **Three migration rule types.** `Rename` swaps symbols mechanically. `Replace` restructures call forms using templates with positional placeholders. `Remove` flags deleted forms with a compile error and guidance message.

- **`elle rewrite` migrates source files.** One command applies all epoch rules, preserves formatting, and strips the epoch tag. `--check` mode verifies files are up to date in CI.

  See [`docs/epochs.md`](docs/epochs.md) for details.

## Tooling

- **Language server (LSP) for IDE integration.** Real-time diagnostics, hover documentation, jump-to-definition, refactoring support.

- **Static linter catches errors at compile time.** Wrong arity, unused bindings, signal violations, type mismatches in patterns, duplicate pattern variables.

  ```lisp
  # Compile-time errors caught by elle lint:
  (defn foo [x y] (+ x))  # Error: missing argument y
  (let [[unused 42]] 100) # Warning: unused binding
  (fn [a b] (yield))      # Error: silent context, can't yield
  (match x
    ([a b c] a)           # Error: pattern expects 3 elements
    (v v))                # Error: duplicate pattern variable
  ```

- **Match exhaustiveness is checked at compile time.** The compiler warns when a match expression has patterns that can never be reached, and when the match may not cover all cases for a known type.

- **Source-to-source rewriting tool.** The `rewrite` subcommand applies pattern-based rules to Elle source files for refactoring and code generation. Rules are pattern-action pairs that match syntax trees and produce transformed output.

- **Compilation pipeline is fully documented.** See [`docs/pipeline.md`](docs/pipeline.md) for data flow across boundaries and [`AGENTS.md`](AGENTS.md) for architecture details.

## Documentation

All documentation lives in `docs/` as literate markdown — every `.md` file
is runnable via `elle docs/<file>.md`. Code blocks tagged `` ```lisp `` are
extracted and executed; the rest is prose. This means examples are always
tested and never stale.

Start with [QUICKSTART.md](QUICKSTART.md) for the full table of contents.

| Directory | Content |
|-----------|---------|
| `docs/*.md` | Language topics (one file per concept) |
| `docs/signals/` | Signal system and fiber architecture |
| `docs/cookbook/` | Recipes for common codebase changes |
| `docs/analysis/` | Testing, debugging, semantic portraits |
| `docs/impl/` | Implementation internals (reader, HIR, LIR, VM, JIT) |

## Getting Started

### Prerequisites

- **Rust** (stable, 2021 edition) — [install via rustup](https://rustup.rs/)
- **Linux and macOS** — x86_64 and aarch64. On Linux, Elle uses io_uring for
  async I/O; on macOS, a thread-pool backend provides the same API.
- **GNU Make** — for the build targets below.

### Build and run

```bash
make                                      # build elle + plugins
echo '(println "hello")' | ./target/release/elle  # one-liner
./target/release/elle                     # REPL
make smoke                                # run all tests (~30s)
make test                                 # full test suite (~2min)
```

### Subcommands

- **`elle [file...]`** — Run Elle files (`.lisp`, `.lua`, `.md`) or start the REPL
- **`elle lint [options] <file|dir>...`** — Static analysis and linting
- **`elle lsp`** — Start the language server protocol server
- **`elle rewrite [options] <file...>`** — Source-to-source rewriting with rules

### LSP setup

`elle lsp` speaks standard LSP over stdio. Point your editor at it:

**VS Code** — add to `.vscode/settings.json`:

```json
{
  "elle.server.path": "/path/to/elle",
  "elle.server.args": ["lsp"]
}
```

**Neovim** — add to your LSP config:

```lua
vim.lsp.start({
  name = "elle",
  cmd = { "/path/to/elle", "lsp" },
  filetypes = { "elle", "lisp" },
  root_dir = vim.fs.dirname(vim.fs.find({ ".git" }, { upward = true })[1]),
})
```

## License

MIT
