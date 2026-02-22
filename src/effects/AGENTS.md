# effects

Effect system for tracking which signals a function may emit.

## Responsibility

Define the `Effect` type and provide effect inference for the fiber/signal
system. Effects track which signals a function might emit (error, yield,
debug, ffi) and which parameter indices propagate their callee's effects.

## Interface

| Type/Function | Purpose |
|---------------|---------|
| `Effect` | `{ bits: SignalBits, propagates: u32 }` — Copy, const fn constructors |
| `Effect::none()` | No effects |
| `Effect::raises()` | May raise (SIG_ERROR) |
| `Effect::yields()` | May yield (SIG_YIELD) |
| `Effect::yields_raises()` | May yield and raise |
| `Effect::ffi()` | Calls foreign code (SIG_FFI) |
| `Effect::polymorphic(n)` | Effect depends on parameter n |
| `Effect::polymorphic_raises(n)` | Polymorphic + may raise |
| `get_primitive_effects` | Returns effect map for already-interned primitives |

## Predicates

Each predicate asks a specific question. No vague "is_pure".

| Predicate | Meaning |
|-----------|---------|
| `may_suspend()` | Can suspend execution? (yield, debug, or polymorphic) |
| `may_yield()` | Can yield? (SIG_YIELD) |
| `may_raise()` | Can raise an error? (SIG_ERROR) |
| `may_ffi()` | Calls foreign code? (SIG_FFI) |
| `is_polymorphic()` | Effect depends on arguments? (propagates != 0) |
| `propagated_params()` | Iterator over propagated parameter indices |

## Deprecated (backward compat)

| Deprecated | Use instead |
|------------|-------------|
| `Effect::pure()` | `Effect::none()` |
| `Effect::pure_raises()` | `Effect::raises()` |
| `effect.is_pure()` | `!effect.may_suspend()` |

## Interprocedural Effect Tracking

The analyzer performs interprocedural effect tracking:

1. **effect_env**: Maps `BindingId` → `Effect` for locally-defined functions
2. **primitive_effects**: Maps `SymbolId` → `Effect` for primitive functions

When analyzing a call:
- Direct fn calls: use the fn body's effect
- Variable calls: look up in `effect_env` (local) or `primitive_effects` (global)
- Polymorphic effects: resolve by examining the argument's effect via
  `propagated_params()` iterator over the `propagates` bitmask

### Limitations

- Effects are tracked within a single compilation unit
- Cross-unit effect tracking is not implemented
- `set!` invalidates effect tracking for the mutated binding
- Mutual recursion in `letrec` may have incomplete effect information

## Dependents

Used across the pipeline and the runtime:
- `hir/analyze/call.rs` — infers effects during analysis, resolves polymorphic via `propagates` bitmask
- `hir/expr.rs` — `Hir` carries an `Effect`
- `lir/emit.rs` — emits effect metadata on closures
- `value/closure.rs` — `Closure` stores its `Effect`
- `pipeline.rs` — builds primitive effects map, passes to Analyzer
- `jit/compiler.rs` — JIT gate checks `!effect.may_suspend()`
- `vm/call.rs` — call dispatch checks `!effect.may_suspend()`
- `primitives/coroutines.rs` — coroutine warnings check `!effect.may_yield()`

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~350 | `Effect` struct, constructors, predicates, Display, combine, tests |
| `primitives.rs` | ~220 | Registers known primitive effects |

## Invariants

1. **Effect::none() is the default.** Unknown effects start as none. This is
   conservative — we may miss some suspension propagation but never produce
   false positives.

2. **Suspension propagates.** If any sub-expression may suspend, the parent
   may suspend. This includes call sites: calling a suspending function
   propagates suspension.

3. **Polymorphic uses a bitmask.** `propagates` is a u32 bitmask where bit i
   set means parameter i's effects flow through. Higher-order functions like
   `map`, `filter`, `fold` use this. `propagated_params()` iterates the set bits.

4. **set! invalidates tracking.** When a binding is mutated via `set!`, its
   effect becomes uncertain and is removed from `effect_env`.

5. **Effect is Copy.** No allocation, no cloning needed. `const fn` constructors.
