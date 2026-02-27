# Elle Debugging Toolkit

> Design document. February 2026.

## Motivation

Debugging Elle programs currently requires rebuilding the Rust binary to add
instrumentation, then removing it afterward. This cycle is slow, wasteful,
and error-prone. Timing primitives (`clock/monotonic`, `clock/realtime`, `clock/cpu`,
`time/sleep`, `time/stopwatch`, `time/elapsed`) and closure introspection
primitives (`closure?`, `jit?`, `pure?`, `coro?`, `mutates-params?`,
`raises?`, `arity`, `captures`, `bytecode-size`) have been implemented.

This document specifies a debugging toolkit that lives *inside* the language.
Once implemented, debugging and benchmarking happen from Elle source — no
recompilation, no throwaway instrumentation code.

### Dependency philosophy

Elle is a Rust project. We leverage the Rust ecosystem freely — if a
well-maintained crate does what we need at the right size and scope, we pull
it in rather than writing our own. We do not, however, spread our
dependencies into other languages: no C libraries via FFI for internal
tooling, no build-time code generation in other languages, no shelling out
to non-Rust tools. The standard library and crates.io are our toolkit.

This applies throughout this document. Where earlier drafts referenced POSIX
APIs and the `libc` crate, the design now uses Rust-native equivalents from
`std` and targeted crates.

> **Known inconsistency**: The existing `memory-usage` primitive shells out
> to `ps` on macOS and `powershell` on Windows. On Linux it reads
> `/proc/self/status` directly. A future cleanup should replace the
> non-Linux paths with a Rust crate (e.g., `sysinfo`), but that's outside
> the scope of this document.

## 1. Introspection Primitives

These operate on **values**, not symbols. Pass a closure (or any value) and
get information about it. All are `NativeFn`. Primitives that need VM access use the SIG_RESUME pattern.

### 1.1 Compiler/runtime predicates

> **Status: Partially implemented.** `jit?`, `pure?`, `coro?`, `mutates-params?`, and `closure?` are registered and working. `global?` is not yet implemented.

| Primitive | Signature | Returns | Notes |
|-----------|-----------|---------|-------|
| `jit?` | `(jit? value)` | `true` or `false` | True if value is a closure with JIT-compiled native code |
| `pure?` | `(pure? value)` | `true` or `false` | True if value is a closure with `Effect::Pure` |
| `coro?` | `(coro? value)` | `true` or `false` | True if value is a closure with `Effect::Yields` |
| `global?` | `(global? sym)` | `true` or `false` | **Not yet implemented.** True if symbol is bound as a global. Requires VM access (SIG_RESUME). |
| `mutates-params?` | `(mutates-params? value)` | `true` or `false` | True if value is a closure whose body mutates any of its own parameters (i.e., `cell_params_mask != 0`) |
| `closure?` | `(closure? value)` | `true` or `false` | True if value is a closure (bytecode, not native/vm-aware) |

Implementation: each is a simple predicate that examines the `Value` and,
for closures, reads fields on the `Closure` struct.

- `jit?` checks `closure.jit_code.is_some()`
- `pure?` checks `closure.effect == Effect::Pure`
- `coro?` checks `closure.effect == Effect::Yields`
- `mutates-params?` checks `closure.cell_params_mask != 0` (any cell-wrapped params)
- `closure?` checks `value.as_closure().is_some()`
- `global?` takes a symbol, checks `vm.get_global(sym_id).is_some()`

Note: `cell_params_mask` tracks which *parameters* are mutated inside the
closure body and need `LocalCell` wrapping. It does **not** indicate whether
the closure captures mutable bindings from an outer scope. Those are
`LocalCell` values in the closure's `env` vector — detecting them would
require scanning `env`, which is a different (and more expensive) operation.

### 1.2 JIT trigger

> **Status: Not yet implemented.** Neither `jit` nor `jit!` are registered as primitives. The JIT compiler exists internally but has no Elle-level trigger.

| Primitive | Signature | Returns | Notes |
|-----------|-----------|---------|-------|
| `jit` | `(jit value)` | closure | Triggers JIT compilation if value is a closure with `lir_function`. Returns the closure (with `jit_code` populated if compilation succeeded). Does not mutate any global. Requires VM access (SIG_RESUME). |
| `jit!` | `(jit! sym)` | closure | Takes a symbol, looks up the global, JIT-compiles it, and **replaces the global binding** with the JIT-compiled closure. Returns the new value. Requires VM access (SIG_RESUME). |

`jit` is the pure-style API: give it a closure, get back a closure that may
now have JIT code. `jit!` is the imperative API: give it a symbol naming a
global, and it mutates the global in place.

Both require the closure to have a `lir_function`. If the closure has no LIR
(e.g., it's a native fn or already lost its LIR), they return the value
unchanged (for `jit`) or signal an error (for `jit!`).

### 1.3 Exception tracking: `raises?`

> **Status: Implemented.** `raises?` is registered and works. It reads `closure.effect.may_raise()`, which checks `SIG_ERROR` in the effect's signal bits. No separate `Raises` effect extension was needed — the signal-bits-based `Effect` struct already tracks this.

| Primitive | Signature | Returns | Notes |
|-----------|-----------|---------|-------|
| `raises?` | `(raises? value)` | `true` or `false` | Returns `true` if the closure may raise an exception, `false` if it is guaranteed not to. Returns `false` for non-closures. |

This is a boolean query. When we add specific exception type tracking in the
future (§4.6), the return type will change to a list of exception type
keywords. See §4.5 for details.

### 1.4 Additional introspection

> **Status: Partially implemented.** `arity`, `captures`, and `bytecode-size` are registered and working. `call-count` is not yet implemented.

| Primitive | Signature | Returns | Notes |
|-----------|-----------|---------|-------|
| `arity` | `(arity value)` | int, pair, or nil | For closures: exact arity as int, or `(min . max)` pair for range, or `(min . nil)` for variadic. Nil for non-closures. |
| `captures` | `(captures value)` | int or nil | Number of captured variables, or nil for non-closures. |
| `call-count` | `(call-count value)` | int | **Not yet implemented.** Number of times this closure has been called (from VM's hotness tracker). Requires VM access (SIG_RESUME). |
| `bytecode-size` | `(bytecode-size value)` | int or nil | Size of closure's bytecode in bytes. Nil for non-closures. |

## 2. Time API

> **Status: Implemented.** All clock and time primitives are registered and working. See `src/primitives/time.rs` (Rust primitives) and `src/primitives/time_def.rs` (Elle definitions for `time/stopwatch` and `time/elapsed`). Property tests in `tests/integration/time_property.rs`.

Time values are plain floats (f64 seconds). No opaque types, no new heap
variants. This means time values compose naturally with arithmetic: subtract
two timestamps, multiply by 1000 for milliseconds, compare with `<`.

Two namespaces separate concerns: `clock/` for point-in-time readings,
`time/` for operations on durations and convenience wrappers.

### 2.1 Clock primitives (Rust)

| Primitive | Signature | Returns | Effect | Backing |
|-----------|-----------|---------|--------|---------|
| `clock/monotonic` | `(clock/monotonic)` | float | `Effect::none()` | `std::time::Instant` relative to a process-global epoch |
| `clock/realtime` | `(clock/realtime)` | float | `Effect::none()` | `std::time::SystemTime::UNIX_EPOCH` |
| `clock/cpu` | `(clock/cpu)` | float | `Effect::none()` | `libc::clock_gettime(CLOCK_THREAD_CPUTIME_ID)` |

`clock/monotonic` uses a `OnceLock<Instant>` initialized on first call.
All readings are relative to this epoch, keeping values small and maximizing
f64 precision for the deltas that matter.

`clock/realtime` returns seconds since Unix epoch. Microsecond precision
for decades — adequate for wall-clock timestamps.

`clock/cpu` returns thread CPU time in seconds. This is a real syscall
(not vDSO), so it's ~5x slower than `clock/monotonic` (~500ns vs ~80ns).
Use it when you need to distinguish computation time from I/O wait.

### 2.2 Time utilities (Elle)

| Primitive | Signature | Returns | Effect | Implementation |
|-----------|-----------|---------|--------|----------------|
| `time/sleep` | `(time/sleep seconds)` | nil | `Effect::raises()` | `std::thread::sleep` (Rust primitive) |
| `time/stopwatch` | `(time/stopwatch)` | coroutine | yields | Elle: coroutine over `clock/monotonic` |
| `time/elapsed` | `(time/elapsed thunk)` | `(result seconds)` | polymorphic | Elle: wraps thunk with clock reads |

`time/stopwatch` returns a coroutine. Each `coro/resume` yields the total
seconds elapsed since the stopwatch was created:

```lisp
(var sw (time/stopwatch))
(coro/resume sw)   # => 0.000234
;# ... do work ...
(coro/resume sw)   # => 1.532100  (cumulative, not delta)
```

Implementation (in `src/primitives/time_def.rs`):

```lisp
(def time/stopwatch (fn ()
  (coro/new (fn ()
    (let ((start (clock/monotonic)))
      (while true
        (yield (- (clock/monotonic) start))))))))
```

`time/elapsed` takes a thunk and returns a pair of (result, elapsed-seconds):

```lisp
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

### 2.4 Deferred

These have no current consumer. Add when needed:

- **`time/unix->parts`** / **`time/parts->unix`**: Calendar decomposition
  (requires `chrono` or hand-rolled math).

## 3. Benchmarking Macro

> **Status: Not yet working — requires fix to defmacro gensym expansion.** The `defmacro` + `gensym` + quasiquote combination is broken in Elle: gensym symbols inside quasiquoted templates don't expand correctly, producing unevaluated syntax instead of executable code. `lib/bench.lisp` does not exist yet.

A `defmacro`-based benchmarking facility, written in Elle, that wraps the
time primitives. Lives in `lib/bench.lisp` (loaded via `import-file`).

### 3.1 Core: `bench`

```lisp
(defmacro bench (label expr)
  (let ((start (gensym))
        (result (gensym))
        (dt (gensym)))
    `(let* ((,start (clock/monotonic))
            (,result ,expr)
            (,dt (- (clock/monotonic) ,start)))
       (display ,label)
       (display ": ")
       (display ,dt)
       (display "s\n")
       ,result)))
```

Usage: `(bench "fibonacci(30)" (fibonacci 30))` prints timing, returns result.

### 3.2 Iteration: `bench-n`

```lisp
(defmacro bench-n (label n expr)
  (let ((i (gensym))
        (start (gensym))
        (dt (gensym))
        (result (gensym))
        (count (gensym)))
    `(let* ((,count ,n)
            (,start (clock/monotonic))
            (,result nil))
       (var ,i 0)
       (while (< ,i ,count)
         (set ,result ,expr)
         (set ,i (+ ,i 1)))
       (let ((,dt (- (clock/monotonic) ,start)))
         (display ,label)
         (display " (")
         (display ,count)
         (display " iterations): ")
         (display ,dt)
         (display "s (")
         (display (/ ,dt ,count))
         (display "s/iter)\n")
         ,result))))
```

These live in `lib/bench.lisp` and are imported by test scripts.

## 4. Effect System Extension: Raises

> **Status: Implemented (differently than originally proposed).** The `Effect` type was restructured as a signal-bits-based struct, not the `YieldBehavior` + `may_raise: bool` design proposed below. The actual implementation uses `{ bits: SignalBits, propagates: u32 }` where `bits` is a bitmask of signal types (`SIG_ERROR`, `SIG_YIELD`, `SIG_DEBUG`, `SIG_FFI`). `may_raise()` is a method that checks `bits & SIG_ERROR != 0`. There is no separate `may_raise` bool field — it's encoded in the signal bits. There is no `YieldBehavior` enum. The `Closure` struct stores an `Effect` directly (not a separate `may_raise: bool`).

### 4.1 Design (as implemented)

The `Effect` struct tracks which signals a function may emit via a `bits`
field (bitmask of `SIG_ERROR`, `SIG_YIELD`, `SIG_DEBUG`, `SIG_FFI`) and
which parameter indices propagate their callee's effects via a `propagates`
bitmask. This is more general than the original `YieldBehavior` + `may_raise`
proposal — it handles error, yield, debug, and FFI effects uniformly.

```rust
pub struct Effect {
    pub bits: SignalBits,    // which signals this function itself might emit
    pub propagates: u32,     // bitmask of parameter indices whose effects flow through
}
```

Constructors: `Effect::none()`, `Effect::raises()`, `Effect::yields()`,
`Effect::yields_raises()`, `Effect::ffi()`, `Effect::polymorphic(n)`,
`Effect::polymorphic_raises(n)`.

Predicates: `may_raise()`, `may_yield()`, `may_suspend()`, `may_ffi()`,
`is_polymorphic()`.

We do NOT attempt to track specific exception types in this iteration. Any
`throw` is conservatively marked as "raises." Specific type tracking (which
would let `try`/`catch` subtract known types) is a future refinement.

### 4.2 Inference rules

| Form | Raises |
|------|--------|
| `(throw expr)` | `true` — always, regardless of argument |
| `(try body (catch exception e ...))` | `false` — exception is the root type, catches everything |
| `(try body (catch error e ...))` | body.raises — catching a subtype doesn't guarantee all exceptions are caught |
| `(begin a b)` | a.raises ∨ b.raises |
| `(if c t e)` | c.raises ∨ t.raises ∨ e.raises |
| `(f args...)` | args.raises ∨ f.may_raise |
| literal | `false` |
| primitive call | Uses the primitive's registered `may_raise` flag (see §5) |

### 4.3 Key principle: conservative and correct

Every `throw` is an unknown exception. We don't peek into the argument to
determine the type — `(throw (error "x"))` and `(throw some-variable)` both
produce `raises = true`.

The only way to clear `raises` is `try`/`catch` catching `exception` (ID 1),
which is the root of the hierarchy and catches everything. Catching a specific
subtype like `error` does NOT clear `raises` because the throw could be a
`warning` or any other type.

This is genuinely useful: it tells you which functions are **guaranteed** to
never throw. The set of non-raising functions is exactly the set where every
code path avoids `throw` and calls only non-raising functions.

### 4.4 Propagation during fixpoint iteration

Raises effects propagate exactly like yield effects during the cross-form
fixpoint iteration in `compile_all`. Self-recursive functions start
with `may_raise = false` (optimistic) and iterate until stable.

### 4.5 Runtime query

`(raises? value)` reads `closure.effect.may_raise()` (checks `SIG_ERROR`
in the effect's signal bits). Returns `true` if the closure may raise, `false`
otherwise.

When we add specific exception type tracking (§4.6), the return type will
change to a list of exception type keywords (`:error`, `:type-error`,
`:division-by-zero`, etc.) for closures that may raise, and `false` for those
that don't.

### 4.6 Future: specific exception types

Once the boolean tracking is proven correct, we can extend to
`BTreeSet<u32>` tracking specific exception IDs. This would enable:
- `try`/`catch` catching `error` to subtract error and its children
- `raises?` returning a list of specific exception type keywords
- Primitive annotations (e.g., `/` raises `:division-by-zero`)

This is additive — the boolean version is a proper subset of the set version.

## 5. Primitive effect registration

> **Status: Partially implemented.** `register_fn` already takes an `Effect` parameter and every primitive declares its effect at registration time. However, `effects/primitives.rs` still exists as a parallel side-table — it has not been deleted yet. Both systems coexist: `registration.rs` returns an effects map, and `effects/primitives.rs` provides `get_primitive_effects` for the analyzer.

### 5.1 Registration signature

`register_fn` takes an `Effect` parameter (already implemented):

```rust
fn register_fn(
    vm: &mut VM,
    symbols: &mut SymbolTable,
    effects: &mut HashMap<SymbolId, Effect>,
    name: &str,
    func: fn(&[Value]) -> (SignalBits, Value),
    effect: Effect,
)
```

Every primitive declares its full effect at registration time.

### 5.2 Effect values for primitives

Most primitives use `Effect::raises()` (may raise on arity/type errors).
Type predicates and constants use `Effect::none()`. The naming convention
uses `none()`/`raises()` rather than the deprecated `pure()`/`pure_raises()`
aliases:

```rust
// The common case: may raise
register_fn(vm, symbols, &mut effects, "first", prim_first, Effect::raises());

// Type predicates: no effects
register_fn(vm, symbols, &mut effects, "nil?", prim_is_nil, Effect::none());

// Division: may raise (division by zero)
register_fn(vm, symbols, &mut effects, "/", prim_div_vm, Effect::raises());
```

Constructors on `Effect`:
- `Effect::none()` — no effects (preferred over deprecated `Effect::pure()`)
- `Effect::raises()` — may raise (preferred over deprecated `Effect::pure_raises()`)
- `Effect::yields()` — may yield, does not raise
- `Effect::yields_raises()` — may yield, may raise
- `Effect::polymorphic(n)` — effect depends on param n, does not raise
- `Effect::polymorphic_raises(n)` — effect depends on param n, may raise

### 5.3 Remaining migration

`effects/primitives.rs` still exists and provides `get_primitive_effects`.
It should be deleted once the analyzer is updated to use the effects map
returned by `register_primitives` instead.

### 5.4 Effect annotations for debugging primitives

| Primitive | Effect | Status |
|-----------|--------|--------|
| `clock/monotonic`, `clock/realtime`, `clock/cpu` | `Effect::none()` | Registered |
| `time/sleep` | `Effect::raises()` | Registered |
| `closure?`, `jit?`, `pure?`, `coro?`, `mutates-params?` | `Effect::none()` | Registered |
| `arity`, `captures`, `bytecode-size` | `Effect::none()` | Registered |
| `raises?` | `Effect::none()` | Registered |
| `jit`, `jit!` | `Effect::raises()` | Not yet implemented |
| `call-count` | `Effect::none()` | Not yet implemented |
| `global?` | `Effect::none()` | Not yet implemented |

## 6. Testing strategy

> **Status: Partially implemented.** Property tests for time primitives exist. Integration tests and benchmark tests are not yet created.

### 6.1 Unit tests for primitives

One test per primitive in `tests/unittests/primitives.rs`, following the
existing pattern (call with good args, call with bad args, verify errors).

### 6.2 Integration tests

Not yet created. Planned file: `tests/integration/debugging.rs`
- Test each introspection primitive on known closures
- Test clock primitives return valid floats
- Test `jit` trigger on a simple pure function
- Test `raises?` on functions with known throw patterns

### 6.3 Property tests

Implemented in `tests/integration/time_property.rs`:
- **Clock monotonicity**: successive `(clock/monotonic)` calls are non-decreasing
- **Clock realtime plausibility**: `(clock/realtime)` returns a value in a
  plausible Unix epoch range
- **Stopwatch monotonicity**: successive stopwatch samples are non-decreasing
- **Elapsed non-negativity**: `time/elapsed` always returns non-negative timing
- **Monotonic-realtime consistency**: both clocks advance together

Not yet implemented:
- **Raises monotonicity**: adding a `throw` to a function body can only
  change `may_raise` from `false` to `true`, never the reverse

### 6.4 Example files

`examples/time.lisp` exists. `examples/debugging.lisp` exists.
`examples/debugging-profiling.lisp` still exists (not yet replaced).

### 6.5 Benchmark tests

Not yet created. Blocked on defmacro gensym expansion fix (§3).

## 7. File plan

| File | Status | Content |
|------|--------|---------|
| `src/primitives/debugging.rs` | **Done** | Introspection primitives: `jit?`, `pure?`, `coro?`, `closure?`, `mutates-params?`, `arity`, `captures`, `bytecode-size`, `raises?`, `disbit`, `disjit`. |
| `src/primitives/debug.rs` | **Done** | `debug-print`, `trace`, `memory-usage`. `profile` removed. |
| `src/primitives/time.rs` | **Done** | Clock primitives (`clock/monotonic`, `clock/realtime`, `clock/cpu`) and `time/sleep`. |
| `src/primitives/time_def.rs` | **Done** | Elle definitions for `time/stopwatch` and `time/elapsed`. |
| `src/primitives/registration.rs` | **Done** | `Effect` parameter on `register_fn`# all primitives registered with effects. |
| `src/effects/mod.rs` | **Done** | `Effect` restructured as `{ bits: SignalBits, propagates: u32 }` (not the `YieldBehavior` + `may_raise` design originally proposed). |
| `src/effects/primitives.rs` | **Still exists** | Should be deleted once analyzer uses the registration-time effects map. |
| `lib/bench.lisp` | **Not created** | Blocked on defmacro gensym expansion fix. |
| `examples/time.lisp` | **Done** | Time/clock primitive examples. |
| `examples/debugging.lisp` | **Done** | Introspection examples. |
| `examples/debugging-profiling.lisp` | **Still exists** | Not yet replaced/removed. |
| `tests/integration/time_property.rs` | **Done** | Property tests for time primitives. |
| `tests/integration/debugging.rs` | **Not created** | Integration tests for introspection primitives. |
| `tests/integration/benchmarks.rs` | **Not created** | Blocked on defmacro gensym expansion fix. |

`debug.rs` keeps the low-level printf-style tools (`debug-print`, `trace`,
`memory-usage`). `debugging.rs` has the introspection primitives.

Note: `src/primitives/introspection.rs` already exists and contains
exception introspection (`exception-id`, `condition-field`, etc.). That
file is unrelated and unchanged.

## 8. Non-goals

- **Profiler**: A sampling profiler (interrupts, stack walking) is out of
  scope. The `bench` macro and time primitives provide deterministic
  measurement.

- **Step debugger**: GDB/LLDB integration is out of scope. Use `trace` and
  `debug-print` for printf-style debugging.

- **Coverage**: Code coverage instrumentation is out of scope.

- **Hot-reload**: Reloading changed source without restart is out of scope.

## 9. Deferred

These are useful but have no current consumer. Add them when needed:

- **`time/unix->parts`** / **`time/parts->unix`**: Calendar decomposition
  (year, month, day, etc.). Requires `chrono` or hand-rolled calendar math.
- **Bench macros**: `bench`, `bench-n`, `bench-compare` in `lib/bench.lisp`.
  Blocked on defmacro gensym expansion fix (§3).
- **`global?`**: Requires VM access via SIG_RESUME.
- **`jit`** / **`jit!`**: JIT trigger primitives.
- **`call-count`**: Requires VM hotness tracker integration.
