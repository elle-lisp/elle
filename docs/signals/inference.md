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
```
(defn silence [& _]
  (error {:error :invalid-silence
          :message "silence must appear in a function body preamble"}))
```

**Examples:**
```lisp
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

### `squelch` Primitive: Runtime Closure Transform

`squelch` is a **primitive function** that takes a closure and returns a new closure with runtime signal enforcement. It is NOT a preamble declaration.

**Syntax:** `(squelch closure :keyword)`

**Semantics:**

- Takes a closure as the first argument and a signal keyword as the second
- Returns a **new** closure that, when called, intercepts signals matching the keyword and converts them to `:error` with kind `"signal-violation"`
- The returned closure shares the same bytecode and environment (Rc clones)
  — near-zero cost, just swaps the closure header
- Accepts a keyword or a set: `(squelch f |:yield :io|)`
- Composable: layering squelch calls ORs the masks together
- The returned closure's `effective_signal()` reflects the squelch mask (squelched bits are cleared, `SIG_ERROR` is added only if the original closure could emit those bits)

**Contrast with `silence`:**

`silence` is a **compile-time total suppressor**: `(silence f)` means f must be completely silent — no signals at all. It is a preamble declaration inside lambda bodies.

`squelch` is a **runtime blacklist** (open-world): `(squelch f :yield)` returns a new closure that forbids `:yield` at the call boundary. Everything else is allowed, including user-defined signals not listed. It is a primitive function that can appear anywhere an expression is valid.

**Examples:**
```lisp
(defn f [] (yield 42))

# Squelch a single signal
(def safe-f (squelch f :yield))

# Squelch multiple signals with a set
(def f2 (squelch f |:yield :io|))
```

**Error cases:**

| Condition | Error |
|-----------|-------|
| `(squelch f)` with no keyword | arity error |
| `(squelch non-closure :yield)` | type-error |
| `(squelch f non-keyword)` | type-error |

**Known limitation:** Squelch enforcement does not fire when the squelched closure is invoked in tail position (tracked as issue #588). The squelch boundary is at the call site in `call_inner`; tail calls bypass this check.


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
```lisp
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
```lisp
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
```lisp
(defn apply-silent (f x)
  (silence f)
  (f x))

# Compile-time check passes: + is silent
(apply-silent + 42)

# Passing a yielding function would fail:
# (apply-silent (fn () (yield 1)) 42)
# => error: argument violates signal bound
```


## Runtime Verification

When a closure is passed to a function with a signal bound, the runtime checks that the closure's signal satisfies the bound. This is necessary for dynamic arguments where the signal cannot be determined at compile time.

### Silence Bounds (Total Suppression Check)

**Mechanism:**
- The lowerer emits a `CheckSignalBound` instruction at function entry for each silence-bounded parameter
- The VM checks: `closure.signal.bits != 0` (any bits set → violation)
- If the check fails, the VM signals `:error` with a descriptive message

**Example:**
```lisp
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
```lisp
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

```lisp
# === Creation and control ===
# (fiber/new mask fn) => fiber
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

```lisp
# try/catch
# (try body (catch e handler))

# yield is sugar for (emit :yield value)
# error is sugar for (emit 1 value)

# coro/ aliases
# (coro/new fn) => (fiber/new |:yield| fn)
# (coro/resume co val) => (fiber/resume co val)
# (coro/status co) => (fiber/status co)
```

### Signal Restrictions

```lisp
# Compile-time silence bounds
(defn silent-add (x y)
  (silence)           # no signals — silent
  (+ x y))

(defn callback-must-be-silent (f xs)
  (silence f) # f must have no signals
  (map f xs))

# Runtime squelch transform
(defn safe-apply (f x)
  (let ((safe-f (squelch f :yield)))  # returns a new closure
    (safe-f x)))

(defn safe-iterate (f xs)
  (let ((safe-f (squelch f :yield)))  # f must not yield; other signals allowed
    (map safe-f xs)))
```

---

## See also

- [Signal index](index.md)
