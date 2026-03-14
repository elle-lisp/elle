# HIR Analysis

The analysis phase transforms expanded `Syntax` trees into `HIR` by resolving all variable references, computing closure captures, and inferring signals.

## What Analysis Does

1. **Binding Resolution**: Every variable reference becomes a `Binding` — a unique NaN-boxed pointer to a heap-allocated `BindingInner`. Identity is pointer equality.

2. **Capture Analysis**: When a lambda references a variable from an enclosing scope, the analyzer records what's captured and how to access it (directly from parent's locals, transitively through parent's captures, or from globals).

3. **Mutation Tracking**: Variables modified with `set` are marked as mutated. Combined with capture information, this determines which variables need lbox boxing for correct semantics.

4. **Signal Inference**: Each expression is tagged with its signal (`Silent`, `Yields`, or `Polymorphic`). Signals propagate upward through the tree.

## Scope Rules

- `let` creates block scope (doesn't cross function boundaries)
- `fn`/`lambda` creates function scope (capture boundary)
- `def`/`var` at top level creates global binding (`def` is immutable, `var` is mutable)
- `def`/`var` inside function creates local binding

## Key Files

| File | Purpose |
|------|---------|
| [`mod.rs`](mod.rs) | `Analyzer` struct, scope-aware resolution |
| [`forms.rs`](forms.rs) | Core form analysis: `analyze_expr`, control flow |
| [`binding.rs`](binding.rs) | Binding forms: `let`, `letrec`, `def`/`var`, `set` |
| [`destructure.rs`](destructure.rs) | Destructuring pattern analysis |
| [`lambda.rs`](lambda.rs) | Lambda/fn analysis with captures |
| [`special.rs`](special.rs) | Special forms: `match`, `yield` |
| [`call.rs`](call.rs) | Call analysis and signal tracking |

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`src/hir/`](../) - HIR types and overview
- [`src/lir/lower/`](../../lir/lower/) - consumes HIR output
