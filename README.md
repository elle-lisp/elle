# Elle

[![CI](https://github.com/elle-lisp/elle/actions/workflows/ci.yml/badge.svg)](https://github.com/elle-lisp/elle/actions/workflows/ci.yml)

Elle is a Lisp. What separates it from other Lisps is the depth of its static analysis: full binding resolution, capture analysis, and effect inference happen at compile time, before any code runs. This gives Elle a sound effect system, fully hygienic macros, colorless concurrency via fibers, and deterministic memory management — all derived from the same analysis pass.

## Contents

- [What Makes Elle Different](#what-makes-elle-different)
- [Language](#language)
- [Control Flow](#control-flow)
- [Memory](#memory)
- [FFI](#ffi)
- [Modules and Plugins](#modules-and-plugins)
- [Tooling](#tooling)
- [Getting Started](#getting-started)
- [License](#license)

## What Makes Elle Different

- **Static analysis is a first-class feature.** The compiler performs full binding resolution, capture analysis, effect inference, and lint passes before any code runs. This is not optional tooling bolted on — it is the compilation pipeline. Most Lisps are dynamic; Elle knows at compile time what every binding refers to, what every closure captures, and what effects every function can produce.
  <details><summary>More: Compile-Time Analysis</summary>

  The compilation pipeline is: Source → Reader → Syntax → Expander → Analyzer → HIR → Lowerer → LIR → Emitter → Bytecode → VM. Each stage infers more than the last. The analyzer resolves all bindings to their definitions, computes which variables each closure captures, infers the effect of every expression, and flags lint violations — all before bytecode is emitted. This is why the linter catches errors at compile time, why the effect system is sound, and why the JIT can make intelligent decisions about what to compile natively.
  </details>

- **A sound effect system, inferred not declared.** Every function is automatically classified as `Pure`, `Yields`, or `Polymorphic`. The compiler enforces this: a pure context cannot call a yielding function. No annotations required. This is what makes the fiber/concurrency story coherent — the compiler knows which functions can suspend.
  <details><summary>Example: Effect System</summary>

  ```janet
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

  The compiler enforces effect contracts: a pure context cannot call a yielding function. This is checked at compile time.
  </details>

- **Fully hygienic macros that operate on syntax objects, not text or s-expressions.** Macros receive and return `Syntax` objects carrying scope information (Racket-style scope sets). Name capture is structurally impossible, not just conventionally avoided. This is stronger than Janet's macros, which are s-expression templates.
  <details><summary>Example: Hygienic Macros</summary>

  ```janet
  (defmacro my-swap (a b)
    `(let ((tmp ,a)) (set ,a ,b) (set ,b tmp)))

  (let ((tmp 100) (x 1) (y 2))
    (my-swap x y)
    tmp)  # => 100, not 1
  ```

  The `tmp` binding introduced by the macro does not shadow the caller's `tmp`. This is guaranteed by the scope set mechanism, not by convention.
  </details>

- **Functions are colorless.** Any function can be called from a fiber. There is no `async`/`await` annotation that marks a function as suspending and forces all its callers to be marked too. Whether something runs concurrently is decided at the call site, not baked into the function definition.
  <details><summary>More: Colorless Functions</summary>

  In languages like Rust, JavaScript, and Python, a function marked `async` infects its entire call graph — every caller must also be `async`. In Elle, a function is just a function. The caller decides whether to wrap it in a fiber. A pure function and a yielding function have the same type, the same calling convention, and the same syntax. The difference is only visible to the compiler's effect analysis, which uses it to optimize, not to restrict.
  </details>

- **Structured concurrency via fibers with per-fiber memory.** Each fiber has its own heap arena. When a fiber finishes, its memory is reclaimed in O(1) — no GC pause, no reference counting. The compiler's escape analysis drives scope-level reclamation within fibers.
  <details><summary>Example: Fibers and Coroutines</summary>

  ```janet
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

  Fibers are independent execution contexts. Each has its own stack, call frames, and heap. When a fiber finishes, its entire heap is freed in O(1). No garbage collection, no reference counting, no pause.
  </details>

- **The Rust ecosystem.** FFI without ceremony. Native plugins as Rust cdylib crates. Values are marshalled directly to C types via libffi — no intermediate serialization format, no separate process, no generated bindings.

## Language

- **Modern Lisp syntax with no parser ambiguity.** Macros operate on syntax trees, not text. See [`prelude.lisp`](prelude.lisp) for hygienic macros and standard forms.

- **Collection literals with mutable/immutable split.** Bare delimiters are immutable: `[1 2 3]` (tuple), `{:key val}` (struct), `"hello"` (string). `@`-prefixed are mutable: `@[1 2 3]` (array), `@{:key val}` (table), `@"hello"` (buffer).
  <details><summary>Example: Collections</summary>

  ```janet
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

  ```janet
  (def (head & tail) (list 1 2 3 4))
  (def [x _ z] [10 20 30])
  (def {:name n :age a} {:name "Bob" :age 25})
  (def {:config {:db {:host h}}}
    {:config {:db {:host "localhost"}}})
  ```
  </details>

- **Closures with automatic capture analysis.** The compiler tracks which variables each closure captures. Mutable captures use cells automatically. Enables escape analysis for scope-level memory reclamation.
  <details><summary>Example: Closures</summary>

  ```janet
  (defn make-counter [start]
    (var n start)
    (fn []
      (set n (+ n 1))
      n))

  (def c (make-counter 0))
  (c)  # => 1
  (c)  # => 2
  ```

  The closure captures `n` by value. The compiler detects that `n` is mutated, so it wraps it in a cell automatically. No explicit `box` or `ref` needed.
  </details>

- **Splice operator for array spreading.** `;expr` marks a value for spreading at call sites and in data constructors. `(splice expr)` is the long form.
  <details><summary>Example: Splice</summary>

  ```janet
  (def args @[2 3])
  (+ 1 ;args)  # => 6, same as (+ 1 2 3)

  (def items @[1 2])
  @[0 ;items 3]  # => @[0 1 2 3]
  ```
  </details>

- **Reader macros for quasiquote and unquote.** `` ` `` for quasiquote, `,` for unquote, `,;` for unquote-splice (inside quasiquote).

- **Parameters for dynamic binding.** `make-parameter` creates a parameter, `parameterize` sets it in a scope, child fibers inherit parent parameter frames.
  <details><summary>Example: Parameters</summary>

  ```janet
  (def *port* (make-parameter :stdout))

  (parameterize ((*port* :stderr))
    (print "to stderr"))  # uses *port* = :stderr

  (print "to stdout")     # uses *port* = :stdout
  ```
  </details>

## Control Flow

- **Conditionals: `if`, `cond`, `when`, `unless`, `case`.** `if` is the primitive, others are macros or sugar.
  <details><summary>Example: Conditionals</summary>

  ```janet
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

- **Pattern matching with `match`.** Type guards, element extraction, nested patterns, wildcard `_`.
  <details><summary>Example: Pattern Matching</summary>

  ```janet
  (match value
    ([a b] (+ a b))
    ({:x x :y y} (+ x y))
    ((cons h t) h)
    (_ "no match"))
  ```
  </details>

- **Error handling: `try`/`catch`, `protect`, `defer`.** Built on fibers and signals, not exceptions.
  <details><summary>Example: Error Handling</summary>

  ```janet
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

  ```janet
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

  ```janet
  (def libc (ffi/native nil))
  (ffi/defbind sqrt libc "sqrt" :double @[:double])
  (sqrt 2.0)  # => 1.4142135623730951
  ```
  </details>

- **Struct marshalling, variadic calls, callbacks, manual memory management all work.**
  <details><summary>Example: Advanced FFI</summary>

  ```janet
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

  ```janet
  # math.lisp
  (fn (scale)
    {:add (fn (a b) (* (+ a b) scale))
     :mul (fn (a b) (* (* a b) scale))})

  # Usage
  (def {:add add :mul mul} ((import-file "math.lisp") 2))
  (add 1 2)  # => 6
  ```
  </details>

- **Native plugins are Rust cdylib crates.** Link against `elle`, export an init function. Plugins register primitives through the same `PrimitiveDef` mechanism as builtins — same effect declarations, same doc strings, same arity checking. Work directly with `Value`. No intermediate serialization format, no separate process, no generated bindings.
  <details><summary>Example: Plugin Usage</summary>

  ```janet
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

- **Static linter catches errors at compile time.** Wrong arity, unused bindings, effect violations, type mismatches in patterns, duplicate pattern variables.
  <details><summary>Example: Static Analysis and Linting</summary>

  ```janet
  # Compile-time errors caught by elle lint:
  (defn foo (x y) (+ x))  # Error: missing argument y
  (let ((unused 42)) 100) # Warning: unused binding
  (fn (a b) (yield))      # Error: pure context, can't yield
  (match x
    ([a b c] a)           # Error: pattern expects 3 elements
    (v v))                # Error: duplicate pattern variable
  ```
  </details>

- **Source-to-source rewriting tool.** The `rewrite` subcommand applies pattern-based rules to Elle source files for refactoring and code generation. Rules are pattern-action pairs that match syntax trees and produce transformed output.

- **Formatter for consistent code style.** The `formatter` subcommand formats Elle source files.

- **Compilation pipeline is fully documented.** See [`docs/pipeline.md`](docs/pipeline.md) for data flow across boundaries and [`AGENTS.md`](AGENTS.md) for architecture details.

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
