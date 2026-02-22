# binding

Compile-time variable resolution. Transforms symbol references into concrete
locations (local slot, upvalue index, global lookup).

## Responsibility

This module answers: "Where does this variable live?" It does NOT:
- Execute lookups (that's the VM)
- Manage runtime environments (that's `vm/scope`)
- Track types or effects (that's `hir`)

## Interface

| Type | Purpose |
|------|---------|
| `VarRef` | Resolved location: `Local`, `LetBound`, `Upvalue`, `Global` |
| `ResolvedVar` | `VarRef` + boxing flag for mutable captures |
| `Binding` | Metadata: symbol, index, captured?, mutated? |
| `Scope` | Single lexical level with bindings |
| `ScopeStack` | Nested scopes for traversal |

## Data flow

```
AST traversal
    │
    ├─► push scope (entering fn/let)
    ├─► bind (defining variable)
    ├─► lookup (referencing variable) → VarRef
    ├─► mark_captured/mark_mutated (set!/nested fn)
    └─► pop scope (leaving)
```

## Dependents

Used by the HIR/LIR pipeline:
- `hir/` - binding resolution produces `BindingId`
- `lir/` - lowerer maps `BindingId` to slot indices

## Invariants

1. **All resolution happens at compile time.** If `VarRef` appears at runtime,
   something is architecturally wrong.

2. **`Upvalue` index is placeholder until capture resolution.** The `index`
   field gets rewritten during `adjust_var_indices` in the compiler.

3. **`captured` and `mutated` flags drive cell boxing.** A variable that is
   both captured AND mutated needs `LocalCell` wrapping.

4. **Function scopes create activation frames; block scopes don't.**
   `is_function: true` means new frame. `let` creates block scope only.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | 30 | Re-exports |
| `scope.rs` | 280 | `Scope`, `ScopeStack`, `Binding` |
| `varref.rs` | 150 | `VarRef`, `ResolvedVar` |
