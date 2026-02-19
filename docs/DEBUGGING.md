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
| `mutable?` | `(mutable? value)` | `#t` or `#f` | True if value is a closure that captures any mutable bindings |
| `closure?` | `(closure? value)` | `#t` or `#f` | True if value is a closure (bytecode, not native/vm-aware) |

Implementation: each is a simple predicate that examines the `Value` and,
for closures, reads fields on the `Closure` struct.

- `jit?` checks `closure.jit_code.is_some()`
- `pure?` checks `closure.effect == Effect::Pure`
- `coro?` checks `closure.effect == Effect::Yields`
- `mutable?` checks `closure.cell_params_mask != 0` (any cell-wrapped params)
- `closure?` checks `value.as_closure().is_some()`
- `global?` takes a symbol, checks `vm.get_global(sym_id).is_some()`

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
| `raises?` | `(raises? value)` | `nil` or vector of keywords | Returns nil if the closure is known to never raise. Returns a vector of exception type keywords if it may raise. |

Exception types are returned as keywords: `:error`, `:type-error`,
`:division-by-zero`, `:arity-error`, `:undefined-variable`, `:warning`,
`:style-warning`, `:condition`, `:unknown`.

This requires the `Raises` effect extension (§4).

### 1.4 Additional introspection

| Primitive | Signature | Returns | Notes |
|-----------|-----------|---------|-------|
| `arity` | `(arity value)` | int, pair, or nil | For closures: exact arity as int, or `(min . max)` pair for range, or `(min . nil)` for variadic. Nil for non-closures. |
| `captures` | `(captures value)` | int or nil | Number of captured variables, or nil for non-closures. |
| `call-count` | `(call-count value)` | int | Number of times this closure has been called (from VM's hotness tracker). VmAwareFn. |
| `bytecode-size` | `(bytecode-size value)` | int or nil | Size of closure's bytecode in bytes. Nil for non-closures. |

## 2. Clock API

POSIX `clock_gettime` family, exposed as Elle primitives. All return
`(seconds . nanoseconds)` pairs (cons cells). All are `NativeFn` — they
call libc directly, no VM access needed.

### 2.1 Reading clocks

| Primitive | Signature | Returns | Notes |
|-----------|-----------|---------|-------|
| `clock-realtime` | `(clock-realtime)` | `(sec . nsec)` | `CLOCK_REALTIME` — wall clock since epoch |
| `clock-monotonic` | `(clock-monotonic)` | `(sec . nsec)` | `CLOCK_MONOTONIC` — monotonic since boot |
| `clock-process` | `(clock-process)` | `(sec . nsec)` | `CLOCK_PROCESS_CPUTIME_ID` — CPU time for this process |
| `clock-thread` | `(clock-thread)` | `(sec . nsec)` | `CLOCK_THREAD_CPUTIME_ID` — CPU time for this thread |

### 2.2 Clock resolution

| Primitive | Signature | Returns | Notes |
|-----------|-----------|---------|-------|
| `clock-resolution` | `(clock-resolution clock-keyword)` | `(sec . nsec)` | `clock_getres`. Argument is `:realtime`, `:monotonic`, `:process`, or `:thread`. |

### 2.3 Sleeping

| Primitive | Signature | Returns | Notes |
|-----------|-----------|---------|-------|
| `clock-nanosleep` | `(clock-nanosleep sec nsec)` | `(sec . nsec)` or `#t` | `clock_nanosleep` on `CLOCK_MONOTONIC` with relative time. Returns remaining time if interrupted, `#t` if completed. |

### 2.4 Timespec arithmetic

| Primitive | Signature | Returns | Notes |
|-----------|-----------|---------|-------|
| `timespec-diff` | `(timespec-diff a b)` | `(sec . nsec)` | Returns `a - b`, handling borrow. |
| `timespec->float` | `(timespec->float ts)` | float | Converts `(sec . nsec)` to float seconds. Precision loss for large values. |
| `timespec->ns` | `(timespec->ns ts)` | int | Converts to nanoseconds. Panics if result exceeds 48-bit range (~39 hours). |

### 2.5 Implementation

Use Rust's `libc` crate for direct syscalls. All clock primitives:
1. Call `libc::clock_gettime(clock_id, &mut ts)`
2. Return `Value::cons(Value::int(ts.tv_sec as i64), Value::int(ts.tv_nsec as i64))`

The `libc` crate is already a transitive dependency (via cranelift). Add it
as a direct dependency if not already present.

## 3. Benchmarking Macro

A `defmacro`-based benchmarking facility, written in Elle, that wraps the
clock primitives. Lives in `lib/bench.lisp` (loaded via `import-file`).

### 3.1 Core macro: `bench`

```lisp
(defmacro bench (label expr)
  (let ((start (gensym))
        (result (gensym))
        (end (gensym))
        (elapsed (gensym)))
    `(let ((,start (clock-monotonic))
           (,result ,expr)
           (,end (clock-monotonic))
           (,elapsed (timespec-diff ,end ,start)))
       (display ,label)
       (display ": ")
       (display (timespec->float ,elapsed))
       (display "s\n")
       ,result)))
```

Usage: `(bench "fibonacci(30)" (fibonacci 30))` prints timing, returns result.

### 3.2 Iteration macro: `bench-n`

```lisp
(defmacro bench-n (label n expr)
  (let ((i (gensym))
        (start (gensym))
        (end (gensym))
        (elapsed (gensym))
        (result (gensym)))
    `(let ((,start (clock-monotonic))
           (,result nil))
       (define ,i 0)
       (while (< ,i ,n)
         (set! ,result ,expr)
         (set! ,i (+ ,i 1)))
       (let ((,end (clock-monotonic))
             (,elapsed (timespec-diff ,end ,start)))
         (display ,label)
         (display " (")
         (display ,n)
         (display " iterations): ")
         (display (timespec->float ,elapsed))
         (display "s (")
         (display (timespec->float (timespec-diff ,elapsed (cons 0 0))))
         (display "s/iter)\n")
         ,result))))
```

### 3.3 Comparison macro: `bench-compare`

```lisp
;; (bench-compare "label" n expr1 expr2) — runs both n times, prints comparison
```

### 3.4 Assertion macro: `assert-faster`

```lisp
;; (assert-faster "label" n fast-expr slow-expr) — fails if fast is not faster
```

These live in `lib/bench.lisp` and are imported by test scripts.

## 4. Effect System Extension: Raises

### 4.1 Design

Add a `raises` field tracking which exception types an expression may raise.
This is orthogonal to the yield/pure axis: a function can be `Pure` (doesn't
yield) but still raise exceptions.

**Representation**: `bool` on `Effect` (or alongside it). `true` means "may
raise an exception." `false` means "guaranteed not to raise."

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
| primitive call | `false` for now (future: annotate which primitives may raise) |

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
may raise, `#f` otherwise. (The original spec said "nil or vector of
keywords" — with the simplified boolean design, it returns a boolean.
When we add specific type tracking later, it will return the vector.)

### 4.6 Future: specific exception types

Once the boolean tracking is proven correct, we can extend to
`BTreeSet<u32>` tracking specific exception IDs. This would enable:
- `handler-case` catching `error` to subtract error and its children
- `raises?` returning a vector of specific exception type keywords
- Primitive annotations (e.g., `/` raises `:division-by-zero`)

This is additive — the boolean version is a proper subset of the set version.

## 5. Effect annotations for new primitives

All introspection primitives (§1) and clock primitives (§2) are `Pure`
in the yield sense (they never yield). Add them to the pure list in
`effects/primitives.rs`.

For the raises axis:
- Clock primitives: `may_raise = false`
- Introspection predicates: `may_raise = false`
- `jit`, `jit!`: `may_raise = true` (may error on failure)
- `call-count`: `may_raise = false`

## 6. Testing strategy

### 6.1 Unit tests for primitives

One test per primitive in `tests/unittests/primitives.rs`, following the
existing pattern (call with good args, call with bad args, verify errors).

### 6.2 Integration tests

New file: `tests/integration/debugging.rs`
- Test each introspection primitive on known closures
- Test clock primitives return valid timespecs
- Test `jit` trigger on a simple pure function
- Test `raises?` on functions with known throw patterns

### 6.3 Example file

New file: `examples/debugging.lisp` (replace the current
`debugging-profiling.lisp` which is mostly placeholder patterns).

### 6.4 Benchmark tests

New file: `tests/integration/benchmarks.rs`
- Test `bench` macro produces correct results
- Test `bench-n` with known iteration counts
- Test `timespec-diff` arithmetic
- Test clock monotonicity (second reading ≥ first reading)

## 7. File plan

| File | Action | Content |
|------|--------|---------|
| `src/primitives/introspect.rs` | New | `jit?`, `pure?`, `coro?`, `closure?`, `mutable?`, `arity`, `captures`, `bytecode-size` |
| `src/primitives/clock.rs` | New | Clock API (§2) |
| `src/primitives/jit_ops.rs` | New | `jit`, `jit!`, `call-count`. VmAwareFn. |
| `src/primitives/debug.rs` | Modify | Remove placeholder `profile`. Keep `debug-print`, `trace`, `memory-usage`. |
| `src/primitives/registration.rs` | Modify | Register new primitives |
| `src/primitives/mod.rs` | Modify | Add new modules |
| `src/effects/primitives.rs` | Modify | Add effect annotations for new primitives |
| `lib/bench.lisp` | New | Benchmarking macros |
| `examples/debugging.lisp` | New (replaces `debugging-profiling.lisp`) | Demonstrates introspection and benchmarking |
| `tests/integration/debugging.rs` | New | Integration tests |
| `tests/integration/benchmarks.rs` | New | Benchmark macro tests |

## 8. Non-goals

- **Profiler**: A sampling profiler (interrupts, stack walking) is out of
  scope. The `bench` macro and `clock-*` primitives provide deterministic
  measurement.

- **Step debugger**: GDB/LLDB integration is out of scope. Use `trace` and
  `debug-print` for printf-style debugging.

- **Coverage**: Code coverage instrumentation is out of scope.

- **Hot-reload**: Reloading changed source without restart is out of scope.
