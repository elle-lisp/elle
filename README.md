# Elle

[![CI](https://github.com/elle-lisp/elle/actions/workflows/main.yml/badge.svg)](https://github.com/elle-lisp/elle/actions/workflows/main.yml)

Elle is a Lisp with a compilation pipeline that does deep static analysis before any code runs: full binding resolution, capture analysis, and signal inference at compile time. This gives Elle a sound signal system, fully hygienic macros, colorless concurrency via fibers, and deterministic memory management — all derived from the same analysis pass.

The compiler already knows what every binding refers to, what every closure captures, and what signals every function can emit. This information is available to user code at runtime via `compile/analyze` and related functions, and exposed to AI agents via:

- **[Portrait](docs/analysis/portrait.md)** — Signal profiles, captures, and composition properties of a single file
- **[MCP Server](docs/mcp.md)** — Semantic knowledge graph of the entire codebase, queryable via SPARQL
- **[Agent Reasoning](docs/analysis/agent-reasoning.md)** — How AI agents use these tools to understand, refactor, and verify code

Humans write readable code without type annotations or formal constraints. Agents query the semantic model the compiler produces. Neither compromises the other.

If you know [Janet](https://janet-lang.org), think Janet on steroids — the same practical spirit (embeddable, batteries-included, modern syntax), with a compiler that understands your code deeply enough to expose it as structured data.

## Contents

- [What Makes Elle Different](#what-makes-elle-different)
- [Language](#language)
- [Types](#types)
- [Control Flow](#control-flow)
- [Concurrency](#concurrency)
- [Memory](#memory)
- [Execution Backends](#execution-backends)
- [FFI](#ffi)
- [Module System](#module-system)
- [Standard Library Modules](#standard-library-modules)
- [Plugins](#plugins)
- [Epochs — Versioned Syntax Migration](#epochs--versioned-syntax-migration)
- [Tooling](#tooling)
- [Documentation](#documentation)
- [For Agent Developers](#for-agent-developers)
- [Alternative Surface Syntaxes](#alternative-surface-syntaxes)
- [Coming from Another Language](#coming-from-another-language)
- [Getting Started](#getting-started)
- [License](#license)

## What Makes Elle Different

- **Fibers are the concurrency primitive.** ([docs/concurrency.md](docs/concurrency.md)) A fiber is an independent execution context — its own stack, call frames, signal mask, and heap. Fibers are cooperative and explicitly resumed. The parent drives execution by calling `fiber/resume`. When a fiber emits a signal, it suspends and the parent decides what to do next.

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

- **Signals are typed, cooperative flow-control interrupts.** ([docs/runtime.md](docs/runtime.md)) A signal is a keyword — `:error`, `:log`, `:abort`, or any user-defined name — that a fiber emits to its parent. The parent's signal mask determines which signals surface; unmasked signals propagate further up. The compiler infers which functions can emit signals and enforces that silent contexts don't call yielding ones.

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

- **Fully hygienic macros that operate on syntax objects, not text or s-expressions.** ([docs/macros.md](docs/macros.md)) Macros receive and return `Syntax` objects carrying scope information (Racket-style scope sets). Name capture is structurally impossible, not just conventionally avoided.

  ```lisp
  (defmacro my-swap (a b)
    `(let [tmp ,a] (assign ,a ,b) (assign ,b tmp)))

  (let [tmp 100 x 1 y 2]
    (my-swap x y)
    tmp)  # => 100, not 1
  ```

  The `tmp` introduced by the macro does not shadow the caller's `tmp`. This is guaranteed by scope sets, not by convention.

- **Functions are colorless.** Any function can be called from a fiber. There is no `async`/`await` annotation that marks a function as suspending and forces all its callers to be marked too. Whether something runs concurrently is decided at the call site, not baked into the function definition. In Rust/JS/Python, a suspending `fetch` forces every caller to be `async` too; in Elle, the signal is inferred by the compiler and callers are unaffected.

- **Erlang-style processes fall out of the fiber model.** ([docs/processes.md](docs/processes.md)) The same fibers that drive coroutines and I/O compose into a full process system: mailboxes, links, monitors, named registration, supervisors, and GenServers — implemented entirely in Elle as [`lib/process.lisp`](lib/process.lisp). No VM changes, no special runtime support. A supervisor is a process that traps exits and restarts children; a GenServer is a process in a receive loop with call/cast dispatch. The signal system makes this possible: `yield` delivers scheduler commands, `:error` propagates crashes through links, `:fuel` enables preemptive scheduling, and `:io` lets processes do async I/O without blocking the scheduler.

  ```lisp
  (def process ((import "std/process")))

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

- **Modern Lisp syntax with no parser ambiguity.** ([docs/syntax.md](docs/syntax.md)) Macros operate on syntax trees, not text. See [`prelude.lisp`](prelude.lisp) for hygienic macros and standard forms.

- **Collection literals with mutable/immutable split.** ([docs/types.md](docs/types.md)) Bare delimiters are immutable: `[1 2 3]` (array), `{:key val}` (struct), `"hello"` (string). `@`-prefixed are mutable: `@[1 2 3]` (@array), `@{:key val}` (@struct), `@"hello"` (@string).

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

- **Strings are sequences of grapheme clusters.** ([docs/strings.md](docs/strings.md)) `length`, slicing, indexing, and iteration all count grapheme clusters — not bytes, not codepoints.

  ```lisp
  (length "café")           # => 4, not 5 bytes
  (get "café" 3)              # => "é"
  (slice "café" 0 2)        # => "ca"
  (first "café")            # => "c"
  (rest "café")             # => "afé"
  (length "👨‍👩‍👧")   # => 1
  ```

- **Destructuring in all binding positions.** ([docs/destructuring.md](docs/destructuring.md)) `def`, `let`, `let*`, `var`, `fn` parameters, `match` patterns — missing values become `nil`, wrong types become `nil`.

  ```lisp
  (def (head & tail) (list 1 2 3 4))
  (def [x _ z] [10 20 30])
  (def {:name n :age a} {:name "Bob" :age 25})
  (def {:config {:db {:host h}}}
    {:config {:db {:host "localhost"}}})
  ```

- **Closures with automatic capture analysis.** ([docs/functions.md](docs/functions.md)) The compiler tracks which variables each closure captures. Mutable captures use cells automatically. Enables escape analysis for scope-level memory reclamation.

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

- **Parameters for dynamic binding.** ([docs/parameters.md](docs/parameters.md)) `parameter` creates a parameter, `parameterize` sets it in a scope, child fibers inherit parent parameter frames.

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
| string | @string | `"hello"` | `@"hello"` |
| array | @array | `[1 2 3]` | `@[1 2 3]` |
| struct | @struct | `{:a 1}` | `@{:a 1}` |
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

**bytes** — immutable binary data. Literal syntax `b[1 2 3]`.

```lisp
(def b b[1 2 3])
(def b2 (string->bytes "hello"))
(get b 0)               # => 1
(length b)              # => 3
(bytes->hex b2)         # => "68656c6c6f"
```

**@bytes** — mutable binary data. Literal syntax `@b[1 2 3]`.

```lisp
(def b @b[1 2 3])
(def b2 (string->@bytes "hello"))
(get b 0)               # => 1
(length b)              # => 3
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

Every type has a predicate: `nil?`, `integer?`, `string?`, `array?`, `struct?`, `pair?`, `bytes?`, `set?`, `fiber?`, `closure?`, `mutable?`, etc. `type-of` returns the type as a keyword.

```lisp
(type-of 42)        # => :integer
(string? "hello")   # => true
(mutable? @[1 2])   # => true
(mutable? [1 2])    # => false
```

See [docs/types.md](docs/types.md) for the full list.

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
| @string | `@"hello"` |
| cons | `(1 2 3)` or `(a . b)` for improper |
| array | `[1 2 3]` |
| @array | `@[1 2 3]` |
| struct | `{:a 1}` |
| @struct | `@{:a 1}` |
| set | `\|1 2 3\|` |
| @set | `@\|1 2 3\|` |
| bytes | `b[1 2 3]` |
| @bytes | `@b[1 2 3]` |
| closure | `<closure>` |
| native fn | `<native-fn>` |
| fiber | `<fiber:status>` |
| box | `<box value>` |
| pointer | `<pointer 0x...>` |

## Control Flow

- **Conditionals: `if`, `cond`, `when`, `unless`, `case`.** ([docs/control.md](docs/control.md)) `if` is the primitive, others are macros or sugar.

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

- **Pattern matching with `match`.** ([docs/match.md](docs/match.md)) Type guards, element extraction, nested patterns, wildcard `_`, and guard clauses.

  ```lisp
  (match value
    (0                    "zero")
    (n when (< n 0)       "negative")
    (n when (> n 0)       "positive")
    ([a b]                (+ a b))
    ({:x x :y y}          (+ x y))
    (_                    "no match"))
  ```

- **Error handling: `try`/`catch`, `protect`, `defer`.** ([docs/errors.md](docs/errors.md)) Built on fibers and signals, not exceptions.

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

- **Loops: `while`, `forever`, `break`.** ([docs/loops.md](docs/loops.md)) `while` is the primitive, `forever` is a macro, `break` exits a block.

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

See [docs/concurrency.md](docs/concurrency.md), [docs/scheduler.md](docs/scheduler.md), and [docs/io.md](docs/io.md).

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
(def process ((import "std/process")))

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

- **No garbage collector.** ([docs/memory.md](docs/memory.md)) Memory is reclaimed deterministically through three mechanisms, all derived from the same static analysis that drives the signal system:

  - **Per-fiber heaps:** Each fiber owns a slab allocator (`FiberHeap`) with 256-slot chunks and an intrusive free list. When a fiber finishes, its entire heap is freed — no traversal, no mark phase, no sweep. Slab allocation is O(1) with strong cache locality.

  - **Zero-copy inter-fiber sharing:** The compiler knows at fiber-creation time whether a fiber can yield (signal inference). Yielding fibers route all allocations to a `SharedAllocator` owned by the parent — the parent reads yielded values directly from shared memory. Silent fibers skip this entirely and allocate into their own slab with no indirection. No deep copy, no serialization, no runtime decision.

  - **Escape-analysis-driven scope reclamation:** The compiler analyzes every `let`, `letrec`, `block` scope. When it can prove no allocated value escapes — no captures, no suspension, no outward mutation — it emits `RegionEnter`/`RegionExit` bytecodes that return slab slots to the free list at scope exit, recycling memory without waiting for fiber death.

- **Long-running fiber schedulers don't accumulate garbage.** Each fiber's heap dies with it. Scope reclamation recycles memory within a fiber's lifetime. The ownership topology — private slab per fiber, shared slab per yield boundary — is the minimal structure that gives per-fiber lifecycle management and zero-copy yield simultaneously. See [`docs/memory.md`](docs/memory.md) for the full model.

## Execution Backends

Elle has four execution tiers. All share the same front end (reader →
expander → analyzer → HIR → LIR); they diverge at code generation.
The VM tries tiers in order and falls through automatically — no
annotations needed.

### Bytecode VM + Cranelift JIT (default)

The default backend emits bytecode from LIR and runs it on a stack-based
VM. Hot functions are automatically compiled to native code via Cranelift
at runtime — no annotations, no opt-in. The compiler's signal system
identifies eligible functions; the JIT fires transparently.

### MLIR-CPU (tier-2, optional)

Pure numeric functions (arithmetic, comparison, local variables, control
flow — no heap allocation, no calls, no signals beyond `:error`) are
compiled through MLIR → LLVM → native code. This runs **before** the
Cranelift JIT in the dispatch chain: hot eligible functions get MLIR
instead of Cranelift.

Eligible functions may capture variables (passed as extra parameters)
and return booleans. The caller reboxes the raw `i64` result based on
the return type. Non-numeric arguments fall through to bytecode.

Requires `--features mlir` and LLVM 22 + MLIR at build time.

See [`docs/impl/mlir.md`](docs/impl/mlir.md) for details.

### GPU / SPIR-V (optional)

The same eligibility predicate drives SPIR-V emission: a pure numeric
closure is lowered to a compute kernel and dispatched to the GPU via
Vulkan. `gpu:map` applies a scalar function across arrays in parallel —
each workgroup thread runs the function on one element.

```lisp
(def gpu ((import "std/gpu")))
(gpu:map (fn [x] (* x x)) [1 2 3 4])  # => [1 4 9 16]
```

The fiber suspends on the GPU fence fd — no thread pool thread is
held while the GPU works. SPIR-V can also be written by hand via
`lib/spirv.lisp` for fused or custom kernels.

Requires `--features mlir` and the `vulkan` plugin.

See [`docs/impl/gpu.md`](docs/impl/gpu.md) and
[`docs/impl/spirv.md`](docs/impl/spirv.md) for details.

### WASM backend (experimental)

The WASM backend compiles the entire program (stdlib + user code) into a
single WebAssembly module and executes it via Wasmtime. It supports
closures, fibers, tail calls, I/O, and the async scheduler — everything
except `eval`.

```bash
elle --wasm=full script.lisp
```

A tiered mode compiles individual hot closures to WASM during bytecode
VM execution:

```bash
elle --wasm=11 script.lisp
```

See [`docs/impl/wasm.md`](docs/impl/wasm.md) for details.

## FFI

- **Call C without ceremony.** ([docs/ffi.md](docs/ffi.md)) Load a library, bind a symbol, call it.

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

## Module System

- **Minimal and parametric.** ([docs/modules.md](docs/modules.md)) `import` loads a file — Elle source or native `.so` plugin — compiles and executes it, returns the last expression's value. Elle modules are closures that return structs; call the closure to instantiate. Parameters to the closure configure the module — inject dependencies, toggle features, pass credentials.

  ```lisp
  ## Simple module — call the returned closure
  (def b64 ((import "std/base64")))
  (b64:encode "hello")

  ## Parametric module — pass the hash plugin to enable UUID v5
  (def hash-plugin (import "plugin/hash"))
  (def uuid ((import "std/uuid") hash-plugin))
  (uuid:v5 "6ba7b810-9dad-11d1-80b4-00c04fd430c8" "example.com")

  ## Plugin — import returns a struct directly (no closure call)
  (def re (import "plugin/regex"))
  (re:match "\\d+" "abc123")
  ```

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

- **`include` splices source at compile time.** Unlike `import` which compiles and runs a separate file, `include` inserts another file's forms directly into the current compilation unit — they share scope. Use `include` for splitting large files; use `import` for separate modules.

- **Module system is user-replaceable.** `import` is an ordinary primitive. You can wrap it with caching, path resolution, sandboxing, or shadow it entirely.

## Standard Library Modules

See [docs/libraries.md](docs/libraries.md) for full documentation.

- **Pure Elle and FFI modules require no compilation.** Import with the `std/` prefix. Modules that wrap C libraries (sqlite, compress, git) use Elle's FFI — the system library must be installed, but no Rust build step is needed.

  ```lisp
  (def b64 ((import "std/base64")))
  (b64:encode "hello")  # => "aGVsbG8="

  (def db ((import "std/sqlite")))
  (def conn (db:open ":memory:"))
  (db:exec conn "CREATE TABLE t (id INTEGER, name TEXT)")
  ```

  | Module | Description |
  |--------|-------------|
  | `aws` | Elle-native AWS client (SigV4, HTTPS) |
  | `base64` | Base64 encoding/decoding |
  | `cli` | Declarative CLI argument parsing |
  | `color` | Color spaces, mixing, gradients, perceptual distance |
  | `compress` | Gzip, zlib, deflate, zstd (FFI to libz + libzstd) |
  | `contract` | Compositional validation for function boundaries |
  | `dns` | Pure Elle DNS client (RFC 1035) |
  | `egui` | Immediate-mode GUI wrapping the `egui` plugin |
  | `git` | Git repository operations (FFI to libgit2) |
  | `glob` | Filesystem glob pattern matching |
  | `gtk4` | GTK4 bindings via FFI (pure Elle, no plugin) |
  | `hash` | Streaming hash helpers (ports, coroutines) |
  | `http` | Pure Elle HTTP/1.1 client and server |
  | `irc` | Coroutine-based IRCv3 client with SASL |
  | `lua` | Lua compatibility prelude |
  | `mqtt` | MQTT client (uses the `mqtt` plugin for packet codec) |
  | `portrait` | Semantic portraits from `compile/analyze` |
  | `process` | Erlang-style GenServer, Supervisor, Actor, Task |
  | `rdf` | RDF triple generation for the Elle knowledge graph |
  | `redis` | Pure Elle Redis client (RESP2) |
  | `resource` | Deterministic resource consumption measurement |
  | `sdl3` | SDL3 bindings via FFI |
  | `semver` | Semantic version parsing and comparison |
  | `sqlite` | SQLite database (FFI to libsqlite3) |
  | `svg` | SVG construction and emission (pure Elle) |
  | `sync` | Locks, semaphores, condvars, barriers, queues |
  | `telemetry` | OpenTelemetry metrics (OTLP/HTTP JSON export) |
  | `tls` | TLS client and server (wraps `tls` plugin) |
  | `uuid` | UUID generation and parsing |
  | `watch` | Event-driven filesystem watcher |
  | `zmq` | ZeroMQ bindings via FFI |

## Plugins

- **Native plugins are Rust cdylib crates.** Link against `elle`, export an init function. Plugins register primitives through the same `PrimitiveDef` mechanism as builtins — same signal declarations, same doc strings, same arity checking. Work directly with `Value`. No intermediate serialization format, no separate process, no generated bindings.

  ```lisp
  (def re (import "plugin/regex"))
  (def pat (re:compile "\\d+"))
  (re:find-all pat "a1b2c3")
  # => ({:match "1" ...} {:match "2" ...} ...)
  ```

- **22 plugins ship with Elle.** See [docs/plugins.md](docs/plugins.md) for details.

  | Plugin | Description |
  |--------|-------------|
  | `arrow` | Apache Arrow columnar data and Parquet serialization |
  | `crypto` | SHA-2 hashing and HMAC |
  | `csv` | CSV reading and writing |
  | `egui` | Immediate-mode GUI (egui + winit + glow) |
  | `hash` | Universal hashing (MD5, SHA-1/2/3, BLAKE2/3, CRC32, xxHash) |
  | `image` | Raster image I/O, transforms, drawing, and analysis |
  | `jiff` | Date, time, and duration arithmetic |
  | `mqtt` | MQTT packet codec |
  | `msgpack` | MessagePack serialization |
  | `oxigraph` | RDF graph database (SPARQL) |
  | `polars` | Polars DataFrame operations (eager and lazy APIs) |
  | `protobuf` | Protocol Buffers serialization |
  | `random` | Pseudo-random number generation |
  | `regex` | Regular expressions |
  | `selkie` | Mermaid diagram rendering |
  | `svg` | SVG rasterization via resvg (construction lives in `lib/svg.lisp`) |
  | `syn` | Rust source code parsing |
  | `tls` | TLS client and server via rustls |
  | `toml` | TOML parsing and generation |
  | `tree-sitter` | Multi-language parsing and structural queries |
  | `xml` | XML parsing and generation |
  | `yaml` | YAML parsing and generation |

## Epochs — Versioned Syntax Migration

- **Breaking changes are versioned.** ([docs/epochs.md](docs/epochs.md)) Each source file can declare an epoch — `(elle/epoch N)` — to pin the syntax version it was written for. The compiler transparently rewrites old-epoch syntax before macro expansion. Files without an epoch declaration target the current epoch.

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

- **MCP server for AI coding assistants.** ([docs/mcp.md](docs/mcp.md)) An [MCP](https://modelcontextprotocol.io) server written in Elle that gives AI agents deep structural access to the codebase. Maintains a persistent RDF knowledge graph of both Elle and Rust source. 15 tools for static analysis, refactoring, and cross-language tracing. Complements the LSP server — LSP handles real-time editing; MCP handles AI-driven code understanding.

  **What can an AI agent do with it?**

  - *"What does `fold` do?"* — `portrait` returns the full effect profile, failure modes, and composition properties.
  - *"What breaks if I change `prim_first`?"* — `impact` traces all callers and downstream signal changes.
  - *"Trace `map` from Elle through primitives into Rust."* — `trace` follows the call chain: Elle stdlib → `cons`/`first`/`rest` primitives → Rust `prim_cons`/`prim_first`/`prim_rest` → `Value::cons()`/`as_cons()`.
  - *"Which functions are JIT-eligible?"* — `signal_query` with `jit-eligible` returns all silent functions.
  - *"Rename `helper` to `utils` across the whole file."* — `compile_rename` rewrites all references, respecting lexical scope.
  - *"Find all Rust structs that have a `signal` field."* — direct SPARQL: `SELECT ?name WHERE { ?s a rust:Struct ; rust:field "signal" ; rust:name ?name }`

  See [`tools/demo-queries.lisp`](tools/demo-queries.lisp) for more examples.

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
| [DEVLOG.md](DEVLOG.md) | Per-PR development log (368 entries from diffs) |
| [CHANGELOG.md](CHANGELOG.md) | Changelog by subsystem arc (agent-optimized) |

## For Agent Developers

The compiler computes signal inference, capture analysis, and call graphs for every file. The MCP server makes all of this queryable.

- **[Agent Reasoning Guide](docs/analysis/agent-reasoning.md)** — Workflow: understand locally via `portrait`, reason globally via SPARQL, refactor via compile-aware tools
- **[MCP Server](docs/mcp.md)** — 15 tools: `portrait`, `signal_query`, `impact`, `trace`, `compile_rename`, `compile_extract`, `compile_parallelize`, `verify_invariants`, and SPARQL
- **[Analysis overview](docs/analysis/README.md)** — How portrait, MCP, and agent reasoning fit together

## Alternative Surface Syntaxes

Elle's native syntax is s-expressions. If you find parentheses unfamiliar,
you can write Elle programs using Python, JavaScript, or Lua syntax instead.
These are purely cosmetic — the reader translates them into the same syntax
trees as s-expressions, and the rest of the pipeline (macro expansion,
analysis, compilation, execution) is unchanged.

Note that not all semantics map cleanly to other syntaxes. The alternative
readers support a common subset of syntax features for testing purposes,
but they are not as fully-featured as the s-expression reader.

To use an alternative syntax, just name your file with the appropriate
extension:

```bash
elle program.py    # Python syntax
elle program.js    # JavaScript syntax
elle program.lua   # Lua syntax
elle program.lisp  # s-expression syntax (default)
```

Each surface syntax maps its idioms to Elle primitives:

| Python | JS | Lua | Elle |
|---|---|---|---|
| `x = 42` | `const x = 42` | `local x = 42` | `(def x 42)` |
| `lambda x: x + 1` | `(x) => x + 1` | `function(x) return x + 1 end` | `(fn (x) (+ x 1))` |
| `if c: a` / `else: b` | `if (c) { a } else { b }` | `if c then a else b end` | `(if c a b)` |
| `for x in arr:` | `for (const x of arr)` | `for x in arr do ... end` | `(each x in arr ...)` |
| `{"x": 1, "y": 2}` | `{x: 1, y: 2}` | `{x = 1, y = 2}` | `@{:x 1 :y 2}` |
| `[1, 2, 3]` | `[1, 2, 3]` | `{1, 2, 3}` | `@[1 2 3]` |

See [`demos/syntax.py`](demos/syntax.py), [`demos/syntax.js`](demos/syntax.js),
and [`demos/syntax.lua`](demos/syntax.lua) for comprehensive examples of
every syntax feature.

## Coming from Another Language

Orientation guides for programmers arriving from other languages — key
differences, concept mappings, and gotchas:
[Python](docs/coming-from.md#python) ·
[JavaScript](docs/coming-from.md#javascript--typescript) ·
[Rust](docs/coming-from.md#rust) ·
[Go](docs/coming-from.md#go) ·
[Clojure](docs/coming-from.md#clojure) ·
[Common Lisp / Scheme](docs/coming-from.md#common-lisp--scheme) ·
[Erlang / Elixir](docs/coming-from.md#erlang--elixir) ·
[Janet](docs/coming-from.md#janet) ·
[C](docs/coming-from.md#c)

## Getting Started

See [`INSTALL.md`](INSTALL.md) for full build instructions, system
dependencies, and optional features (WASM, MLIR).

### Quick start

```bash
cargo build --release -p elle              # build elle
echo '(println "hello")' | ./target/release/elle  # one-liner
./target/release/elle                     # REPL
make smoke                                # run all tests (~30s)
```

Plugins live in a [separate repository](https://github.com/elle-lisp/plugins)
and use a stable ABI — they can be built independently from elle.

### Subcommands

- **`elle [file...]`** — Run Elle files (`.lisp`, `.py`, `.js`, `.lua`, `.md`) or start the REPL
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
