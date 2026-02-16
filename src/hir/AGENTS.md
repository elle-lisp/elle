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

## Invariants

1. **Every variable reference is a `BindingId`.** No symbols in HIR. If you
   see a symbol at this stage, analysis failed.

2. **`BindingId` is unique per binding site.** Two `let x` in different
   scopes get different IDs, unlike `SymbolId` which is per-name.

3. **`needs_cell()` determines cell boxing.** A binding needs a cell if
   it's captured AND mutated, or if it's a local that's captured.

4. **Effects combine upward.** A `begin` has the combined effect of its
   children. A `lambda` body's effect is stored but the lambda itself is Pure.

5. **Captures are computed per-lambda.** Each `HirKind::Lambda` carries its
   own `Vec<CaptureInfo>` listing what it captures and how.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | 21 | Re-exports |
| `analyze.rs` | 1400 | `Analyzer`, scope tracking, HIR construction |
| `expr.rs` | 180 | `Hir`, `HirKind` |
| `binding.rs` | 120 | `BindingId`, `BindingInfo`, `CaptureInfo` |
| `pattern.rs` | ~100 | Pattern matching types |
