# Elle Debugging Toolkit

## Contents

- [Overview](#overview)
- [1. Introspection Primitives](#1-introspection-primitives)
- [2. Time API](#2-time-api)
- [3. Signal System](#3-signal-system)

## Overview

Elle provides a comprehensive debugging toolkit that lives *inside* the language.
Debugging and benchmarking happen from Elle source — no recompilation, no
throwaway instrumentation code.

## 1. Introspection Primitives

These operate on **values**, not symbols. Pass a closure (or any value) and
get information about it. All are `NativeFn`. Primitives that need VM access use the SIG_RESUME pattern.

### 1.1 Compiler/runtime predicates

| Primitive | Signature | Returns | Notes |
|-----------|-----------|---------|-------|
| `jit?` | `(jit? value)` | `true` or `false` | True if value is a closure with JIT-compiled native code |
| `silent?` | `(silent? value)` | `true` or `false` | True if value is a closure that does not suspend (no yield/debug/polymorphic signal) |
| `coro?` | `(coro? value)` | `true` or `false` | True if value is a closure with the yield signal bit set |
| `mutates-params?` | `(mutates-params? value)` | `true` or `false` | True if value is a closure whose body mutates any of its own parameters (i.e., `lbox_params_mask != 0`) |
| `closure?` | `(closure? value)` | `true` or `false` | True if value is a closure (bytecode, not native/vm-aware) |

Implementation: each is a simple predicate that examines the `Value` and,
for closures, reads fields on the `Closure` struct.

- `jit?` checks `closure.jit_code.is_some()`
- `silent?` checks `!closure.signal.may_suspend()` (no yield/debug bits and propagates == 0)
- `coro?` checks `closure.signal.bits & SIG_YIELD != 0`
- `mutates-params?` checks `closure.lbox_params_mask != 0` (any lbox-wrapped params)
- `closure?` checks `value.as_closure().is_some()`
- `global?` takes a symbol, checks `vm.get_global(sym_id).is_some()`

Note: `lbox_params_mask` tracks which *parameters* are mutated inside the
closure body and need `LocalLBox` wrapping. It does **not** indicate whether
the closure captures mutable bindings from an outer scope. Those are
`LocalLBox` values in the closure's `env` vector — detecting them would
require scanning `env`, which is a different (and more expensive) operation.

### 1.2 Error signal tracking: `fn/errors?`

| Primitive | Signature | Returns | Notes |
|-----------|-----------|---------|-------|
| `fn/errors?` | `(fn/errors? value)` | `true` or `false` | Returns `true` if the closure may signal an error, `false` if it is guaranteed not to. Returns `false` for non-closures. |

This is a boolean query.

### 1.3 Additional introspection

| Primitive | Signature | Returns | Notes |
|-----------|-----------|---------|-------|
| `arity` | `(arity value)` | int, pair, or nil | For closures: exact arity as int, or `(min . max)` pair for range, or `(min . nil)` for variadic. Nil for non-closures. |
| `captures` | `(captures value)` | int or nil | Number of captured variables, or nil for non-closures. |
| `bytecode-size` | `(bytecode-size value)` | int or nil | Size of closure's bytecode in bytes. Nil for non-closures. |

## 2. Time API

Time values are plain floats (f64 seconds). No opaque types, no new heap
variants. This means time values compose naturally with arithmetic: subtract
two timestamps, multiply by 1000 for milliseconds, compare with `<`.

Two namespaces separate concerns: `clock/` for point-in-time readings,
`time/` for operations on durations and convenience wrappers.

### 2.1 Clock primitives (Rust)

| Primitive | Signature | Returns | Signal | Backing |
|-----------|-----------|---------|--------|---------|
| `clock/monotonic` | `(clock/monotonic)` | float | `Signal::inert()` | `std::time::Instant` relative to a process-global epoch |
| `clock/realtime` | `(clock/realtime)` | float | `Signal::inert()` | `std::time::SystemTime::UNIX_EPOCH` |
| `clock/cpu` | `(clock/cpu)` | float | `Signal::inert()` | `libc::clock_gettime(CLOCK_THREAD_CPUTIME_ID)` |

`clock/monotonic` uses a `OnceLock<Instant>` initialized on first call.
All readings are relative to this epoch, keeping values small and maximizing
f64 precision for the deltas that matter.

`clock/realtime` returns seconds since Unix epoch. Microsecond precision
for decades — adequate for wall-clock timestamps.

`clock/cpu` returns thread CPU time in seconds. This is a real syscall
(not vDSO), so it's ~5x slower than `clock/monotonic` (~500ns vs ~80ns).
Use it when you need to distinguish computation time from I/O wait.

### 2.2 Time utilities (Elle)

| Primitive | Signature | Returns | Signal | Implementation |
|-----------|-----------|---------|--------|----------------|
| `time/sleep` | `(time/sleep seconds)` | nil | `Signal::errors()` | `std::thread::sleep` (Rust primitive) |
| `time/stopwatch` | `(time/stopwatch)` | coroutine | yields | Elle: coroutine over `clock/monotonic` |
| `time/elapsed` | `(time/elapsed thunk)` | `(result seconds)` | polymorphic | Elle: wraps thunk with clock reads |

`time/stopwatch` returns a coroutine. Each `coro/resume` yields the total
seconds elapsed since the stopwatch was created:

```janet
(var sw (time/stopwatch))
(coro/resume sw)   # => 0.000234
;# ... do work ...
(coro/resume sw)   # => 1.532100  (cumulative, not delta)
```

Implementation (in `src/primitives/time_def.rs`):

```janet
(def time/stopwatch (fn ()
  (coro/new (fn ()
    (let ((start (clock/monotonic)))
      (while true
        (yield (- (clock/monotonic) start))))))))
```

`time/elapsed` takes a thunk and returns a pair of (result, elapsed-seconds):

```janet
(var result (time/elapsed (fn () (heavy-computation))))
(first result)          # => computation result
(first (rest result))   # => elapsed seconds
```

For hot-path timing where coroutine overhead matters, subtract two
`clock/monotonic` readings directly.

### 2.3 Why floats, not opaque types

An earlier design proposed `HeapObject::Instant` and `HeapObject::Duration`
variants. The float approach is simpler and better:

- **No new heap types.** No changes to `HeapObject`, `HeapTag`, constructors,
  accessors, display, `SendValue`, `PartialEq`, or `Debug`.

- **Composable with arithmetic.** `(- end start)` gives elapsed seconds.
  `(* elapsed 1000)` gives milliseconds. `(< a b)` compares timestamps.

- **Adequate precision.** f64 gives ~nanosecond precision for durations up
  to a few hours (stopwatch use case), and ~microsecond precision for epoch
  timestamps spanning decades.

- **Precedent.** Lua's `os.clock()`, Common Lisp's `get-internal-real-time`,
  JavaScript's `performance.now()` — all return numbers, not opaque types.

## 3. Signal System

### 3.1 Design

The `Signal` struct tracks which signals a function may emit via a `bits`
field (bitmask of `SIG_ERROR`, `SIG_YIELD`, `SIG_DEBUG`, `SIG_FFI`) and
which parameter indices propagate their callee's signals via a `propagates`
bitmask. This handles error, yield, debug, and FFI signals uniformly.

```rust
pub struct Signal {
    pub bits: SignalBits,    // which signals this function itself might emit
    pub propagates: u32,     // bitmask of parameter indices whose signals flow through
}
```

Constructors: `Signal::inert()`, `Signal::errors()`, `Signal::yields()`,
`Signal::yields_errors()`, `Signal::ffi()`, `Signal::polymorphic(n)`,
`Signal::polymorphic_errors(n)`.

Predicates: `may_error()`, `may_yield()`, `may_suspend()`, `may_ffi()`,
`is_polymorphic()`.

### 3.2 Inference rules

| Form | Signals |
|------|--------|
| `(error val)` | `true` — always signals `:error` |
| `(emit bits val)` | `true` — always |
| `(try body (catch e ...))` | `false` — catches all `:error` signals |
| `(begin a b)` | a.signals ∨ b.signals |
| `(if c t e)` | c.signals ∨ t.signals ∨ e.signals |
| `(f args...)` | args.signals ∨ f.may_error |
| literal | `false` |
| primitive call | Uses the primitive's registered `may_error` flag (see §5) |

### 3.3 Key principle: conservative and correct

Every `error` or `emit` call with the `:error` bit is conservatively marked
as "signals." The analyzer doesn't peek into the argument.

`try`/`catch` clears the error signal because it catches `:error` unconditionally.

This is genuinely useful: it tells you which functions are **guaranteed** to
never signal an error. The set of non-error-signalling functions is exactly
the set where every code path avoids `error`/`emit` and calls only
non-error-signalling functions.

### 3.4 Propagation during fixpoint iteration

Error signals propagate exactly like yield signals during the cross-form
fixpoint iteration in `compile_all`. Self-recursive functions start
with `may_error = false` (optimistic) and iterate until stable.

### 3.5 Runtime query

`(fn/errors? value)` reads `closure.signal.may_error()` (checks `SIG_ERROR`
in the signal's bits). Returns `true` if the closure may signal an error,
`false` otherwise.

## 4. Docgen Site

`demos/docgen/generate.lisp` generates the documentation site. CI builds it as part of the docs job. Because it's written in Elle, any change to language semantics can break it.

When the docs CI job fails, check `demos/docgen/generate.lisp` and `demos/docgen/lib/`. Common failure: using `nil?` instead of `empty?` for list termination (see nil vs empty list distinction in root AGENTS.md and `docs/oddities.md`).
