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

### Analyzer methods

| Method | Purpose |
|--------|---------|
| `analyze(&mut self, syntax: &Syntax) -> Result<AnalysisResult, String>` | Analyze a single Syntax tree into HIR |
| `analyze_file_letrec(&mut self, forms: Vec<FileForm>, span: Span) -> Result<Hir, String>` | Analyze a list of top-level forms as a synthetic letrec. Classifies each form as `Def` (immutable), `Var` (mutable), or `Expr` (gensym-named dummy binding). Two-pass analysis: Pass 1 pre-binds all names, Pass 2 analyzes initializers sequentially. Returns a single `HirKind::Letrec` node. |
| `bind_primitives(&mut self, hir: Hir) -> Hir` | Wrap a file's letrec in an outer scope that binds all registered primitives as immutable Global bindings. Primitives are visible to all file-level code but can be shadowed by file-level `def` bindings. |

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
- `lint/cli.rs` - uses `HirLinter` for static analysis
- `lsp/state.rs` - uses `extract_symbols_from_hir` and `HirLinter` for IDE features

## Invariants

1. **Every variable reference is a `Binding`.** No symbols in HIR. If you
   see a symbol at this stage, analysis failed.

2. **`Binding` identity is bit-pattern equality.** Two references to the same
   binding site share the same NaN-boxed pointer. `Binding` implements
   `Hash`/`Eq` via `Value::to_bits()`.

3. **`needs_cell()` determines cell boxing.** A local binding needs a cell if
   captured AND mutable. A parameter needs a cell if mutated. Globals never need
   cells. Immutable captured locals are captured by value directly.

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
     `HirPattern::Table { entries: Vec<(PatternKey, HirPattern)> }` maps
     keyword or symbol keys to sub-patterns. `PatternKey::Keyword(String)`
     for `:foo` keys, `PatternKey::Symbol(SymbolId)` for `'foo` keys.
     In binding forms (`def`, `var`, `let`, `fn` params),
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

15. **Docstrings are extracted from leading string literals.**
     `HirKind::Lambda` has a `doc: Option<Value>` field. The analyzer
     extracts the first string literal in a function body and stores it
     in `doc`. This field is threaded through LIR into `Closure.doc` and
     used by the `(doc name)` primitive and LSP hover.

16. **Qualified symbols are desugared to nested `get` calls.**
     `a:b:c` in `SyntaxKind::Symbol` is desugared during analysis to
     `(get (get a :b) :c)`. The first segment is resolved as a variable
     (local or global). Subsequent segments become keyword arguments to
     `get`. This produces standard `HirKind::Call` nodes — no special
     HIR variant. The `get` binding always resolves to the global
     primitive, matching the pattern used for tuple/array/struct/table
     literal desugaring. All synthesized nodes carry the original
     symbol's span.

17. **`Parameterize` creates dynamic binding frames.**
      `HirKind::Parameterize { bindings: Vec<(Hir, Hir)>, body: Box<Hir> }`
      is produced by the analyzer for `(parameterize ((p1 v1) (p2 v2) ...) body ...)`.
      Each binding is a (parameter, value) pair. The analyzer validates that
      each parameter expression is a parameter (or will be at runtime). The
      lowerer emits `PushParamFrame` before evaluating bindings, stores them
      in the frame, then emits `PopParamFrame` after the body.

17. **Files compile to a single synthetic letrec.** `analyze_file_letrec`
    transforms a list of top-level forms into a single `HirKind::Letrec`.
    Each form is classified: `def` → immutable binding, `var` → mutable
    binding, bare expression → gensym-named dummy binding. Two-pass analysis
    pre-binds all names (enabling mutual recursion), then analyzes initializers
    sequentially. The letrec body is the last binding's name (or a gensym if
    the last form was a bare expression). This replaces the old model of
    independent top-level forms connected by mutable globals.

18. **Primitives are pre-bound as immutable Global bindings.** `bind_primitives`
    wraps the file's letrec in an outer scope containing all registered
    primitives. Primitives are `BindingScope::Global` with `mark_immutable()`
    set. File-level `def` bindings shadow primitives. The lowerer emits
    `LoadGlobal` for both, but compile-time checks (e.g., `(set + 42)` is
    an error) use the `Binding` identity.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | 25 | Re-exports |
| `analyze/mod.rs` | ~500 | `Analyzer` struct, `ScopedBinding`, scope-aware resolution, `analyze_file_letrec`, `bind_primitives` |
| `analyze/forms.rs` | ~355 | Core form analysis: `analyze_expr`, control flow |
| `analyze/binding.rs` | ~425 | Binding forms: `let`, `letrec`, `def`/`var`, `set` |
| `analyze/destructure.rs` | ~215 | Destructuring pattern analysis, define-form detection, rest-pattern splitting |
| `analyze/lambda.rs` | ~160 | Lambda/fn analysis with captures, params, effects |
| `analyze/special.rs` | ~210 | Special forms: `match`, `yield` |
| `analyze/call.rs` | ~200 | Call analysis and effect tracking |
| `expr.rs` | ~180 | `Hir`, `HirKind` |
| `binding.rs` | ~110 | `Binding(Value)` newtype, `CaptureInfo`, `CaptureKind` |
| `pattern.rs` | ~100 | Pattern matching types |
| `tailcall.rs` | ~462 | Post-analysis pass marking tail calls |
| `lint.rs` | ~150 | HIR-based linter (walks HirKind, produces Diagnostics) |
| `symbols.rs` | ~200 | HIR-based symbol extraction (builds SymbolIndex) |
