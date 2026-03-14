# signals

Signal system for tracking which signals a function may emit. Includes the global signal registry for mapping signal keywords to bit positions.

## Responsibility

1. Define the `Signal` type and provide signal inference for the emit/fiber system
2. Maintain the global signal registry mapping signal keywords to bit positions
3. Track which signals a function might emit (error, yield, debug, ffi, io, halt, user-defined)
4. Track which parameter indices propagate their callee's signals
5. Support signal bounds on functions and parameters via `silence` declarations

## Interface

| Type/Function | Purpose |
|---------------|---------|
| `Signal` | `{ bits: SignalBits, propagates: u32 }` — Copy, const fn constructors |
| `Signal::silent()` | No signals |
| `Signal::errors()` | May error (SIG_ERROR) |
| `Signal::yields()` | May yield (SIG_YIELD) |
| `Signal::yields_errors()` | May yield and error |
| `Signal::ffi()` | Calls foreign code (SIG_FFI) |
| `Signal::ffi_errors()` | FFI + may error |
| `Signal::halts()` | May halt (SIG_HALT) |
| `Signal::polymorphic(n)` | Signal depends on parameter n |
| `Signal::polymorphic_errors(n)` | Polymorphic + may error |


## Predicates

Each predicate asks a specific question. No vague "is_inert".

| Predicate | Meaning |
|-----------|---------|
| `may_suspend()` | Can suspend execution? (yield, debug, or polymorphic) |
| `may_yield()` | Can yield? (SIG_YIELD) |
| `may_error()` | Can signal an error? (SIG_ERROR) |
| `may_ffi()` | Calls foreign code? (SIG_FFI) |
| `may_halt()` | Can halt? (SIG_HALT) |
| `is_polymorphic()` | Signal depends on arguments? (propagates != 0) |
| `propagated_params()` | Iterator over propagated parameter indices |

## Constants

| Constant | Value |
|----------|-------|
| `Signal::SILENT` | `Signal::silent()` |
| `Signal::YIELDS` | `Signal::yields()` |

## Signal Registry

The global signal registry maps signal keywords to bit positions. It is a process-global singleton initialized with built-in signals and extended with user-defined signals via `(signal :keyword)` forms.

### Built-in Signals

| Keyword | Bit | Meaning |
|---------|-----|---------|
| `:error` | 0 | Error signal |
| `:yield` | 1 | Cooperative suspension |
| `:debug` | 2 | Breakpoint/trace |
| `:ffi` | 4 | Calls foreign code |
| `:halt` | 8 | Graceful VM termination |
| `:io` | 9 | I/O request to scheduler |

Bits 3, 5, 6, 7, 10–15 are reserved for VM-internal use.

### User-Defined Signals

User signals are allocated bits 16–31 (up to 16 user signals per compilation unit). The registry is append-only — once a keyword is registered, its bit position is fixed for the lifetime of the process.

### Registry Interface

- `global_registry()` — Access the process-global registry
- `register(&mut self, name: &str) -> Result<u32, String>` — Register a new signal, returns bit position
- `lookup(&self, name: &str) -> Option<u32>` — Look up bit position for a keyword
- `to_signal_bits(&self, name: &str) -> Option<SignalBits>` — Convenience: keyword → SignalBits
- `format_signal_bits(&self, bits: SignalBits) -> String` — Human-readable representation for error messages

## Inferred Signals

Every lambda has a signal-related field:

1. **`inferred_signals: Signal`** (always present, never Optional) — The minimum guaranteed set of signals the lambda may produce, accumulated from:
   - Direct signal emissions in the body
   - Signals of internal calls to statically-known functions
   - Signals contributed by bounded parameters (their bound's bits are included)
   - Unbounded callable parameters contribute conservatively (Yields)

The programmer-supplied ceiling constraint from `(silence)` or `(silence :kw ...)` is a separate concept — the `silence` form provides a bound that the compiler checks `inferred_signals` against. When a `silence` bound is present, the compiler checks `inferred_signals.bits ⊆ bound.bits`. If the check passes, the lambda's final signal is the declared bound (tighter). If it fails, compile-time error.

### Parameter Bounds

Parameter bounds are stored as `param_bounds: Vec<(Binding, Signal)>` on the Lambda node. When a parameter has a bound, it is no longer polymorphic — its signal contribution to the lambda is the bound's bits, not a polymorphic reference.

## Interprocedural Signal Tracking

The analyzer performs interprocedural signal tracking:

1. **signal_env**: Maps `Binding` → `Signal` for locally-defined functions
2. **primitive_signals**: Maps `SymbolId` → `Signal` for primitive functions
3. **current_param_bounds**: Maps `Binding` → `Signal` for parameters with declared bounds (during lambda analysis)

When analyzing a call:
- Direct fn calls: use the fn body's signal
- Variable calls: look up in `signal_env` (local) or `primitive_signals` (global)
- Polymorphic signals: resolve by examining the argument's signal via `propagated_params()` iterator over the `propagates` bitmask
- Bounded parameters: their signal contribution is the bound's bits, not polymorphic

### Limitations

- Signals are tracked within a single compilation unit
- Cross-unit signal tracking is not implemented
- `assign` invalidates signal tracking for the mutated binding
- Mutual recursion in `letrec` may have incomplete signal information

## I/O Signals

Stream primitives and network primitives include `SIG_IO` in their signal
annotations. This is critical for escape analysis: `may_suspend()` checks
`SIG_YIELD | SIG_DEBUG` bits and `propagates != 0`, but the scheduler also
needs to know that a function may yield an I/O request. Primitives that
return `(SIG_YIELD | SIG_IO, IoRequest)` must declare both bits.

Stream primitives (`stream/read-line`, `stream/read`, `stream/read-all`,
`stream/write`, `stream/flush`) have signal `SIG_ERROR | SIG_YIELD | SIG_IO`.
Network primitives (`tcp/accept`, `tcp/connect`, `tcp/shutdown`, `udp/send-to`,
`udp/recv-from`, `unix/accept`, `unix/connect`, `unix/shutdown`) also include
`SIG_YIELD | SIG_IO`. The async sleep primitive `ev/sleep` has signal
`SIG_ERROR | SIG_YIELD | SIG_IO`.

## Dependents

Used across the pipeline and the runtime:
- `hir/analyze/call.rs` — infers signals during analysis, resolves polymorphic via `propagates` bitmask
- `hir/expr.rs` — `Hir` carries a `Signal`
- `lir/emit.rs` — emits signal metadata on closures
- `value/closure.rs` — `ClosureTemplate` stores its `Signal`
- `pipeline.rs` — builds primitive signals map, passes to Analyzer
- `jit/compiler.rs` — JIT gate checks `!signal.may_suspend()`
- `vm/call.rs` — call dispatch checks `!signal.may_suspend()`
- `primitives/coroutines.rs` — coroutine warnings check `!signal.may_yield()`
- `primitives/stream.rs` — stream primitives use `SIG_ERROR | SIG_YIELD | SIG_IO`
- `io/backend.rs` — backend execution returns `(SIG_OK, result)` or `(SIG_ERROR, error)`

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~350 | `Signal` struct, constructors, predicates, Display, combine, tests |
| `registry.rs` | ~200 | `SignalRegistry` struct, global singleton, built-in registration, user signal allocation |

## Invariants

1. **Signal::silent() is the default.** Unknown signals start as silent. This is
   conservative — we may miss some suspension propagation but never produce
   false positives.

2. **Suspension propagates.** If any sub-expression may suspend, the parent
   may suspend. This includes call sites: calling a suspending function
   propagates suspension.

3. **Polymorphic uses a bitmask.** `propagates` is a u32 bitmask where bit i
   set means parameter i's signals flow through. Higher-order functions like
   `map`, `filter`, `fold` use this. `propagated_params()` iterates the set bits.

4. **assign invalidates tracking.** When a binding is mutated via `assign`, its
   signal becomes uncertain and is removed from `signal_env`.

5. **Signal is Copy.** No allocation, no cloning needed. `const fn` constructors.
