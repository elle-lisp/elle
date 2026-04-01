# hir

High-level Intermediate Representation. Fully-analyzed form with resolved
bindings, inferred signals, and computed captures.

## Responsibility

Transform expanded Syntax into a representation suitable for lowering.
- Resolve all variable references to `Binding` (arena indices)
- Compute closure captures
- Infer signals
- Validate scope rules

Does NOT:
- Generate code (that's `lir` and `compiler`)
- Execute anything (that's `vm`)
- Parse source (that's `reader` and `syntax`)

## Interface

| Type | Purpose |
|------|---------|
| `Hir` | Expression node with kind, span, signal |
| `HirKind` | Expression variants (literals, control flow, etc.) |
| `Binding` | `u32` index into `BindingArena` — 4 bytes, Copy, identity by integer equality |
| `BindingArena` | Owns all `BindingInner` values for a compilation unit; `&mut` in analysis, `&` in lowering |
| `BindingInner` | Binding metadata: name, scope, mutation/capture/immutability flags |
| `BindingScope` | `Parameter` or `Local` (in `hir::arena`) |
| `CaptureInfo` | What a closure captures and how |
| `CaptureKind` | `Local` or `Capture` (transitive) |
| `BlockId` | Unique identifier for a block, used by `break` to target the correct block |
| `Analyzer` | Transforms Syntax → HIR; takes `&mut BindingArena` |
| `AnalysisResult` | HIR produced by the analyzer |
| `HirLinter` | HIR-based linter producing Diagnostics (no constructor args) |
| `extract_symbols_from_hir` | Builds SymbolIndex from HIR (3 args: hir, symbols, arena) |

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
Analyzer (&mut BindingArena)
    ├─► resolve variables → Binding (u32 index into BindingArena)
    ├─► track mutations → arena.get_mut(b).is_mutated = true
    ├─► track captures → arena.get_mut(b).is_captured = true + CaptureInfo
    └─► infer signals → Signal
    │
    ▼
HIR (binding indices are inline — metadata lives in BindingArena)
    │
    ▼
Lowerer (&BindingArena) — read-only access to binding metadata
```

## Dependents

- `lir/lower/` - consumes HIR, reads `arena.get(b).needs_lbox()` via `&BindingArena`
- `pipeline.rs` - orchestrates Syntax → HIR → LIR → Bytecode
- `lint/cli.rs` - uses `HirLinter` for static analysis
- `lsp/state.rs` - uses `extract_symbols_from_hir` and `HirLinter` for IDE features

## Invariants

1. **Every variable reference is a `Binding`.** No symbols in HIR. If you
   see a symbol at this stage, analysis failed.

2. **`Binding` identity is integer equality.** Two references to the same
   binding site have the same `u32` index. `Binding` implements `Hash`/`Eq`
   via the derived `u32` comparison.

3. **`needs_lbox()` determines lbox boxing.** A local binding needs an lbox if
   captured AND mutable. A parameter needs an lbox if mutated. Globals never need
   lboxes. Immutable captured locals are captured by value directly.

4. **Signals combine upward.** A `begin` has the combined signal of its
   children. A `fn` body's signal is stored but the fn itself is Silent.

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
   analyzer mutates bindings via `arena.get_mut(b).is_mutated = true` etc.
   The lowerer only reads via `arena.get(b).needs_lbox()`, `arena.get(b).name`,
   etc. The type system enforces this: the analyzer holds `&mut BindingArena`,
   the lowerer holds `&BindingArena`.

10. **`Destructure` decomposes values into pattern bindings.** 
    `HirKind::Destructure { pattern: HirPattern, value: Box<Hir> }` is
    produced by the analyzer for `def`, `var`, `let`, and `fn` parameter
    destructuring. The pattern's leaf `Var` bindings are created in the
    current scope. `let*` is desugared to nested `let` in the expander,
    so the analyzer never sees `let*`.

11. **Destructured bindings use silent nil semantics.** Missing list/@array/@struct
     elements produce `nil`, not errors. Wrong-type values produce `nil`
     for all bindings. No runtime type checks.

12. **`HirPattern::Table` and `HirPattern::Struct` support struct destructuring with optional rest.**
     Both `Struct { entries: Vec<(PatternKey, HirPattern)>, rest: Option<Box<HirPattern>> }`
     and `Table { entries, rest }` map keyword or symbol keys to sub-patterns.
     `PatternKey::Keyword(String)` for `:foo` keys, `PatternKey::Symbol(SymbolId)` for `'foo` keys.
     When `rest` is `Some(pat)`, the rest pattern binds a new immutable struct of all keys
     NOT explicitly named. Rest is `None` at all construction sites by default.
     In binding forms (`def`, `var`, `let`, `fn` params), uses `TableGetDestructure`
     (strict: error on missing key) for entries, `StructRest` for the rest.
     In `match` patterns, emits an `IsStruct`/`IsTable` type guard first so wrong-type
     values fall through to the next arm.

13. **`Block` and `Break` are compile-time control flow.** `HirKind::Block`
    has a `BlockId` and optional name. `HirKind::Break` targets a `BlockId`.
    The analyzer validates: break outside block → error, unknown block name
    → error, break across function boundary → error. The lowerer compiles
    break to `Move` + `Jump` — no new bytecode instructions needed.
    `while` wraps its `While` node in an implicit `Block` named `"while"`,
    so `(break :while val)` or unnamed `(break)` can exit a while loop.

14. **`Eval` compiles and executes a datum at runtime.**
    `HirKind::Eval { expr: Box<Hir>, env: Box<Hir> }` is produced by the
    analyzer for `(eval expr)` or `(eval expr env)`. The signal is always
    `Yields` (conservative — eval'd code can do anything). Not in tail
    position. The VM handler accesses the symbol table via thread-local
    context and caches the Expander on the VM for reuse.

15. **Docstrings are extracted from leading string literals.**
       `HirKind::Lambda` has a `doc: Option<Value>` field. The analyzer
       extracts the first string literal in a function body and stores it
       in `doc`. This field is threaded through LIR into `Closure.doc` and
       used by the `(doc name)` primitive and LSP hover.

16. **Signal bounds are declared via `silence` preambles.**
        `HirKind::Lambda` has signal-related fields (see `signals/AGENTS.md`):
        - `inferred_signals: Signal` — the minimum guaranteed signal set
        - `param_bounds: Vec<(Binding, Signal)>` (from `(silence param)`)
        `(silence)` declares total silence; `(silence param)` bounds a parameter.
        `squelch` is a runtime primitive, not a preamble.
        When a parameter has a `squelch` bound, it remains polymorphic — the bound only restricts what signals are forbidden.

17. **Set literals are desugared to constructor calls.**
      `SyntaxKind::Set` (immutable set `|1 2 3|`) desugars to `(set ;elems)`.
      `SyntaxKind::SetMut` (mutable set `@|1 2 3|`) desugars to `(mutable-set ;elems)`.
      The `set` and `mutable-set` bindings resolve to global primitives.
      All synthesized nodes carry the original set literal's span.

18. **Original syntax is captured for eval reconstruction.**
       `HirKind::Lambda` has a `syntax: Option<Rc<Syntax>>` field that stores
       the original lambda `Syntax` node, captured in `analyze_lambda` from
       the input `Syntax`. This enables `eval` to reconstruct closures in the
       environment. The field is threaded through LIR and set on `Closure.syntax`
       by the emitter.

19. **Qualified symbols are desugared to nested `get` calls.**
      `a:b:c` in `SyntaxKind::Symbol` is desugared during analysis to
      `(get (get a :b) :c)`. The first segment is resolved as a variable
      (local or global). Subsequent segments become keyword arguments to
      `get`. This produces standard `HirKind::Call` nodes — no special
      HIR variant. The `get` binding always resolves to the global
       primitive, matching the pattern used for array/@array/struct/@struct
       literal desugaring. All synthesized nodes carry the original
      symbol's span.

19. **`Parameterize` creates dynamic binding frames.**
       `HirKind::Parameterize { bindings: Vec<(Hir, Hir)>, body: Box<Hir> }`
       is produced by the analyzer for `(parameterize ((p1 v1) (p2 v2) ...) body ...)`.
       Each binding is a (parameter, value) pair. The analyzer validates that
       each parameter expression is a parameter (or will be at runtime). The
       lowerer emits `PushParamFrame` before evaluating bindings, stores them
       in the frame, then emits `PopParamFrame` after the body.
 
 20. **Files compile to a single synthetic letrec.** `analyze_file_letrec`
     transforms a list of top-level forms into a single `HirKind::Letrec`.
     Each form is classified: `def` → immutable binding, `var` → mutable
     binding, bare expression → gensym-named dummy binding. Two-pass analysis
     pre-binds all names (enabling mutual recursion), then analyzes initializers
     sequentially. The letrec body is the last binding's name (or a gensym if
     the last form was a bare expression). This replaces the old model of
     independent top-level forms connected by mutable globals.

 21. **Primitives are pre-bound as immutable Local bindings.** `bind_primitives`
     wraps the file's letrec in an outer scope containing all registered
     primitives. Primitives are `BindingScope::Local` with `is_immutable = true`
     set via the arena. File-level `def` bindings shadow primitives. The lowerer
     emits upvalue loads for both — compile-time checks (e.g., `(set + 42)` is
     an error) use the `Binding` identity.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | 25 | Re-exports |
| `analyze/mod.rs` | ~500 | `Analyzer` struct, `ScopedBinding`, scope-aware resolution, `analyze_file_letrec`, `bind_primitives` |
| `analyze/forms.rs` | ~355 | Core form analysis: `analyze_expr`, control flow |
| `analyze/binding.rs` | ~425 | Binding forms: `let`, `letrec`, `def`/`var`, `set` |
| `analyze/destructure.rs` | ~215 | Destructuring pattern analysis, define-form detection, rest-pattern splitting |
| `analyze/lambda.rs` | ~160 | Lambda/fn analysis with captures, params, signals |
| `analyze/special.rs` | ~210 | Special forms: `match`, `yield` |
| `analyze/call.rs` | ~200 | Call analysis and signal tracking |
| `expr.rs` | ~180 | `Hir`, `HirKind` |
| `binding.rs` | ~40 | `Binding(u32)` index type, `CaptureInfo`, `CaptureKind` |
| `arena.rs` | ~150 | `BindingArena`, `BindingInner`, `BindingScope` |
| `pattern.rs` | ~100 | Pattern matching types |
| `tailcall.rs` | ~462 | Post-analysis pass marking tail calls |
| `lint.rs` | ~150 | HIR-based linter (walks HirKind, produces Diagnostics) |
| `symbols.rs` | ~200 | HIR-based symbol extraction (builds SymbolIndex) |
