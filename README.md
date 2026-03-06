# Elle

[![CI](https://github.com/elle-lisp/elle/actions/workflows/ci.yml/badge.svg)](https://github.com/elle-lisp/elle/actions/workflows/ci.yml)

Elle is a Lisp with modern syntax inspired by Janet. It has first-class fibers, a static effect system, and deep static analysis — giving you the safety and tooling of a compiled language with the flexibility of a Lisp. It runs on the Rust ecosystem with no garbage collector.

## Contents

- [Language](#language)
- [Control Flow](#control-flow)
- [Static Analysis](#static-analysis)
- [Fibers and Concurrency](#fibers-and-concurrency)
- [Memory](#memory)
- [FFI](#ffi)
- [Modules and Plugins](#modules-and-plugins)
- [Tooling](#tooling)
- [Getting Started](#getting-started)
- [License](#license)

## Language

- **Modern Lisp syntax with no parser ambiguity.** Macros operate on syntax trees, not text. See [`prelude.lisp`](prelude.lisp) for hygienic macros and standard forms.

- **Hygienic macros prevent accidental name capture.** Scope sets (Racket-style) protect macro-introduced bindings. `defmacro`, `quasiquote`, `unquote`, `datum->syntax` for intentional capture.
  <details><summary>Example: Hygienic Macros</summary>

  ```lisp
  (defmacro my-swap (a b)
    `(let ((tmp ,a)) (set ,a ,b) (set ,b tmp)))

  (let ((tmp 100) (x 1) (y 2))
    (my-swap x y)
    tmp)  # => 100, not 1
  ```
  </details>

- **Prelude macros for common patterns.** `defn`, `let*`, `->`, `->>`, `when`, `unless`, `try`/`catch`, `protect`, `defer`, `with`, `yield*`, `each`, `forever` — all defined in Elle, not special forms.

- **Collection literals with mutable/immutable split.** Bare delimiters are immutable: `[1 2 3]` (tuple), `{:key val}` (struct), `"hello"` (string). `@`-prefixed are mutable: `@[1 2 3]` (array), `@{:key val}` (table), `@"hello"` (buffer).
  <details><summary>Example: Collections</summary>

  ```lisp
  # Immutable
  (def t [1 2 3])           # tuple
  (def s {:name "Bob"})     # struct
  (def str "hello")         # string

  # Mutable
  (def a @[1 2 3])          # array
  (def tbl @{:name "Bob"})  # table
  (def buf @"hello")        # buffer

  # Bytes and blobs (no literal syntax)
  (def b (bytes 1 2 3))     # immutable bytes
  (def bl (blob 1 2 3))     # mutable blob
  ```
  </details>

- **Destructuring in all binding positions.** `def`, `let`, `let*`, `var`, `fn` parameters, `match` patterns — missing values become `nil`, wrong types become `nil`.
  <details><summary>Example: Destructuring</summary>

  ```lisp
  (def (head & tail) (list 1 2 3 4))
  (def [x _ z] [10 20 30])
  (def {:name n :age a} {:name "Bob" :age 25})
  (def {:config {:db {:host h}}}
    {:config {:db {:host "localhost"}}})
  ```
  </details>

- **Functions with closures and capture analysis.** `fn`, `defn`, variadic parameters (`&`), tail calls optimized, closures capture by value, mutable captures use cells automatically.
  <details><summary>Example: Functions and Closures</summary>

  ```lisp
  (defn make-counter [start]
    (var n start)
    (fn []
      (set n (+ n 1))
      n))

  (def c (make-counter 0))
  (c)  # => 1
  (c)  # => 2
  ```
  </details>

- **Splice operator for array spreading.** `;expr` marks a value for spreading at call sites and in data constructors. `(splice expr)` is the long form.
  <details><summary>Example: Splice</summary>

  ```lisp
  (def args @[2 3])
  (+ 1 ;args)  # => 6, same as (+ 1 2 3)

  (def items @[1 2])
  @[0 ;items 3]  # => @[0 1 2 3]
  ```
  </details>

- **Reader macros for quasiquote and unquote.** `` ` `` for quasiquote, `,` for unquote, `,;` for unquote-splice (inside quasiquote).

- **Parameters for dynamic binding.** `make-parameter` creates a parameter, `parameterize` sets it in a scope, child fibers inherit parent parameter frames.
  <details><summary>Example: Parameters</summary>

  ```lisp
  (def *port* (make-parameter :stdout))

  (parameterize ((*port* :stderr))
    (print "to stderr"))  # uses *port* = :stderr

  (print "to stdout")     # uses *port* = :stdout
  ```
  </details>

## Control Flow

- **Conditionals: `if`, `cond`, `when`, `unless`, `case`.** `if` is the primitive, others are macros or sugar.
  <details><summary>Example: Conditionals</summary>

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
  </details>

- **Pattern matching with `match`.** Type guards (`IsArray`, `IsPair`, `IsStruct`, `IsTable`), element extraction, nested patterns, wildcard `_`.
  <details><summary>Example: Pattern Matching</summary>

  ```lisp
  (match value
    ([a b] (+ a b))
    ({:x x :y y} (+ x y))
    ((cons h t) h)
    (_ "no match"))
  ```
  </details>

- **Error handling: `try`/`catch`, `protect`, `defer`.** Built on fibers and signals, not exceptions.
  <details><summary>Example: Error Handling</summary>

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
  </details>

- **Loops: `while`, `forever`, `break`.** `while` is the primitive, `forever` is a macro, `break` exits a block.
  <details><summary>Example: Loops</summary>

  ```lisp
  (while (< i 10)
    (print i)
    (set i (+ i 1)))

  (forever
    (if (done?) (break) (step)))

  (block :outer
    (each x in xs
      (if (found? x) (break :outer x))))
  ```
  </details>

## Static Analysis

- **Effect system infers what code does.** Compiler automatically determines whether a function is pure, yields, or polymorphic — no annotations needed. Effects flow through the entire pipeline.
  <details><summary>Example: Effect System</summary>

  ```lisp
  # Pure function — inferred automatically
  (defn add (a b) (+ a b))

  # Yielding function — inferred from yield call
  (defn fetch-data (url)
    (yield :http-request url)
    (yield :http-wait))

  # Polymorphic — effect depends on callback
  (defn map-effect (f xs)
    (map f xs))  # effect = effect of f
  ```
  </details>

- **Static linter catches errors at compile time.** Wrong arity, unused bindings, effect violations, type mismatches in patterns, duplicate pattern variables.
  <details><summary>Example: Static Analysis and Linting</summary>

  ```lisp
  # Compile-time errors caught by elle lint:
  (defn foo (x y) (+ x))  # Error: missing argument y
  (let ((unused 42)) 100) # Warning: unused binding
  (fn (a b) (yield))      # Error: pure context, can't yield
  (match x
    ([a b c] a)           # Error: pattern expects 3 elements
    (v v))                # Error: duplicate pattern variable
  ```
  </details>

- **Capture analysis and closure optimization.** Compiler tracks which variables are captured by closures. Mutable captures use cells automatically. Enables escape analysis for scope-level memory reclamation and JIT eligibility decisions.
  <details><summary>More: Capture Analysis</summary>

  The compiler analyzes every closure to determine which variables from outer scopes it references. This enables:
  - Automatic cell wrapping for mutable captures
  - Escape analysis for scope-level memory reclamation
  - JIT eligibility decisions (non-suspending closures compile to native code)
  </details>

## Fibers and Concurrency

- **Fibers are independent execution contexts.** Each fiber has its own stack, call frames, and heap. Fibers communicate through `yield`/`resume`, each yield carries a signal (integer classifying the event).
  <details><summary>Example: Fibers and Coroutines</summary>

  ```lisp
  (defn make-producer []
    (coro/new (fn []
      (each i in (range 5)
        (yield i)))))

  (def co (make-producer))
  (forever
    (if (coro/done? co)
      (break)
      (print (coro/resume co))))
  ```
  </details>

- **Colorless functions, colored fibers.** Any function can run inside a fiber. The fiber's signal mask (set at creation) decides what to catch — not the function. No `async`/`await` coloring.

- **Scheduling is user-space.** Elle provides no built-in scheduler. [`examples/processes.lisp`](examples/processes.lisp) demonstrates Erlang-style cooperative scheduling in ~200 lines: `spawn`, `send`, `recv`, `link`, `trap-exit`, `spawn-link`, crash cascade, deadlock detection.
  <details><summary>More: Fiber Scheduling</summary>

  Crash isolation comes from each fiber owning its own heap — when a fiber dies, its entire heap is freed in O(1). Link-based supervision comes from signal propagation through fiber chains. Both are properties of fibers themselves, not the scheduler.
  </details>

- **Signal dispatch is O(1).** Single bitmask check, branch-predictor-friendly. `try`/`catch`, `protect`, generators are all prelude macros built on `coro/new` and `coro/resume`.

## Memory

- **No garbage collector.** Memory is reclaimed deterministically through three mechanisms:
  - **Per-fiber heaps:** Each fiber allocates into a bump arena. When it finishes, the entire heap is freed in O(1) — no traversal, no mark phase, no sweep. Fibers get strong cache locality.
  - **Zero-copy inter-fiber sharing:** Yielding fibers route allocations to a shared arena; parents read directly from shared memory. No deep copy, no serialization.
  - **Escape-analysis-driven scope reclamation:** Compiler analyzes every `let`, `letrec`, `block` scope. When it can prove no allocated value escapes — no captures, no suspension, no outward mutation — it frees allocations at scope exit.

- **NaN-boxed values: 8 bytes per value.** Integers, floats, booleans, nil, symbols, keywords, and short strings (≤6 bytes) fit inline. Everything else is a pointer into a heap.

- **Long-running fiber schedulers don't accumulate garbage.** Each fiber's heap dies with it. Memory is reclaimed at scope exit or fiber death, without pausing the world.

## FFI

- **Call C without ceremony.** Load a library, bind a symbol, call it.
  <details><summary>Example: Basic FFI</summary>

  ```lisp
  (def libc (ffi/native nil))
  (ffi/defbind sqrt libc "sqrt" :double @[:double])
  (sqrt 2.0)  # => 1.4142135623730951
  ```
  </details>

- **Struct marshalling, variadic calls, callbacks, manual memory management all work.**
  <details><summary>Example: Advanced FFI</summary>

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
  </details>

- **FFI calls are tagged in the effect system.** Compiler knows where Elle's safety guarantees end and C's begin.

## Modules and Plugins

- **Module system is minimal by design.** `import-file` loads a file — Elle source or native `.so` plugin — compiles and executes it, returns the last expression's value. No module declarations, no export lists, no special import form. It's a function call.

- **Source modules return their last expression.** A module that defines functions via `def` makes them available as globals; a module that ends with a struct or function hands that value back to the caller.
  <details><summary>Example: Parametric Modules</summary>

  ```lisp
  # math.lisp
  (fn (scale)
    {:add (fn (a b) (* (+ a b) scale))
     :mul (fn (a b) (* (* a b) scale))})

  # Usage
  (def {:add add :mul mul} ((import-file "math.lisp") 2))
  (add 1 2)  # => 6
  ```
  </details>

- **Native plugins are Rust cdylib crates.** Link against `elle`, export an init function. Plugins register primitives through the same `PrimitiveDef` mechanism as builtins — same effect declarations, same doc strings, same arity checking. Work directly with `Value`. No C marshalling, no serialization boundary.
  <details><summary>Example: Plugin Usage</summary>

  ```lisp
  (def re (import-file "target/release/libelle_regex.so"))
  (def pat (re:compile "\\d+"))
  (re:find-all pat "a1b2c3")
  # => ({:match "1" ...} {:match "2" ...} ...)
  ```
  </details>

- **Five plugins ship with Elle:** regex, sqlite, crypto, random, selkie.

- **Module system is user-replaceable.** `import-file` is an ordinary primitive. You can wrap it with caching, path resolution, sandboxing, or shadow it entirely.

## Tooling

- **Language server (LSP) for IDE integration.** Real-time diagnostics, hover documentation, jump-to-definition, refactoring support.

- **Source-to-source rewriting tool.** The `rewrite` subcommand applies pattern-based rules to Elle source files for refactoring and code generation. Rules are pattern-action pairs that match syntax trees and produce transformed output.

- **Formatter for consistent code style.** The `formatter` subcommand formats Elle source files.

- **Compilation pipeline is fully documented.** See [`docs/pipeline.md`](docs/pipeline.md) for data flow across boundaries and [`AGENTS.md`](AGENTS.md) for architecture details.
  <details><summary>More: Pipeline Stages</summary>

  Source → Reader → Syntax → Expander → Syntax → Analyzer → HIR → Lowerer → LIR → Emitter → Bytecode → VM

  Source locations survive the full journey for error reporting. Each stage infers more than the last: the reader produces syntax objects with scope sets; the analyzer resolves bindings, infers effects, computes captures, and flags mutations; the lowerer runs escape analysis and emits scope-level memory reclamation.

  The pipeline has three non-linear paths:
  - The analyzer loops until inter-procedural effects converge (fixpoint iteration over mutually recursive top-level defines)
  - The expander re-enters the pipeline recursively to evaluate macro bodies
  - The JIT forks off the VM to compile non-suspending closures to native x86_64 after bytecode execution

  Nothing is annotated. Everything is inferred.
  </details>

## Getting Started

```bash
make                                      # build elle + plugins + docs
./target/release/elle examples/hello.lisp # run a file
./target/release/elle                     # REPL
./target/release/elle lint <file|dir>    # static analysis
./target/release/elle lsp                 # language server
./target/release/elle rewrite <file>     # source-to-source rewriting
```

The `examples/` directory is executable documentation. Each file demonstrates a feature and asserts its own correctness — they run as part of CI.

### Subcommands

- **`elle [file...]`** — Run Elle files or start the REPL if no files given
- **`elle lint [options] <file|dir>...`** — Static analysis and linting
- **`elle lsp`** — Start the language server protocol server
- **`elle rewrite [options] <file...>`** — Source-to-source rewriting with rules
- **`elle format [options] <file...>`** — Format Elle source files

## License

MIT
