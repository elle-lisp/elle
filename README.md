# Elle

[![CI](https://github.com/elle-lisp/elle/actions/workflows/main.yml/badge.svg)](https://github.com/elle-lisp/elle/actions/workflows/main.yml)

Elle is a Lisp. What separates it from other Lisps is the depth of its static analysis: full binding resolution, capture analysis, and effect inference happen at compile time, before any code runs. This gives Elle a sound effect system, fully hygienic macros, colorless concurrency via fibers, and deterministic memory management — all derived from the same analysis pass.

## Contents

- [What Makes Elle Different](#what-makes-elle-different)
- [Language](#language)
- [Types](#types)
- [Control Flow](#control-flow)
- [Memory](#memory)
- [JIT](#jit)
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

- **A sound effect system, inferred not declared.** Every function is automatically classified as `Inert`, `Yields`, or `Polymorphic`. The compiler enforces this: a pure context cannot call a yielding function. No annotations required. This is what makes the fiber/concurrency story coherent — the compiler knows which functions can suspend.

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

  <details><summary>More: Effect Enforcement</summary>

  The compiler enforces effect contracts: a pure context cannot call a yielding function. This is checked at compile time.
  </details>

- **Fully hygienic macros that operate on syntax objects, not text or s-expressions.** Macros receive and return `Syntax` objects carrying scope information (Racket-style scope sets). Name capture is structurally impossible, not just conventionally avoided. This is stronger than Janet's macros, which are s-expression templates.

  ```janet
  (defmacro my-swap (a b)
    `(let ((tmp ,a)) (assign ,a ,b) (assign ,b tmp)))

  (let ((tmp 100) (x 1) (y 2))
    (my-swap x y)
    tmp)  # => 100, not 1
  ```

  <details><summary>More: Scope Sets</summary>

  The `tmp` binding introduced by the macro does not shadow the caller's `tmp`. This is guaranteed by the scope set mechanism, not by convention.
  </details>

- **Functions are colorless.** Any function can be called from a fiber. There is no `async`/`await` annotation that marks a function as suspending and forces all its callers to be marked too. Whether something runs concurrently is decided at the call site, not baked into the function definition.

  ```janet
  # A pure function
  (defn add (a b)
    (+ a b))

  # A yielding function — suspends to fetch a value
  (defn fetch (key)
    (yield :get key))

  # Both called identically — compute is not marked async/await
  (defn compute (a b key)
    (+ (add a b) (fetch key)))
  ```

  <details><summary>More: Colorless Functions</summary>

  In Rust/JS/Python, `fetch` would be `async fn`/`async def`, forcing `compute` to be `async` too, and every caller to `await` it. In Elle, the effect is inferred by the compiler, not declared by the programmer. Callers are unaffected.
  </details>

- **Structured concurrency via fibers with per-fiber memory.** Each fiber has its own heap arena. When a fiber finishes, its memory is reclaimed in O(1) — no GC pause, no reference counting. The compiler's escape analysis drives scope-level reclamation within fibers.

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

  <details><summary>More: Fiber Memory</summary>

  Fibers are independent execution contexts. Each has its own stack, call frames, and heap. When a fiber finishes, its entire heap is freed in O(1). No garbage collection, no reference counting, no pause.
  </details>

- **The Rust ecosystem.** FFI without ceremony. Native plugins as Rust cdylib crates. Values are marshalled directly to C types via libffi — no intermediate serialization format, no separate process, no generated bindings.

## Language

- **Modern Lisp syntax with no parser ambiguity.** Macros operate on syntax trees, not text. See [`prelude.lisp`](prelude.lisp) for hygienic macros and standard forms.

- **Collection literals with mutable/immutable split.** Bare delimiters are immutable: `[1 2 3]` (array), `{:key val}` (struct), `"hello"` (string). `@`-prefixed are mutable: `@[1 2 3]` (@array), `@{:key val}` (@struct), `@"hello"` (@string).

   ```janet
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

   # Bytes and @bytes (no literal syntax)
   (def b (bytes 1 2 3))     # bytes
   (def bl (@bytes 1 2 3))   # @bytes
   ```

- **Strings are sequences of grapheme clusters.** `length`, slicing, indexing, and iteration all count grapheme clusters — not bytes, not codepoints.

  ```janet
  (length "café")           # => 4, not 5 bytes
  (get "café" 3)              # => "é"
  (slice "café" 0 2)        # => "ca"
  (first "café")            # => "c"
  (rest "café")             # => "afé"
  (length "👨‍👩‍👧")   # => 1
  ```

- **Destructuring in all binding positions.** `def`, `let`, `let*`, `var`, `fn` parameters, `match` patterns — missing values become `nil`, wrong types become `nil`.

  ```janet
  (def (head & tail) (list 1 2 3 4))
  (def [x _ z] [10 20 30])
  (def {:name n :age a} {:name "Bob" :age 25})
  (def {:config {:db {:host h}}}
    {:config {:db {:host "localhost"}}})
  ```

- **Closures with automatic capture analysis.** The compiler tracks which variables each closure captures. Mutable captures use cells automatically. Enables escape analysis for scope-level memory reclamation.

  ```janet
  (defn make-counter [start]
    (var n start)
    (fn []
      (assign n (+ n 1))
      n))

  (def c (make-counter 0))
  (c)  # => 1
  (c)  # => 2
  ```

  <details><summary>More: Automatic Cell Wrapping</summary>

  The closure captures `n` by value. The compiler detects that `n` is mutated, so it wraps it in a cell automatically. No explicit `box` or `ref` needed.
  </details>

- **Full tail-call optimisation.** All tail calls are optimised — not just self-recursion. Mutually recursive functions, continuation-passing style, and trampolining all work without stack overflow.

- **Splice operator for array spreading.** `;expr` marks a value for spreading at call sites and in data constructors. `(splice expr)` is the long form.

  ```janet
  (def args @[2 3])
  (+ 1 ;args)  # => 6, same as (+ 1 2 3)

  (def items @[1 2])
  @[0 ;items 3]  # => @[0 1 2 3]
  ```

- **Reader macros for quasiquote and unquote.** `` ` `` for quasiquote, `,` for unquote, `,;` for unquote-splice (inside quasiquote).

- **Parameters for dynamic binding.** `parameter` creates a parameter, `parameterize` sets it in a scope, child fibers inherit parent parameter frames.

  ```janet
  (def *port* (parameter :stdout))

  (parameterize ((*port* :stderr))
    (print "to stderr"))  # uses *port* = :stderr

  (print "to stdout")     # uses *port* = :stdout
  ```

## Types

Every Elle value is a NaN-boxed 64-bit word. Immediates (nil, booleans, integers, floats, symbols, keywords, empty list) fit inline with no allocation. Everything else is a reference-counted pointer into a heap.

### Immediate types

| Type | Literal | Notes |
|------|---------|-------|
| nil | `nil` | Absence of a value. Falsy. |
| boolean | `true`, `false` | `false` is falsy; `true` is truthy. |
| integer | `42`, `-17` | 48-bit signed. No auto-coercion to float. Overflow panics. |
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
| bytes | @bytes | *(no literal)* | *(no literal)* |
| set | @set | `|1 2 3|` | `@|1 2 3|` |

The `@` prefix means "mutable version of this literal." The types within each pair share the same logical structure but differ in mutability.

**string** — interned text. Equality is O(1) via interning. Indexing and length count grapheme clusters, not bytes.

```janet
(def s "café")
(length s)              # => 4 (grapheme clusters, not bytes)
(get s 3)               # => "é"
(slice s 0 2)           # => "ca"
(concat s "!")          # => "café!"
```

**array** — fixed-length sequence.

```janet
(def a [1 2 3])
(get a 0)               # => 1
(length a)              # => 3
(concat a [4 5])        # => [1 2 3 4 5]
```

**struct** — ordered dictionary. Keys are typically keywords.

```janet
(def s {:name "Bob" :age 25})
(get s :name)           # => "Bob"
(keys s)                # => (:name :age)
(values s)              # => ("Bob" 25)
(has? s :name)          # => true
```

**set** — ordered collection of unique values. Mutable values are frozen on insertion.

```janet
(def s |1 2 3|)
(contains? s 2)         # => true
(add s 4)               # => |1 2 3 4|
(del s 1)               # => |2 3|
(union |1 2| |2 3|)     # => |1 2 3|
(intersection |1 2| |2 3|)  # => |2|
(difference |1 2| |2 3|)    # => |1|
```

**bytes** — immutable binary data. No literal syntax. Displays as `#bytes[hex ...]`.

```janet
(def b (bytes 1 2 3))
(def b2 (string->bytes "hello"))
(get b 0)               # => 1
(length b)              # => 5
(bytes->hex b2)         # => "68656c6c6f"
```

**@bytes** — mutable binary data. No literal syntax. Displays as `#@bytes[hex ...]`.

```janet
(def b (@bytes 1 2 3))
(def b2 (string->@bytes "hello"))
(get b 0)               # => 1
(length b)              # => 5
(bytes->hex b2)         # => "68656c6c6f"
```

### Lists

Singly-linked cons cells. Proper lists terminate with `()` (empty list), **not** `nil`.

```janet
(list 1 2 3)            # => (1 2 3)
(cons 1 (list 2 3))     # => (1 2 3)
(first (list 1 2 3))    # => 1
(rest (list 1 2 3))     # => (2 3)
(rest (list 1))          # => ()  — empty list, not nil
```

> **nil vs empty list** — this is the most common gotcha. `nil` represents absence and is **falsy**. `()` is the empty list and is **truthy**. Lists terminate with `()`. Use `empty?` to check for end-of-list, not `nil?`. `nil?` only matches `nil`.

```janet
(nil? nil)              # => true
(nil? ())               # => false  — empty list is not nil
(empty? ())             # => true
(empty? nil)            # => false  — nil is not an empty list
```

Lists are linked; tuples and arrays are contiguous in memory. They are not interchangeable.

### Functions

**Closures** — compiled functions with captured environment. Captures are by value; mutable captures use compiler-managed cells automatically.

```janet
(fn (x) (+ x 1))           # anonymous
(defn add1 (x) (+ x 1))    # named (macro)
```

**Native functions** — Rust primitives (`+`, `-`, `cons`, etc.). Not constructible from Elle.

### Concurrency types

**Fiber** — independent execution context with its own stack, call frames, signal mask, and heap. See [Memory](#memory).

```janet
(fiber/new (fn () body) mask)
(fiber/resume f value)
(fiber/status f)
```

**Parameter** — dynamic binding. `(parameter default)` creates one; calling it reads the current value. `parameterize` sets it within a scope. Child fibers inherit parent parameter frames.

**Cell** — mutable box. User cells are explicit (`box`/`unbox`/`set-box!`). Local cells are compiler-created for mutable captures and auto-unwrapped — users never see them.

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
| `array?` | array |
| `@array?` | @array |
| `@struct?` | @struct |
| `struct?` | struct |
| `set?` | set (immutable or @set) |
| `@string?` | @string |
| `bytes?` | bytes (immutable) |
| `@bytes?` | @bytes (mutable) |
| `function?` | closure or native function |
| `closure?` | closure only |
| `primitive?` | native function only |
| `fiber?` | fiber |
| `pointer?` | raw or managed C pointer |
| `zero?` | zero (integer or float) |
| `type` / `type-of` | returns type as keyword (`:integer`, `:string`, etc.) |

### Display format

| Type | Display |
|------|---------|
| nil | `nil` |
| boolean | `true` / `false` |
| integer | `42` |
| float | `3.14` |
| symbol | `foo` |
| keyword | `:foo` |
| empty list | `()` |
| string | `hello` (no quotes) |
| cons | `(1 2 3)` or `(a . b)` for improper |
| array | `[1 2 3]` |
| @array | `@[1 2 3]` |
| struct | `{:a 1}` |
| @struct | `@{:a 1}` |
| set | `|1 2 3|` |
| @set | `@|1 2 3|` |
| bytes | `#bytes[01 02 03]` |
| @bytes | `#@bytes[01 02 03]` |
| closure | `<closure>` |
| native fn | `<native-fn>` |
| fiber | `<fiber:status>` |
| cell | `<cell value>` |
| @string | `@"hello"` |
| pointer | `<pointer 0x...>` |

## Control Flow

- **Conditionals: `if`, `cond`, `when`, `unless`, `case`.** `if` is the primitive, others are macros or sugar.

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

- **Pattern matching with `match`.** Type guards, element extraction, nested patterns, wildcard `_`, and guard clauses.

  ```janet
  (match value
    (0                    "zero")
    (n when (< n 0)       "negative")
    (n when (> n 0)       "positive")
    ([a b]                (+ a b))
    ({:x x :y y}          (+ x y))
    (_                    "no match"))
  ```

- **Error handling: `try`/`catch`, `protect`, `defer`.** Built on fibers and signals, not exceptions.

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

- **Loops: `while`, `forever`, `break`.** `while` is the primitive, `forever` is a macro, `break` exits a block.

  ```janet
  (while (< i 10)
    (print i)
    (assign i (+ i 1)))

  (forever
    (if (done?) (break) (step)))

  (block :outer
    (each x in xs
      (if (found? x) (break :outer x))))
  ```

## Memory

- **No garbage collector.** Memory is reclaimed deterministically through three mechanisms:
  - **Per-fiber heaps:** Each fiber allocates into a bump arena. When it finishes, the entire heap is freed in O(1) — no traversal, no mark phase, no sweep. Fibers get strong cache locality.
  - **Zero-copy inter-fiber sharing:** Yielding fibers route allocations to a shared arena; parents read directly from shared memory. No deep copy, no serialization.
  - **Escape-analysis-driven scope reclamation:** Compiler analyzes every `let`, `letrec`, `block` scope. When it can prove no allocated value escapes — no captures, no suspension, no outward mutation — it frees allocations at scope exit.

- **NaN-boxed values: 8 bytes per value.** Integers, floats, booleans, nil, symbols, keywords, and short strings (≤6 bytes) fit inline. Everything else is a pointer into a heap.

- **Long-running fiber schedulers don't accumulate garbage.** Each fiber's heap dies with it. Memory is reclaimed at scope exit or fiber death, without pausing the world.

## JIT

- **JIT compilation is fully automatic.** Pure, non-suspending functions are compiled to native code via Cranelift at runtime. No annotations, no opt-in. The compiler's effect system identifies eligible functions; the JIT fires transparently.

## FFI

- **Call C without ceremony.** Load a library, bind a symbol, call it.

  ```janet
  (def libc (ffi/native nil))
  (ffi/defbind sqrt libc "sqrt" :double @[:double])
  (sqrt 2.0)  # => 1.4142135623730951
  ```

- **Struct marshalling, variadic calls, callbacks, manual memory management all work.**

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

- **FFI calls are tagged in the effect system.** Compiler knows where Elle's safety guarantees end and C's begin.

## Modules and Plugins

- **Module system is minimal by design.** `import` loads a file — Elle source or native `.so` plugin — compiles and executes it, returns the last expression's value. No module declarations, no export lists, no special import form. It's a function call.

- **Source modules return their last expression.** A module that defines functions via `def` makes them available as globals; a module that ends with a struct or function hands that value back to the caller.

  ```janet
  # math.lisp
  (fn (scale)
    {:add (fn (a b) (* (+ a b) scale))
     :mul (fn (a b) (* (* a b) scale))})

  # Usage
  (def {:add add :mul mul} ((import "math.lisp") 2))
  (add 1 2)  # => 6
  ```

- **Native plugins are Rust cdylib crates.** Link against `elle`, export an init function. Plugins register primitives through the same `PrimitiveDef` mechanism as builtins — same effect declarations, same doc strings, same arity checking. Work directly with `Value`. No intermediate serialization format, no separate process, no generated bindings.

  ```janet
  (def re (import "target/release/libelle_regex.so"))
  (def pat (re:compile "\\d+"))
  (re:find-all pat "a1b2c3")
  # => ({:match "1" ...} {:match "2" ...} ...)
  ```

- **Five plugins ship with Elle:** regex, sqlite, crypto, random, selkie.

- **Module system is user-replaceable.** `import` is an ordinary primitive. You can wrap it with caching, path resolution, sandboxing, or shadow it entirely.

## Tooling

- **Language server (LSP) for IDE integration.** Real-time diagnostics, hover documentation, jump-to-definition, refactoring support.

- **Static linter catches errors at compile time.** Wrong arity, unused bindings, effect violations, type mismatches in patterns, duplicate pattern variables.

  ```janet
  # Compile-time errors caught by elle lint:
  (defn foo (x y) (+ x))  # Error: missing argument y
  (let ((unused 42)) 100) # Warning: unused binding
  (fn (a b) (yield))      # Error: pure context, can't yield
  (match x
    ([a b c] a)           # Error: pattern expects 3 elements
    (v v))                # Error: duplicate pattern variable
  ```

- **Match exhaustiveness is checked at compile time.** The compiler warns when a match expression has patterns that can never be reached, and when the match may not cover all cases for a known type.

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
