# High-level Intermediate Representation (HIR)

HIR is the bridge between surface syntax and code generation. After macro
expansion, the `Analyzer` transforms Syntax trees into HIR by resolving all
variable references and computing the information needed for correct closure
compilation.

## What HIR Does

**Binding Resolution**: Every variable reference becomes a `Binding` — a
NaN-boxed Value pointing to a heap-allocated `BindingInner`. Each `Binding`
is unique per binding site (two variables named `x` in different scopes get
different Bindings). Identity is pointer equality (bit-pattern comparison).

**Capture Analysis**: When a lambda references a variable from an enclosing
scope, HIR records what's captured and how to access it (directly from parent's
locals, transitively through parent's captures, or from globals).

**Mutation Tracking**: Variables modified with `set!` are marked as mutated
via `binding.mark_mutated()`. Combined with capture information, this
determines which variables need cell boxing for correct semantics.

**Effect Inference**: Each expression is tagged with its effect (`Pure`, `Yields`,
or `Polymorphic`). Effects propagate upward through the tree.

## Example

```lisp
(let ((x 10))
  (fn () (+ x 1)))
```

The analyzer produces:
- `x` gets a `Binding` with scope `Local`, marked as captured
- The inner lambda has `CaptureInfo` showing it captures `x`'s Binding from
  parent's local slot
- Since `x` is captured but not mutated, it doesn't need cell boxing

## Key Types

```rust
// A resolved variable reference — NaN-boxed pointer to heap BindingInner
struct Binding(Value);  // Copy, 8 bytes

// Metadata stored on the heap (mutable during analysis, read-only after)
struct BindingInner {
    name: SymbolId,        // Original name for errors
    scope: BindingScope,   // Parameter, Local, or Global
    is_mutated: bool,      // Modified by set!
    is_captured: bool,     // Referenced by nested lambda
    is_immutable: bool,    // Defined with def (not var)
}

// How a closure captures a variable
struct CaptureInfo {
    binding: Binding,
    kind: CaptureKind,   // Local, Capture, or Global
}
```

## Scope Rules

- `let` creates block scope (doesn't cross function boundaries)
- `fn`/`lambda` creates function scope (capture boundary)
- `def`/`var` at top level creates global binding (`def` is immutable, `var` is mutable)
- `def`/`var` inside function creates local binding (letrec semantics in `begin`)

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- `src/syntax/` - input to HIR analysis
- `src/lir/` - consumes HIR output
