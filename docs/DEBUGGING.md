# Elle Debugging Toolkit

> Design document. February 2026.

## Motivation

Debugging Elle programs currently requires rebuilding the Rust binary to add
instrumentation, then removing it afterward. This cycle is slow, wasteful,
and error-prone. There are no timing primitives, no way to inspect closure
properties from Elle code, and the `profile` primitive is a placeholder.

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
get information about it. All are `NativeFn` unless noted otherwise.

### 1.1 Compiler/runtime predicates

| Primitive | Signature | Returns | Notes |
|-----------|-----------|---------|-------|
| `jit?` | `(jit? value)` | `#t` or `#f` | True if value is a closure with JIT-compiled native code |
| `pure?` | `(pure? value)` | `#t` or `#f` | True if value is a closure with `Effect::Pure` |
| `coro?` | `(coro? value)` | `#t` or `#f` | True if value is a closure with `Effect::Yields` |
| `global?` | `(global? sym)` | `#t` or `#f` | True if symbol is bound as a global. VmAwareFn. |
| `mutates-params?` | `(mutates-params? value)` | `#t` or `#f` | True if value is a closure whose body mutates any of its own parameters (i.e., `cell_params_mask != 0`) |
| `closure?` | `(closure? value)` | `#t` or `#f` | True if value is a closure (bytecode, not native/vm-aware) |

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

| Primitive | Signature | Returns | Notes |
|-----------|-----------|---------|-------|
| `jit` | `(jit value)` | closure | Triggers JIT compilation if value is a closure with `lir_function`. Returns the closure (with `jit_code` populated if compilation succeeded). Does not mutate any global. VmAwareFn. |
| `jit!` | `(jit! sym)` | closure | Takes a symbol, looks up the global, JIT-compiles it, and **replaces the global binding** with the JIT-compiled closure. Returns the new value. VmAwareFn. |

`jit` is the pure-style API: give it a closure, get back a closure that may
now have JIT code. `jit!` is the imperative API: give it a symbol naming a
global, and it mutates the global in place.

Both require the closure to have a `lir_function`. If the closure has no LIR
(e.g., it's a native fn or already lost its LIR), they return the value
unchanged (for `jit`) or signal an error (for `jit!`).

### 1.3 Exception tracking: `raises?`

| Primitive | Signature | Returns | Notes |
|-----------|-----------|---------|-------|
| `raises?` | `(raises? value)` | `#t` or `#f` | Returns `#t` if the closure may raise an exception, `#f` if it is guaranteed not to. Returns `#f` for non-closures. |

This is a boolean query. When we add specific exception type tracking in the
future (§4.6), the return type will change to a vector of exception type
keywords. See §4.5 for details.

This requires the `Raises` effect extension (§4).

### 1.4 Additional introspection

| Primitive | Signature | Returns | Notes |
|-----------|-----------|---------|-------|
| `arity` | `(arity value)` | int, pair, or nil | For closures: exact arity as int, or `(min . max)` pair for range, or `(min . nil)` for variadic. Nil for non-closures. |
| `captures` | `(captures value)` | int or nil | Number of captured variables, or nil for non-closures. |
| `call-count` | `(call-count value)` | int | Number of times this closure has been called (from VM's hotness tracker). VmAwareFn. |
| `bytecode-size` | `(bytecode-size value)` | int or nil | Size of closure's bytecode in bytes. Nil for non-closures. |

## 2. Time API

Rust's `std::time` module models time as two distinct types: `Instant`
(an opaque monotonic timestamp) and `Duration` (an inspectable time span).
You take two instants and subtract them to get a duration. You inspect the
duration to get numbers. Elle's time API mirrors this directly.

### 2.1 Types

**Instant** — an opaque monotonic timestamp. You cannot inspect its
contents. The only useful operation is to take two instants and compute
the duration between them. Backed by `std::time::Instant`.

**Duration** — a time span with nanosecond precision. You can convert it
to seconds (float), to nanoseconds (int), and compare durations. Backed
by `std::time::Duration`.

Both are new heap-allocated value types (`HeapObject::Instant`,
`HeapObject::Duration`), following the same pattern as `LibHandle`,
`CHandle`, and `ThreadHandle`. They print as `#<instant>` and
`#<duration 1.234s>`.

Type predicates `instant?` and `duration?` are provided.

### 2.2 Capturing time

| Primitive | Signature | Returns | Notes |
|-----------|-----------|---------|-------|
| `now` | `(now)` | instant | Captures the current monotonic time. Backed by `Instant::now()`. |
| `cpu-time` | `(cpu-time)` | duration | CPU time consumed by this process. Backed by `cpu_time::ProcessTime`. |

`now` is the workhorse. The typical measurement pattern is:

```lisp
(let ((start (now)))
  (do-expensive-work)
  (elapsed start))
```

Every call to `now` heap-allocates an `Rc<HeapObject::Instant>`. For tight
measurement loops this is fine — the allocation is small and short-lived.

### 2.3 Measuring and converting

| Primitive | Signature | Returns | Notes |
|-----------|-----------|---------|-------|
| `elapsed` | `(elapsed instant)` | duration | Time elapsed since the given instant. Equivalent to `instant.elapsed()` in Rust. This is the primary measurement primitive. |
| `duration` | `(duration sec nsec)` | duration | Constructs a duration from seconds and nanoseconds. Both must be non-negative integers. |
| `duration->seconds` | `(duration->seconds d)` | float | Converts duration to fractional seconds. `Duration::as_secs_f64()`. |
| `duration->nanoseconds` | `(duration->nanoseconds d)` | int | Converts duration to nanoseconds. Errors if result exceeds signed 48-bit integer range (~39 hours). |

### 2.4 Duration comparison

| Primitive | Signature | Returns | Notes |
|-----------|-----------|---------|-------|
| `duration<` | `(duration< a b)` | `#t` or `#f` | True if `a` is shorter than `b`. |

This is the one duration operation needed beyond construction and
conversion. It enables `bench-compare` and `assert-faster` (§3). Further
arithmetic (`duration+`, `duration-`, `duration=`, `duration-zero`) can be
added when a concrete use case demands them.

### 2.5 Sleeping

| Primitive | Signature | Returns | Notes |
|-----------|-----------|---------|-------|
| `sleep` | `(sleep d)` | `#t` | Sleeps for the given duration. Backed by `std::thread::sleep`. Always returns `#t`. |

The existing `sleep` primitive (which takes a number of seconds and returns
`nil`) is replaced by this version. It accepts only a duration value — no
polymorphic int/float overload. To sleep for half a second:

```lisp
(sleep (duration 0 500000000))
```

### 2.6 Type predicates

| Primitive | Signature | Returns | Notes |
|-----------|-----------|---------|-------|
| `instant?` | `(instant? value)` | `#t` or `#f` | True if value is an instant. |
| `duration?` | `(duration? value)` | `#t` or `#f` | True if value is a duration. |

### 2.7 Implementation

All time primitives use Rust-native types from `std::time`:

1. **`now`**: Returns `Value::instant(Instant::now())`. One heap allocation
   (an `Rc<HeapObject::Instant>`).

2. **`elapsed`**: Extracts the `Instant` from the value, computes
   `instant.elapsed()`, returns `Value::duration(d)`.

3. **`cpu-time`**: The `cpu-time` crate
   ([crates.io](https://crates.io/crates/cpu-time)) provides
   `ProcessTime::now()`, returning a `Duration`. It works on Linux, macOS,
   and Windows. It's a small, focused crate (~200 lines, no transitive
   dependencies beyond `libc` internally).

4. **Duration conversions**: `duration->seconds` calls `d.as_secs_f64()`.
   `duration->nanoseconds` calls `d.as_nanos()` and checks the signed
   48-bit range before returning `Value::int()`.

5. **Comparison**: Direct delegation to `Duration::cmp`.

**New heap types**: Add `HeapObject::Instant(std::time::Instant)` and
`HeapObject::Duration(std::time::Duration)` to `src/value/heap.rs`.
Add constructors `Value::instant()`, `Value::duration()` and accessors
`as_instant()`, `as_duration()`, `is_instant()`, `is_duration()`.

**Dependencies**: Add `cpu-time` to `Cargo.toml`. Everything else comes
from `std::time`.

### 2.8 Why not `(sec . nsec)` pairs?

The earlier draft represented all times as `(seconds . nanoseconds)` cons
pairs. The instant/duration design is better in every dimension:

- **Type safety**: A cons pair `(1 . 500)` is indistinguishable from any
  other cons pair. A duration value is a distinct type — passing a list
  where a duration is expected is a type error, caught immediately.

- **Performance**: No cons cell allocation for clock reads. `now` allocates
  one `Rc<HeapObject>` instead of an `Rc<Cons>` containing two boxed ints.
  Duration arithmetic operates on the Rust `Duration` directly instead of
  unpacking/repacking cons cells.

- **Ergonomics**: `(elapsed start)` is cleaner than
  `(timespec-diff (clock-monotonic) start)`. `(duration->seconds d)` is
  cleaner than `(timespec->float ts)`.

- **Fidelity**: The API mirrors Rust's own time model, which is
  well-designed and battle-tested. Elle programmers who know Rust will
  find it familiar. Elle programmers who don't will learn good habits.

## 3. Benchmarking Macro

A `defmacro`-based benchmarking facility, written in Elle, that wraps the
time primitives. Lives in `lib/bench.lisp` (loaded via `import-file`).

### 3.1 Core macro: `bench`

```lisp
(defmacro bench (label expr)
  (let ((start (gensym))
        (result (gensym))
        (dt (gensym)))
    `(let* ((,start (now))
            (,result ,expr)
            (,dt (elapsed ,start)))
       (display ,label)
       (display ": ")
       (display (duration->seconds ,dt))
       (display "s\n")
       ,result)))
```

Usage: `(bench "fibonacci(30)" (fibonacci 30))` prints timing, returns result.

### 3.2 Iteration macro: `bench-n`

```lisp
(defmacro bench-n (label n expr)
  (let ((i (gensym))
        (start (gensym))
        (dt (gensym))
        (result (gensym))
        (count (gensym)))
    `(let* ((,count ,n)
            (,start (now))
            (,result nil))
       (define ,i 0)
       (while (< ,i ,count)
         (set! ,result ,expr)
         (set! ,i (+ ,i 1)))
       (let ((,dt (elapsed ,start)))
         (display ,label)
         (display " (")
         (display ,count)
         (display " iterations): ")
         (display (duration->seconds ,dt))
         (display "s (")
         (display (/ (duration->seconds ,dt) ,count))
         (display "s/iter)\n")
         ,result))))
```

### 3.3 Comparison macro: `bench-compare`

```lisp
;; (bench-compare "label" n expr1 expr2) — runs both n times, prints comparison
;; Uses duration< to determine which is faster.
```

These live in `lib/bench.lisp` and are imported by test scripts.

## 4. Effect System Extension: Raises

### 4.1 Design

Add a `may_raise: bool` field tracking whether an expression may raise an
exception. This is orthogonal to the yield/pure axis: a function can be
`Pure` (doesn't yield) but still raise exceptions.

**Placement**: Add `may_raise: bool` as a field on the `Effect` type. This
requires changing `Effect` from an enum to a struct wrapping the existing
yield-axis enum plus the new boolean:

```rust
pub struct Effect {
    pub yield_behavior: YieldBehavior, // Pure | Yields | Polymorphic
    pub may_raise: bool,
}
```

The existing `Effect::Pure`, `Effect::Yields`, `Effect::Polymorphic`
become `YieldBehavior` variants. All code that currently matches on
`Effect` updates to match on `effect.yield_behavior`. The `combine`
method ORs `may_raise` alongside the existing yield combination logic.

We do NOT attempt to track specific exception types in this iteration. Any
`throw` is conservatively marked as "raises." Specific type tracking (which
would let `handler-case` subtract known types) is a future refinement.

**New field on Closure**: `may_raise: bool`.

### 4.2 Inference rules

| Form | Raises |
|------|--------|
| `(throw expr)` | `true` — always, regardless of argument |
| `(handler-case body (condition e ...))` | `false` — condition is the root type, catches everything |
| `(handler-case body (error e ...))` | body.raises — catching a subtype doesn't guarantee all exceptions are caught |
| `(begin a b)` | a.raises ∨ b.raises |
| `(if c t e)` | c.raises ∨ t.raises ∨ e.raises |
| `(f args...)` | args.raises ∨ f.may_raise |
| literal | `false` |
| primitive call | Uses the primitive's registered `may_raise` flag (see §5) |

### 4.3 Key principle: conservative and correct

Every `throw` is an unknown exception. We don't peek into the argument to
determine the type — `(throw (error "x"))` and `(throw some-variable)` both
produce `raises = true`.

The only way to clear `raises` is `handler-case` catching `condition` (ID 1),
which is the root of the hierarchy and catches everything. Catching a specific
subtype like `error` does NOT clear `raises` because the throw could be a
`warning` or any other type.

This is genuinely useful: it tells you which functions are **guaranteed** to
never throw. The set of non-raising functions is exactly the set where every
code path avoids `throw` and calls only non-raising functions.

### 4.4 Propagation during fixpoint iteration

Raises effects propagate exactly like yield effects during the cross-form
fixpoint iteration in `compile_all_new`. Self-recursive functions start
with `may_raise = false` (optimistic) and iterate until stable.

### 4.5 Runtime query

`(raises? value)` reads `closure.may_raise`. Returns `#t` if the closure
may raise, `#f` otherwise.

When we add specific exception type tracking (§4.6), the return type will
change to a vector of exception type keywords (`:error`, `:type-error`,
`:division-by-zero`, etc.) for closures that may raise, and `#f` for those
that don't.

### 4.6 Future: specific exception types

Once the boolean tracking is proven correct, we can extend to
`BTreeSet<u32>` tracking specific exception IDs. This would enable:
- `handler-case` catching `error` to subtract error and its children
- `raises?` returning a vector of specific exception type keywords
- Primitive annotations (e.g., `/` raises `:division-by-zero`)

This is additive — the boolean version is a proper subset of the set version.

## 5. Primitive effect registration

Currently, primitive effects live in a separate side-table
(`effects/primitives.rs`) that maps primitive names to `Effect` values.
This is duplicated, fragile, and only tracks the yield axis. With the
addition of `may_raise`, we unify effect declaration into primitive
registration itself.

### 5.1 New registration signature

`register_fn` and `register_vm_aware_fn` gain an `Effect` parameter:

```rust
fn register_fn(
    vm: &mut VM,
    symbols: &mut SymbolTable,
    name: &str,
    func: fn(&[Value]) -> Result<Value, Condition>,
    effect: Effect,
)
```

Every primitive declares its full effect at registration time. The
compiler enforces this — you cannot register a primitive without
specifying its effects. This eliminates the separate side-table and
makes it impossible to forget an annotation.

### 5.2 Effect values for primitives

Most primitives are pure (no yield) and may raise (arity/type errors):

```rust
// The common case: pure, may raise
register_fn(vm, symbols, "first", prim_first, Effect::pure_raises());

// Type predicates: pure, never raise
register_fn(vm, symbols, "nil?", prim_is_nil, Effect::pure());

// Higher-order: polymorphic yield, may raise
register_fn(vm, symbols, "map", prim_map, Effect::polymorphic_raises(0));

// Division: VM-aware, may raise (division by zero)
register_vm_aware_fn(vm, symbols, "/", prim_div_vm, Effect::pure_raises());
```

Convenience constructors on `Effect`:
- `Effect::pure()` — does not yield, does not raise
- `Effect::pure_raises()` — does not yield, may raise
- `Effect::yields()` — may yield, does not raise
- `Effect::yields_raises()` — may yield, may raise
- `Effect::polymorphic(n)` — yield depends on param n, does not raise
- `Effect::polymorphic_raises(n)` — yield depends on param n, may raise

### 5.3 Migration

This is a cross-cutting change: every `register_fn` and
`register_vm_aware_fn` call in `registration.rs` gains an `Effect`
argument. The default should be `Effect::pure_raises()` (conservative:
most primitives can raise on bad input). Then audit each primitive and
tighten to `Effect::pure()` where appropriate (type predicates, boolean
ops, constants).

`effects/primitives.rs` is deleted. Its two functions
(`register_primitive_effects`, `get_primitive_effects`) are replaced by
the registration-time declarations. The analyzer reads effects from the
same map the VM uses, populated during registration.

### 5.4 Effect annotations for debugging primitives

| Primitive | Effect |
|-----------|--------|
| `now`, `elapsed`, `cpu-time` | `Effect::pure()` |
| `duration`, `duration->seconds`, `duration->nanoseconds` | `Effect::pure_raises()` |
| `duration<`, `instant?`, `duration?` | `Effect::pure()` |
| `closure?`, `jit?`, `pure?`, `coro?`, `mutates-params?` | `Effect::pure()` |
| `arity`, `captures`, `bytecode-size` | `Effect::pure()` |
| `raises?` | `Effect::pure()` |
| `jit`, `jit!` | `Effect::pure_raises()` |
| `call-count` | `Effect::pure()` |
| `global?` | `Effect::pure()` |

## 6. Testing strategy

### 6.1 Unit tests for primitives

One test per primitive in `tests/unittests/primitives.rs`, following the
existing pattern (call with good args, call with bad args, verify errors).

### 6.2 Integration tests

New file: `tests/integration/debugging.rs`
- Test each introspection primitive on known closures
- Test time primitives return valid instants/durations
- Test `jit` trigger on a simple pure function
- Test `raises?` on functions with known throw patterns

### 6.3 Property tests

New properties in `tests/property/`:
- **Duration conversion roundtrip**: for any non-negative `(sec, nsec)` pair
  within range, `duration->seconds(duration(sec, nsec))` ≈ `sec + nsec/1e9`
- **Monotonicity**: two successive `(now)` calls always produce instants
  where the second `elapsed` is ≥ the first
- **Raises monotonicity**: adding a `throw` to a function body can only
  change `may_raise` from `false` to `true`, never the reverse

### 6.4 Example file

New file: `examples/debugging.lisp` (replace the current
`debugging-profiling.lisp` which is mostly placeholder patterns).

### 6.5 Benchmark tests

New file: `tests/integration/benchmarks.rs`
- Test `bench` macro produces correct results
- Test `bench-n` with known iteration counts
- Test monotonicity (second `now` reading always later than first)

## 7. File plan

| File | Action | Content |
|------|--------|---------|
| `src/value/heap.rs` | Modify | Add `HeapObject::Instant`, `HeapObject::Duration` variants; update `HeapTag`, `tag()`, `type_name()`, `Debug` |
| `src/value/repr/constructors.rs` | Modify | Add `Value::instant()`, `Value::duration()` |
| `src/value/repr/accessors.rs` | Modify | Add `as_instant()`, `as_duration()`, `is_instant()`, `is_duration()` |
| `src/value/repr/traits.rs` | Modify | Add `PartialEq` arms for Instant and Duration |
| `src/value/display.rs` | Modify | Add display formatting for `#<instant>` and `#<duration ...>` |
| `src/value/send.rs` | Modify | Add `SendValue` handling (Instant is Send; Duration is Send) |
| `src/primitives/debugging.rs` | New | All debugging toolkit primitives: introspection (`jit?`, `pure?`, `coro?`, `closure?`, `mutates-params?`, `arity`, `captures`, `bytecode-size`, `raises?`), time (`now`, `elapsed`, `cpu-time`, `duration`, `duration->seconds`, `duration->nanoseconds`, `duration<`, `instant?`, `duration?`), JIT control (`jit`, `jit!`, `call-count`). NativeFn and VmAwareFn. |
| `src/primitives/debug.rs` | Modify | Remove placeholder `profile`. Keep `debug-print`, `trace`, `memory-usage`. |
| `src/primitives/concurrency.rs` | Modify | Update `sleep` to accept duration values only |
| `src/primitives/registration.rs` | Modify | Add `Effect` parameter to `register_fn`/`register_vm_aware_fn`; register all new primitives with effects; migrate all existing registrations |
| `src/primitives/mod.rs` | Modify | Add `debugging` module |
| `src/effects/mod.rs` | Modify | Restructure `Effect` as struct with `YieldBehavior` + `may_raise` |
| `src/effects/primitives.rs` | Delete | Side-table replaced by registration-time effect declarations |
| `lib/bench.lisp` | New | Benchmarking macros |
| `examples/debugging.lisp` | New (replaces `debugging-profiling.lisp`) | Demonstrates introspection and benchmarking |
| `tests/integration/debugging.rs` | New | Integration tests |
| `tests/integration/benchmarks.rs` | New | Benchmark macro tests |

`debug.rs` keeps the low-level printf-style tools (`debug-print`, `trace`,
`memory-usage`). `debugging.rs` gets all the new toolkit primitives — a
single file for introspection, time, and JIT control. This keeps the
debugging toolkit cohesive and avoids scattering related primitives across
three small files.

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

- **`epoch`**: Wall-clock time since Unix epoch as a duration. Backed by
  `SystemTime::now().duration_since(UNIX_EPOCH)`.
- **`thread-time`**: CPU time for the current thread. Backed by
  `cpu_time::ThreadTime`.
- **`duration+`**, **`duration-`**, **`duration=`**, **`duration-zero`**:
  Duration arithmetic beyond comparison.
