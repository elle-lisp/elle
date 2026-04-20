# Signal Inference

## Signal Restrictions

### `(silence ...)` Form

Declares signal bounds on a function or its parameters. Appears as a preamble declaration in lambda bodies (after optional docstring, before first non-declaration expression).

**Syntax:**
```text
# Function-level restriction (no signals)
(silence)

# Parameter-level restriction (parameter must be silent)
(silence param)
```

**Semantics:**

- `(silence)` — This function emits no signals (silent)
- `(silence param)` — Parameter `param` must be silent (no signals)
- Signal keywords are not accepted. Use `(squelch ...)` for targeted signal restrictions.
- Multiple `silence` forms allowed in one lambda (one per parameter + one function-level)
- Parameter names must match declared parameters
- Duplicate restrictions for the same parameter: the last one wins

**Outside lambda bodies**, `silence` is a call to the stdlib `silence` function, which signals `:error` at runtime. `silence` is implemented as:
```text
(defn silence [& _]
  (error {:error :invalid-silence
          :message "silence must appear in a function body preamble"}))
```

**Examples:**
```text
# Silent function
(defn add (x y)
  (silence)
  (+ x y))

# Higher-order function with silent callback
(defn apply-silent (f x)
  "Apply f to x, requiring f to be silent."
  (silence f)
  (f x))

# Parameter restriction only — f must be silent
(defn map-safe (f xs)
  "Map f over xs. f must be silent."
  (silence f)
  (map f xs))
```

### `squelch` Primitive: Closure Transform with Compile-Time Inference

`squelch` is a **primitive function** that takes a closure and a signal
specifier and returns a new closure with signal enforcement. It is NOT a
preamble declaration.

**Syntax:** `(squelch closure :keyword)` or `(squelch closure |:kw1 :kw2|)`

**Arity:** Exactly 2 — closure + keyword or set.

**Runtime semantics:**

- Returns a **new** closure that, when called, intercepts signals matching
  the mask and converts them to `:error` with kind `"signal-violation"`
- The returned closure shares the same bytecode and environment (Rc clones)
  — near-zero cost, just swaps the closure header
- Composable: layering squelch calls ORs the masks together
- The returned closure's `effective_signal()` reflects the squelch mask
  (squelched bits are cleared, `SIG_ERROR` is added only if the original
  closure could emit those bits)

**Compile-time semantics:**

When the analyzer sees `(squelch f :kw)` where both arguments are
statically known, it computes the resulting signal at compile time using
the same algebra as `Closure::effective_signal()`:

1. Get `f`'s compile-time signal (from `signal_env` or `projection_env`)
2. Resolve the mask from the keyword or set literal
3. Compute: `result = f_signal.squelch(mask)`

The computed signal is propagated to the binding:

```text
(defn producer [] (yield 1))      # signal: {:yield}
(def safe (squelch producer :yield))
# safe's compile-time signal: {:error}  (yield removed, error added)
```

This enables `(silence)` on functions that call squelched imports — the
compiler can prove the function is silent without waiting for runtime.

**Contrast with `silence`:**

`silence` is a **compile-time total suppressor**: `(silence f)` means f
must be completely silent — no signals at all. It is a preamble
declaration inside lambda bodies.

`squelch` is a **runtime blacklist** (open-world): `(squelch f :yield)`
returns a new closure that forbids `:yield` at the call boundary.
Everything else is allowed, including user-defined signals not listed.
It is a primitive function that can appear anywhere an expression is valid.

**Examples:**
```text
(defn f [] (yield 42))

# Squelch a single signal
(def safe-f (squelch f :yield))

# Squelch multiple signals with a set
(def f2 (squelch f |:yield :io|))
```

**Error cases:**

| Condition | Error |
|-----------|-------|
| `(squelch f)` with no mask | arity error |
| `(squelch non-closure :yield)` | type-error |
| `(squelch f non-keyword)` | type-error |

**Known limitation:** Squelch enforcement does not fire when the squelched
closure is invoked in tail position (tracked as issue #588). The squelch
boundary is at the call site in `call_inner`; tail calls bypass this check.


## Compile-Time Verification

### Signal Inference with Bounds

Every lambda has `inferred_signals` — the minimum guaranteed set of signals the lambda may produce. It is always present (never Optional) and is accumulated from:

1. **Direct signal emissions** in the body (e.g., `(yield x)`, `(error "msg")`)
2. **Signals of internal calls** to statically-known functions — their `inferred_signals` bits propagate upward
3. **Signals contributed by parameter calls:**
    - If a parameter has a `silence` bound, its bound's bits are included in `inferred_signals`
    - If a parameter has NO bound, it contributes conservatively (Yields)

The `inferred_signals: Signal` field is always present and contains the minimum guaranteed set of signals the lambda may produce.

**Silence bounds (total suppression):** The programmer-supplied ceiling constraint from `(silence)` declares that the function must emit no signals. When a `silence` bound is present, the compiler checks that `inferred_signals.bits == 0`. If the check fails, compile-time error.

**Example:**
```text
# Function with parameter bound
(defn apply-silent (f x)
  (silence f)  # f must be silent
  (f x))

# Inferred signal: silent (because f is bounded to silent)
# No polymorphism — f's signal is known to be zero bits

# This works: + is silent
(apply-silent + 42)

# Passing a yielding function would fail at compile time:
# (apply-silent (fn () (yield 1)) 42)
# => error: closure may emit {:yield} but parameter is restricted to {}
```

### Silence Bounds Eliminate Polymorphism

A function with `(silence f)` is no longer polymorphic with respect to `f`. The compiler knows `f` must be silent, so the function's signal is determined by its own body only, not by what `f` might do.

**Example:**
```text
# Without bound: polymorphic
(defn map-any (f xs)
  (map f xs))
# Signal: Polymorphic(0) — depends on f's signal

# With silence bound: not polymorphic
(defn map-silent (f xs)
  (silence f)
  (map f xs))
# Signal: silent — f is guaranteed silent, so map is silent
```

### Call-Site Checking

When a concrete function is passed to a parameter with a bound, the analyzer checks the argument's signal against the bound at compile time.

**Example:**
```text
(defn apply-silent (f x)
  (silence f)
  (f x))

# Compile-time check passes: + is silent
(apply-silent + 42)

# Passing a yielding function would fail:
# (apply-silent (fn () (yield 1)) 42)
# => error: argument violates signal bound
```

## Cross-File Signal Inference: Signal Projection

Elle's signal inference operates within a single file via the **fixpoint
loop** (see [pipeline.md](../pipeline.md)). Cross-file signal inference
uses a different mechanism: **signal projection**.

### Signal Projection

When a file returns a struct of closures (the standard module convention),
the compiler extracts a **signal projection** — a mapping from keyword
field names to the signals of the closures they hold. This projection is
cached by file path and reused by all importers.

**The load-bearing convention:** Signal projection only works when the
file's return expression is a struct literal (or a lambda whose body is a
struct literal). This is exactly the closure-as-module convention
documented in [modules.md](../modules.md). If a file returns a dynamic
or computed value, projection falls back to conservative (Polymorphic).

| Return shape | Projectable? |
|---|---|
| `{:add add :double double}` (struct literal) | Yes |
| `(fn [] {:add add :double double})` (closure-as-module) | Yes |
| `(begin ... {:add add})` | Yes (last expression) |
| `(if flag a b)` | Yes (union of branches) |
| Dynamic / computed | No — Polymorphic (same as before) |

**How it works:**

1. `compile_file` analyzes the file and calls `compute_signal_projection`
   on the last binding's value expression
2. The projection is stored on `Bytecode.signal_projection` and cached
   in a thread-local `PROJECTION_CACHE` by resolved file path
3. When the importing file's analyzer sees `((import "std/math"))` — a
   call wrapping a call to `import` with a literal string — it looks up
   the target file's cached projection
4. The projection is recorded on the binding in `projection_env`
5. When the analyzer desugars `math:add` → `(get math :add)`, it looks
   up `:add` in the binding's projection and uses the projected signal

**Example:**

```text
# math.lisp — projection: {:add → {:error}, :double → {:error}}
(defn add [x y] (+ x y))
(defn double [x] (* x 2))
(fn [] {:add add :double double})
```

```text
# user.lisp — projection gives the compiler cross-file signal data
(def math ((import "std/math")))

# math:add has signal {:error}, not Polymorphic
(defn compute [x]
  (silence)           # compiler can prove this!
  (+ (math:add x 10) (math:double x)))
```

### Composition with Compile-Time Squelch

Signal projection and compile-time squelch compose: projection gives the
compiler cross-file signal data, squelch gives it effect subtraction.

```text
(def math ((import "std/math")))
(def safe-add (squelch math:add :error))
# Compile-time squelch:
#   math:add signal = {:error} (from projection)
#   squelch mask = {:error}
#   result signal = {} (silent!)

(defn compute [x]
  (silence)           # compiler proves this!
  (safe-add x 10))
```

### Mutual Recursion Across Files

Fixpoint convergence for mutually recursive definitions operates within
a single file. Mutual recursion across file boundaries does not benefit
from cross-form convergence — each import is a separate compilation.
This is a design choice: files are the unit of compilation, and the
module system's dynamic semantics (parameterized modules, stateful
modules) require treating each import as independent.

### Fallback: Dynamic Modules

When projection is not available (the file returns a computed value, or
the import path is not a literal string), the analyzer falls back to
treating imported values as Polymorphic. Use `squelch` at the call site
to establish signal bounds:

```text
(def b ((import "b.lisp")))

(defn use-b [x y]
  (silence)
  ((squelch b:add |:yield :io|) x y))
```

**Note for agents:** The [MCP knowledge graph](../mcp.md) provides
additional cross-file visibility via SPARQL queries. See
[Agent Reasoning in Elle](../analysis/agent-reasoning.md) for how to
query cross-file dependencies.


### `attune` Primitive: Positive Runtime Enforcement

`attune` is the dual of `squelch`: where squelch says "block these signals"
(negative/blacklist), attune says "allow only these signals"
(positive/whitelist). Everything not in the permitted set is intercepted
and converted to `:error`.

**Syntax:** `(attune signals closure)` — mask-first argument order.

**Runtime semantics:**

- Returns a new closure whose squelch mask suppresses `CAP_MASK - permitted`
- Same mechanism as squelch (Rc clone, near-zero cost)
- Composable with squelch: layers OR their masks together

**Compile-time semantics:**

When the analyzer sees `(attune |:yield| f)` with a static mask, it
computes the resulting signal: `f_signal.squelch(CAP_MASK - permitted)`.
This enables interprocedural signal narrowing.

**Examples:**
```text
# Allow only :yield and :error — block everything else
(def safe-handler (attune |:yield :error| (get-handler)))

# Equivalent to (squelch f |:io :ffi :exec :halt :debug|) — but readable
(def no-side-effects (attune |:yield :error| some-callback))

# Compose with squelch
(def only-error (squelch (attune |:yield :error| f) :yield))
```

### `(attune! signal-spec)` Form

Compile-time preamble declaration that sets the function's signal ceiling.
Generalizes `(silence)`: where silence means "emits nothing", attune!
means "emits at most these signals."

**Syntax:**
```text
(attune! :keyword)           # ceiling = single signal
(attune! |:kw1 :kw2|)       # ceiling = set of signals
```

**Semantics:**

- Declares the maximum signal this function may emit
- Compiler verifies the body's inferred signal fits within the ceiling
- If the body exceeds the ceiling, compile-time error
- Composes with `(muffle ...)`: muffled bits expand the ceiling

**Examples:**
```text
# Function may yield but nothing else
(defn generator [n]
  (attune! :yield)
  (yield n))

# Function may yield and error, but no I/O
(defn parser [input]
  (attune! |:yield :error|)
  (if (empty? input)
    (error {:error :parse-error})
    (yield (first input))))

# Exceeding the ceiling is a compile-time error:
# (defn bad []
#   (attune! :yield)
#   (println "oops"))   # => error: function restricted to {:yield} but body may emit {:io}
```

## Compile-Time Assertions (`!` Convention)

Forms ending with `!` are compile-time assertions with implications for
analysis. They are promises the programmer makes that the compiler
verifies and uses to unlock optimizations. If violated, the program is
rejected at compile time.

| Form | Assertion | Optimization unlocked |
|------|-----------|----------------------|
| `(silent!)` | Function emits no signals | Skip signal dispatch, JIT without suspension |
| `(numeric!)` | All values are numeric | Elide type checks, enable GPU lowering |
| `(immutable! x)` | Binding x is never assigned | SSA treatment, avoid cell indirection |
| `(attune! spec)` | Function emits at most spec | Narrow signal ceiling for callers |

**Rules:**

- Must appear inside a lambda body (preamble position)
- Multiple `!` forms allowed in one lambda
- Violation is always a compile-time error, never a runtime check
- These are NOT runtime guards — they inform the compiler's static model

**Examples:**
```text
# GPU kernel: numeric + silent
(defn mandel-pixel [cx cy max-iter]
  (numeric!)
  (silent!)
  (let [@x 0.0  @y 0.0  @i 0]
    (while (and (< (+ (* x x) (* y y)) 4.0) (< i max-iter))
      (let [xt (+ (- (* x x) (* y y)) cx)]
        (assign y (+ (* 2.0 x y) cy))
        (assign x xt)
        (assign i (+ i 1))))
    i))
```

## Runtime Verification

When a closure is passed to a function with a signal bound, the runtime checks that the closure's signal satisfies the bound. This is necessary for dynamic arguments where the signal cannot be determined at compile time.

### Silence Bounds (Total Suppression Check)

**Mechanism:**
- The lowerer emits a `CheckSignalBound` instruction at function entry for each silence-bounded parameter
- The VM checks: `closure.signal.bits != 0` (any bits set → violation)
- If the check fails, the VM signals `:error` with a descriptive message

**Example:**
```text
(defn apply-silent (f x)
  (silence f)
  (f x))

# At runtime, if f's signal violates the bound, error is signaled:
# (apply-silent some-yielding-fn 42)
# => Runtime error: closure may emit {:yield} but parameter must be silent
```

### Squelch Enforcement (Runtime Closure Transform)

`squelch` is a runtime primitive, not a compile-time bound. When a squelched closure is called, the VM checks if the returned signal matches the squelch mask. If it does, the signal is converted to a `signal-violation` error.

**Mechanism:**
- `(squelch f :yield)` returns a new closure with `squelch_mask` set to the `:yield` bit
- When the squelched closure is called via `call_inner`, after `execute_bytecode_saving_stack` returns, the VM checks: `closure.squelch_mask & signal_bits != 0`
- If the check fails (squelched signal detected), the VM converts to `:error` with kind `"signal-violation"`
- Non-squelched signals pass through normally; errors are never affected by squelch

**Example:**
```text
# Squelch a yielding closure — signal-violation at boundary
(def squelched (squelch (fn [] (yield 1)) :yield))
(try (squelched) (catch e (get e :error)))  # => :signal-violation

# Squelch multiple signals with a set
(def sq2 (squelch (fn [] (yield 1)) |:yield :io|))
(try (sq2) (catch e (get e :error)))  # => :signal-violation

# Composable: layer restrictions from different sources
(defn make-safe [f]
  (squelch f :yield))
(def extra-safe (squelch (make-safe (fn [] (yield 1))) :io))
```


## Surface Syntax

### Fiber Primitives

```text
# === Creation and control ===
# (fiber/new fn mask) => fiber
# (fiber/resume fiber value) => signal-bits
# (emit bits value) => suspends

# === Introspection ===
# (fiber/status fiber) => :new :alive :suspended :dead :error
# (fiber/value fiber) => value
# (fiber/bits fiber) => int
# (fiber/mask fiber) => int

# === Chain traversal ===
# (fiber/parent fiber) => fiber or nil
# (fiber/child fiber) => fiber or nil
```

### Sugar and Aliases

```text
# try/catch
# (try body (catch e handler))

# yield is sugar for (emit :yield value)
# error is sugar for (emit 1 value)

# coro/ aliases
# (coro/new fn) => (fiber/new fn |:yield|)
# (coro/resume co val) => (fiber/resume co val)
# (coro/status co) => (fiber/status co)
```

### Signal Restrictions

```text
# Compile-time silence bounds
(defn silent-add (x y)
  (silence)           # no signals — silent
  (+ x y))

(defn callback-must-be-silent (f xs)
  (silence f) # f must have no signals
  (map f xs))

# Squelch: runtime enforcement + compile-time inference
(defn safe-apply (f x)
  (let [safe-f (squelch f :yield)]  # returns a new closure
    (safe-f x)))                    # safe-f's signal: {:error}

# Squelch with imported module (projection + squelch compose)
(def math ((import "std/math")))
(def safe-add (squelch math:add :error))
(defn compute [x]
  (silence)           # compiler proves this via projection + squelch
  (safe-add x 10))
```

---

## See also

- [Signal index](index.md)
