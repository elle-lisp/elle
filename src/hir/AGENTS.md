# hir

High-level Intermediate Representation. Fully-analyzed form with resolved
bindings, inferred effects, and computed captures.

## Responsibility

Transform expanded Syntax into a representation suitable for lowering.
- Resolve all variable references to `Binding` (NaN-boxed heap objects)
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
| `Binding` | NaN-boxed Value wrapping `HeapObject::Binding(RefCell<BindingInner>)` — Copy, identity by bit-pattern |
| `BindingScope` | `Parameter`, `Local`, or `Global` (in `value::heap`) |
| `CaptureInfo` | What a closure captures and how |
| `CaptureKind` | `Local`, `Capture` (transitive), or `Global` |
| `BlockId` | Unique identifier for a block, used by `break` to target the correct block |
| `Analyzer` | Transforms Syntax → HIR |
| `AnalysisResult` | HIR (no separate bindings map — metadata is inline in Binding) |
| `HirLinter` | HIR-based linter producing Diagnostics (no constructor args) |
| `extract_symbols_from_hir` | Builds SymbolIndex from HIR (2 args: hir, symbols) |

## Data flow

```
Syntax (expanded)
    │
    ▼
Analyzer
    ├─► resolve variables → Binding (heap-allocated, shared by reference)
    ├─► track mutations → binding.mark_mutated()
    ├─► track captures → binding.mark_captured() + CaptureInfo
    └─► infer effects → Effect
    │
    ▼
HIR (bindings are inline — no separate HashMap)
```

## Dependents

- `lir/lower/` - consumes HIR, reads `binding.needs_cell()` / `binding.is_global()` directly
- `pipeline.rs` - orchestrates Syntax → HIR → LIR → Bytecode
- `elle-lint` - uses `HirLinter` for static analysis
- `elle-lsp` - uses `extract_symbols_from_hir` and `HirLinter` for IDE features

## Invariants

1. **Every variable reference is a `Binding`.** No symbols in HIR. If you
   see a symbol at this stage, analysis failed.

2. **`Binding` identity is bit-pattern equality.** Two references to the same
   binding site share the same NaN-boxed pointer. `Binding` implements
   `Hash`/`Eq` via `Value::to_bits()`.

3. **`needs_cell()` determines cell boxing.** A local binding needs a cell if
   captured. A parameter needs a cell if mutated. Globals never need cells.

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

8. **`Define` and `LocalDefine` are unified.** There is a single
   `HirKind::Define { binding, value }`. The lowerer checks
   `binding.is_global()` to decide between global and local define semantics.

9. **Binding metadata is mutable during analysis, read-only after.** The
   analyzer calls `mark_mutated()`, `mark_captured()`, `mark_immutable()`.
   The lowerer only reads via `needs_cell()`, `is_global()`, `name()`, etc.

10. **`Destructure` decomposes values into pattern bindings.** 
    `HirKind::Destructure { pattern: HirPattern, value: Box<Hir> }` is
    produced by the analyzer for `def`, `var`, `let`, and `fn` parameter
    destructuring. The pattern's leaf `Var` bindings are created in the
    current scope. `let*` is desugared to nested `let` in the expander,
    so the analyzer never sees `let*`.

11. **Destructured bindings use silent nil semantics.** Missing list/array/table
     elements produce `nil`, not errors. Wrong-type values produce `nil`
     for all bindings. No runtime type checks.

12. **`HirPattern::Table` supports table/struct destructuring.**
     `HirPattern::Table { entries: Vec<(String, HirPattern)> }` maps keyword
     keys to sub-patterns. In binding forms (`def`, `var`, `let`, `fn` params),
     uses `TableGetOrNil` with silent nil. In `match` patterns, emits an
     `IsTable` type guard first so non-table values fall through to the next arm.

13. **`Block` and `Break` are compile-time control flow.** `HirKind::Block`
    has a `BlockId` and optional name. `HirKind::Break` targets a `BlockId`.
    The analyzer validates: break outside block → error, unknown block name
    → error, break across function boundary → error. The lowerer compiles
    break to `Move` + `Jump` — no new bytecode instructions needed.
    `while` wraps its `While` node in an implicit `Block` named `"while"`,
    so `(break :while val)` or unnamed `(break)` can exit a while loop.

14. **`Eval` compiles and executes a datum at runtime.**
    `HirKind::Eval { expr: Box<Hir>, env: Box<Hir> }` is produced by the
    analyzer for `(eval expr)` or `(eval expr env)`. The effect is always
    `Yields` (conservative — eval'd code can do anything). Not in tail
    position. The VM handler accesses the symbol table via thread-local
    context and caches the Expander on the VM for reuse.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | 25 | Re-exports |
| `analyze/mod.rs` | ~560 | `Analyzer` struct, `ScopedBinding`, scope-aware resolution |
| `analyze/forms.rs` | ~355 | Core form analysis: `analyze_expr`, control flow |
| `analyze/binding.rs` | ~425 | Binding forms: `let`, `letrec`, `def`/`var`, `set!` |
| `analyze/destructure.rs` | ~215 | Destructuring pattern analysis, define-form detection, rest-pattern splitting |
| `analyze/lambda.rs` | ~160 | Lambda/fn analysis with captures, params, effects |
| `analyze/special.rs` | ~180 | Special forms: `match`, `yield`, `module` |
| `analyze/call.rs` | ~200 | Call analysis and effect tracking |
| `expr.rs` | ~180 | `Hir`, `HirKind` |
| `binding.rs` | ~110 | `Binding(Value)` newtype, `CaptureInfo`, `CaptureKind` |
| `pattern.rs` | ~100 | Pattern matching types |
| `tailcall.rs` | ~462 | Post-analysis pass marking tail calls |
| `lint.rs` | ~150 | HIR-based linter (walks HirKind, produces Diagnostics) |
| `symbols.rs` | ~200 | HIR-based symbol extraction (builds SymbolIndex) |
