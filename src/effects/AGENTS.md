# effects

Effect system for tracking which expressions may yield.

## Responsibility

Define the `Effect` type and provide effect inference for colorless coroutines.
Effects track whether an expression may suspend execution (yield).

## Interface

| Type | Purpose |
|------|---------|
| `Effect` | `Pure`, `Yields`, `Polymorphic(usize)` |
| `EffectContext` | Tracks known function effects for inference |

## Dependents

Used across both pipelines and the runtime:
- `hir/analyze.rs` — infers effects during analysis
- `hir/expr.rs` — `Hir` carries an `Effect`
- `lir/emit.rs` — emits effect metadata on closures
- `value/closure.rs` — `Closure` stores its `Effect`
- `compiler/cps/` — CPS transform uses effects
- `primitives/coroutines.rs` — coroutine primitives use `EffectContext`

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~140 | `Effect` enum, combine logic, tests |
| `inference.rs` | ~300 | `EffectContext`, effect inference on Expr |
| `primitives.rs` | ~50 | Registers known primitive effects |

## Invariants

1. **Effect::Pure is the default.** Unknown effects start as Pure.
2. **Yields propagates.** If any sub-expression yields, the parent yields.
3. **Polymorphic tracks parameter index.** `Polymorphic(i)` means the effect
   depends on the i-th parameter's effect (for higher-order functions).
