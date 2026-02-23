# hir

High-level Intermediate Representation. Fully-analyzed form with resolved
bindings, inferred effects, and computed captures.

## Responsibility

Transform expanded Syntax into a representation suitable for lowering.
- Resolve all variable references to `BindingId`
- Compute closure captures
- Infer effects
- Validate scope rules

Does NOT:
- Generate code (that's `lir` and `compiler`)
- Execute anything (that's `vm`)
- Parse source (that's `reader` and `syntax`)

## Interface

| Type | Purpose |
|------|---------|
| `Hir` | Expression node with kind, span, effect |
| `HirKind` | Expression variants (literals, control flow, etc.) |
| `BindingId` | Unique identifier per binding site |
| `BindingInfo` | Metadata: name, mutated?, captured?, kind |
| `BindingKind` | `Parameter`, `Local`, or `Global` |
| `CaptureInfo` | What a closure captures and how |
| `CaptureKind` | `Local`, `Capture` (transitive), or `Global` |
| `Analyzer` | Transforms Syntax → HIR |
| `AnalysisResult` | HIR + binding metadata |
| `HirLinter` | HIR-based linter producing Diagnostics |
| `extract_symbols_from_hir` | Builds SymbolIndex from HIR |

## Data flow

```
Syntax (expanded)
    │
    ▼
Analyzer
    ├─► resolve variables → BindingId
    ├─► track mutations → is_mutated
    ├─► track captures → CaptureInfo
    └─► infer effects → Effect
    │
    ▼
HIR + bindings HashMap
```

## Dependents

- `lir/lower.rs` - consumes HIR, uses binding info for cell decisions
- `pipeline.rs` - orchestrates Syntax → HIR → LIR → Bytecode
- `elle-lint` - uses `HirLinter` for static analysis
- `elle-lsp` - uses `extract_symbols_from_hir` and `HirLinter` for IDE features

## Invariants

1. **Every variable reference is a `BindingId`.** No symbols in HIR. If you
   see a symbol at this stage, analysis failed.

2. **`BindingId` is unique per binding site.** Two `let x` in different
   scopes get different IDs, unlike `SymbolId` which is per-name.

3. **`needs_cell()` determines cell boxing.** A binding needs a cell if
   it's captured AND mutated, or if it's a local that's captured.

4. **Effects combine upward.** A `begin` has the combined effect of its
   children. A `fn` body's effect is stored but the fn itself is Pure.

5. **Captures are computed per-fn.** Each `HirKind::Lambda` carries its
   own `Vec<CaptureInfo>` listing what it captures and how.

6. **Empty lists become `HirKind::EmptyList`, not `HirKind::Nil`.** The analyzer
   distinguishes between `nil` (absence) and `()` (empty list). Conflating them
   breaks truthiness semantics.

7. **Binding resolution is scope-aware (hygienic).** `bind()` stores a
   `Vec<ScopeId>` alongside each binding. `lookup()` uses subset matching:
   a binding is visible to a reference if the binding's scope set is a subset
   of the reference's scope set. When multiple bindings match, the one with
   the largest scope set wins (most specific). Empty scopes `[]` is a subset
   of everything, so pre-expansion code works identically.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | 25 | Re-exports |
| `analyze/mod.rs` | ~600 | `Analyzer` struct, `AnalysisContext`, `ScopedBinding`, scope-aware resolution |
| `analyze/forms.rs` | ~355 | Core form analysis: `analyze_expr`, control flow |
| `analyze/binding.rs` | ~460 | Binding forms: `let`, `def`, `var`, `fn` |
| `analyze/special.rs` | ~180 | Special forms: `match`, `yield`, `module` |
| `analyze/call.rs` | ~200 | Call analysis and effect tracking |
| `expr.rs` | 180 | `Hir`, `HirKind` |
| `binding.rs` | 120 | `BindingId`, `BindingInfo`, `CaptureInfo` |
| `pattern.rs` | ~100 | Pattern matching types |
| `tailcall.rs` | ~462 | Post-analysis pass marking tail calls |
| `lint.rs` | ~150 | HIR-based linter (walks HirKind, produces Diagnostics) |
| `symbols.rs` | ~200 | HIR-based symbol extraction (builds SymbolIndex) |
