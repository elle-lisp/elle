# signals

<!-- audited: 2026-10-04 -->

Signal system for tracking which signals a function may emit. Includes the global signal registry for mapping signal keywords to bit positions.

## Responsibility

1. Define the `Signal` type and provide signal inference for the emit/fiber system.
   `emit` is a special form when the first argument is a literal keyword or keyword set.
   `yield` is a prelude macro (prelude.lisp) that expands to `(emit :yield val)`.
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
| `squelched_bits(bits, mask)` | The bits a boundary carrying `mask` converts into a violation when `bits` leave it; every enforcement site on every tier asks it |
| `bound::violation(value, allowed)` | The message a `(silence p)` bound raises for `value`, judged by a closure's effective signal or a native's declared one |


## Predicates

Each predicate asks a specific question. No vague "is_inert".

| Predicate | Meaning |
|-----------|---------|
| `may_suspend()` | Can suspend execution? (any bit, or polymorphic) |
| `may_park()` | Can park a frame? (any bit outside `:error`/`:halt`/`:ffi`, or polymorphic) — the static face of `dispatch::is_suspending` |
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
| `:exec` | 11 | Subprocess execution (spawn, wait, kill) |
| `:fuel` | 12 | Instruction budget exhaustion |
| `:wait` | 14 | Blocking wait |
| `:gpu` | 15 | GPU hardware dispatch |
| `:os-signal` | 16 | POSIX signal send/raise |
| `:fs` | 17 | Filesystem access |

Bits 3, 5, 6, 7, 10, and 13 are VM-internal and are not registered.

`:exec`, `:gpu`, `:os-signal`, and `:fs` do no dispatch work. They exist so
a fiber mask can withhold the authority, which is what `VM::call_inner`
tests against `def.signal.bits`. Every bit is deniable; only some also
route the call somewhere.

### User-Defined Signals

User signals are allocated bits 32–63 (up to 32 user signals per process). Bits 18–31 are reserved for future runtime signals. The registry is append-only — once a keyword is registered, its bit position is fixed for the lifetime of the process.

### Registry Interface

- `global_registry()` — Access the process-global registry; `with_registry(f)` runs `f` under its lock
- `register(&mut self, name: &str) -> Result<u32, String>` — Register a new signal, returns bit position
- `register_or_get(&mut self, name: &str) -> Result<u32, String>` — The bit of a user signal, registering it on first sight; a builtin name is an error
- `lookup(&self, name: &str) -> Option<u32>` — Look up bit position for a keyword
- `to_signal_bits(&self, name: &str) -> Option<SignalBits>` — Convenience: keyword → SignalBits
- `format_signal_bits(&self, bits: SignalBits) -> String` — Human-readable representation for error messages; the free function `format_bits` does the same under the global lock

## Inferred Signals

Every lambda has a signal-related field:

1. **`inferred_signals: Signal`** (always present, never Optional) — The minimum guaranteed set of signals the lambda may produce, accumulated from:
    - Direct signal emissions in the body
    - Signals of internal calls to statically-known functions
    - Signals contributed by silence-bounded parameters (their bound's bits are included)
    - An unbounded parameter that is called contributes its position to `propagates`, not a bit
    - `:error` from every construct that checks something at run time ([docs/signals/inference.md](../../docs/signals/inference.md), "What raises")

The programmer-supplied ceiling constraint from `(silence)` is a separate concept — the `silence` form provides a total-silence bound that the compiler checks `inferred_signals` against. When a `silence` bound is present, the compiler checks `inferred_signals.bits == 0`. If the check fails, compile-time error. Signal keywords are not accepted by `silence`.

### Parameter Bounds

Parameter bounds are stored as `param_bounds: Vec<ParamBound>` on the Lambda node, where `ParamBound = { binding, signal }`.

- **Silence bounds:** When a parameter has a `silence` bound, it is no longer polymorphic — its signal contribution to the lambda is the bound's bits, not a polymorphic reference. The entry check may raise, so the lambda carries `:error`.

## Interprocedural Signal Tracking

The analyzer performs interprocedural signal tracking:

1. **signal_env**: Maps `Binding` → `Signal` for locally-defined functions
2. **primitive_signals**: Maps `SymbolId` → `Signal` for primitive functions
3. **current_param_bounds**: Maps `Binding` → `Signal` for parameters with declared bounds (during lambda analysis)
4. **current_declared_ceiling**: `Option<Signal>`, the function-level ceiling `(silence)` or `(attune! …)` declared (during lambda analysis)

When analyzing a call:
- Direct fn calls: use the fn body's signal
- Variable calls: look up in `signal_env` (local) or `primitive_signals` (global)
- Polymorphic signals: resolve by examining the argument's signal via `propagated_params()` iterator over the `propagates` bitmask
- Silence-bounded parameters: their signal contribution is the bound's bits, not polymorphic

### Limitations

- Signals are tracked within a single compilation unit
- Cross-unit signal tracking is not implemented
- `assign` invalidates signal tracking for the mutated binding
- Mutual recursion in `letrec` may have incomplete signal information

## I/O Signals

Every primitive that reaches the scheduler declares
`Signal::io_yields_errors()`, which is `SIG_IO | SIG_ERROR`: the stream
primitives (`port/read-line`, `port/read`, `port/read-all`, `port/write`,
`port/flush`), the network primitives (`tcp/accept`, `tcp/connect-ip`,
`tcp/shutdown`, `udp/send-to`, `udp/recv-from`, `unix/accept`,
`unix/connect`, `unix/shutdown`) and `ev/sleep`. `SIG_YIELD` is absent on
purpose: the request suspends its fiber because every signal does, and a
request carrying `:yield` would be caught by every generator mask on its way
to the scheduler. The `IO_ROUND_TRIP` constant in [mod.rs](mod.rs) is the one
definition.

## Dependents

Used across the pipeline and the runtime:
- `hir/analyze/call.rs` — infers signals during analysis, resolves polymorphic via `propagates` bitmask
- `hir/expr.rs` — `Hir` carries a `Signal`
- `lir/emit/` — emits signal metadata on closures
- `value/closure.rs` — a code object's payload stores its `Signal`
- `pipeline/` — builds primitive signals map, passes to Analyzer
- `jit/` — compiles a function whatever its signal; its call helpers ask `squelched_bits` at every boundary
- `vm/call/` — a closure's boundary mask at every call: its squelch, its muffle, and every signal when it is silent
- `primitives/stream.rs`, `primitives/net.rs` — the scheduler round trip, `Signal::io_yields_errors()`
## Invariants

1. **Signal::silent() is the lattice bottom, and the seed of a fixpoint.** A
   `letrec` binding starts silent and the fixpoint raises it; a callee the
   analyzer cannot see is `Signal::unknown()`, every bit a program can
   raise. The seed is optimistic only where a fixpoint corrects it.

2. **Suspension propagates.** If any sub-expression may suspend, the parent
   may suspend. This includes call sites: calling a suspending function
   propagates suspension.

3. **Polymorphic uses a bitmask.** `propagates` is a u32 bitmask where bit i
   set means parameter i's signals flow through. Higher-order functions like
   `map`, `filter`, `fold` use this. `propagated_params()` iterates the set bits.

4. **assign invalidates tracking.** When a binding is mutated via `assign`, its
   signal becomes uncertain and is removed from `signal_env`.

5. **Signal is Copy.** No allocation, no cloning needed. `const fn` constructors.
