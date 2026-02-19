# effects

Effect system for tracking which expressions may yield.

## Responsibility

Define the `Effect` type and provide effect inference for colorless coroutines.
Effects track whether an expression may suspend execution (yield).

## Interface

| Type/Function | Purpose |
|---------------|---------|
| `Effect` | `Pure`, `Yields`, `Polymorphic(usize)` |
| `register_primitive_effects` | Populates effect map for primitives (mutable symbols) |
| `get_primitive_effects` | Returns effect map for already-interned primitives |

## Interprocedural Effect Tracking

The analyzer performs interprocedural effect tracking:

1. **effect_env**: Maps `BindingId` → `Effect` for locally-defined lambdas
2. **primitive_effects**: Maps `SymbolId` → `Effect` for primitive functions

When analyzing a call:
- Direct lambda calls: use the lambda body's effect
- Variable calls: look up in `effect_env` (local) or `primitive_effects` (global)
- Polymorphic effects: resolve by examining the argument's effect

### Limitations

- Effects are tracked within a single compilation unit
- Cross-unit effect tracking is not implemented
- `set!` invalidates effect tracking for the mutated binding
- Mutual recursion in `letrec` may have incomplete effect information

## Dependents

Used across the pipeline and the runtime:
- `hir/analyze.rs` — infers effects during analysis, interprocedural tracking
- `hir/expr.rs` — `Hir` carries an `Effect`
- `lir/emit.rs` — emits effect metadata on closures
- `value/closure.rs` — `Closure` stores its `Effect`
- `pipeline.rs` — builds primitive effects map, passes to Analyzer

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~140 | `Effect` enum, combine logic, tests |
| `primitives.rs` | ~220 | Registers known primitive effects |

## Invariants

1. **Effect::Pure is the default.** Unknown effects start as Pure. This is
   conservative — we may miss some `Yields` propagation but never produce
   false positives.

2. **Yields propagates.** If any sub-expression yields, the parent yields.
   This includes call sites: calling a yielding function propagates `Yields`.

3. **Polymorphic tracks parameter index.** `Polymorphic(i)` means the effect
   depends on the i-th parameter's effect (for higher-order functions like
   `map`, `filter`, `fold`).

4. **set! invalidates tracking.** When a binding is mutated via `set!`, its
   effect becomes uncertain and is removed from `effect_env`.
