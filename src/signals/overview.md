# Signals

<!-- audited: 2026-10-22 -->

The compile-time `Signal` each function carries, the registry that names each signal bit, and the predicates every tier asks of a signal.

A `Signal` is `{ bits, propagates }`. `bits` is the set of signals a function
may emit. `propagates` is the set of parameter positions whose argument's
signal the function passes on when it calls them. The analyzer infers one for
every function, and the compiler chooses a call's convention from it. At run
time a closure carries the flat `SignalBits` alone, which has no
`propagates` ([signalbits.rs](../value/fiber/signalbits.rs)).

| Subject | Owner |
|---------|-------|
| What the analyzer infers, and the forms that bound or check it | [inference](../../docs/signals/inference.md) |
| How a call site reads its callee's signal | [the analyzer](../hir/analyze/AGENTS.md) |
| The bit layout, the registry and user signals | [protocol](../../docs/signals/protocol.md) |
| The runtime bits from 11 up | [runtime signals](../../docs/runtime.md) |
| Which bits a fiber can withhold | [capabilities](../../docs/signals/capabilities.md) |

## One predicate per question

Every tier asks the same questions of a signal that crosses a boundary. Each
question has one function here, so the answers cannot drift apart between
the interpreter, the JIT and the WASM tier:

| Question | Function |
|----------|----------|
| Which bits does a `squelch` or `attune` boundary turn into a violation? | `squelched_bits` |
| Do these bits, leaving a silent function, break its proof? | `breaks_proof` |
| Do these bits park the raising frame and every caller behind it? | `dispatch::parks` |
| What does a `(bound p spec)` entry check raise for this value? | `bound::violation` |

A primitive's return signal routes through `dispatch::classify`, which the
interpreter and the JIT share.

## Invariants

1. **`Signal::silent()` is the lattice bottom, and the seed of a fixpoint.** A
   `letrec` binding starts silent and the fixpoint raises it. A callee the
   analyzer cannot see is `Signal::unknown()`, every bit a program can raise.
   The seed is optimistic only where a fixpoint corrects it.
2. **Suspension propagates.** If any sub-expression may suspend, the
   expression around it may suspend. A call to a function that may suspend
   may suspend.
3. **`propagates` is a bitmask.** Bit i set means parameter i's signal flows
   through. `map`, `filter` and `fold` carry one, and `propagated_params()`
   iterates its set bits.
4. **`Signal` is `Copy`.** Its constructors are `const fn`, and building one
   allocates nothing. Build one through a named constructor in
   [constructors.rs](constructors.rs) where one exists.
