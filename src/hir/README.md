# High-level Intermediate Representation (HIR)

HIR is the bridge between surface syntax and code generation. After macro
expansion, the `Analyzer` transforms Syntax trees into HIR by resolving all
variable references and computing the information needed for correct closure
compilation.

## What HIR Does

**Binding Resolution**: Every variable reference becomes a `BindingId` - a
unique identifier for that specific binding site. This distinguishes between
two variables named `x` in different scopes.

**Capture Analysis**: When a lambda references a variable from an enclosing
scope, HIR records what's captured and how to access it (directly from parent's
locals, transitively through parent's captures, or from globals).

**Mutation Tracking**: Variables modified with `set!` are marked as mutated.
Combined with capture information, this determines which variables need cell
boxing for correct semantics.

**Effect Inference**: Each expression is tagged with its effect (`Pure`, `IO`,
or `Divergent`). Effects propagate upward through the tree.

## Example

```lisp
(let ((x 10))
  (fn () (+ x 1)))
```

The analyzer produces:
- `x` gets `BindingId(0)`, kind `Local`, marked as captured
- The inner lambda has `CaptureInfo` showing it captures `BindingId(0)` from
  parent's local slot 0
- Since `x` is captured but not mutated, it doesn't need cell boxing

## Key Types

```rust
// A resolved variable reference
struct BindingId(u32);

// Metadata about a binding
struct BindingInfo {
    id: BindingId,
    name: SymbolId,      // Original name for errors
    is_mutated: bool,    // Modified by set!
    is_captured: bool,   // Referenced by nested lambda
    kind: BindingKind,   // Parameter, Local, or Global
}

// How a closure captures a variable
struct CaptureInfo {
    binding: BindingId,
    kind: CaptureKind,   // Local, Capture, or Global
    is_mutated: bool,    // Needs cell if true
}
```

## Scope Rules

- `let` creates block scope (doesn't cross function boundaries)
- `fn`/`lambda` creates function scope (capture boundary)
- `define` at top level creates global binding
- `define` inside function creates local binding (letrec semantics in `begin`)

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- `src/syntax/` - input to HIR analysis
- `src/lir/` - consumes HIR output
