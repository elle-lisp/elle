# hir

<!-- audited: 2026-10-06 -->

High-level Intermediate Representation: the analyzed program, with bindings
resolved, captures computed and signals inferred, and the passes over it.

## Responsibility

Analyze expanded Syntax into HIR, then transform HIR for lowering: loop
fusion, the graft, dead-binding elimination, tail marking, functionalization,
ANF, type inference and the fold, and region and escape analysis.

- Resolve all variable references to `Binding` (arena indices)
- Compute closure captures
- Infer signals
- Validate scope rules

Does NOT:
- Generate code (that's `lir` and `compiler`)
- Execute anything (that's `vm`)
- Parse source (that's `reader` and `syntax`)

[analyze/AGENTS.md](analyze/AGENTS.md) owns how the analyzer builds HIR.
[docs/impl/hir.md](../../docs/impl/hir.md) owns the design of the passes.

## Interface

| Type | Purpose |
|------|---------|
| `Hir` | Expression node with kind, span, signal, and a unique `HirId` |
| `HirKind` | Expression variants (literals, control flow, etc.) |
| `Binding` | `u32` index into `BindingArena` — 4 bytes, Copy, identity by integer equality |
| `BindingArena` | Owns all `BindingInner` values for a compilation unit |
| `BindingInner` | Binding metadata: name, scope, and the analysis flags in [arena.rs](arena.rs) |
| `BindingScope` | `Parameter` or `Local` (in `hir::arena`) |
| `HirFragment` | An HIR body closed over its own binding table — portable across arenas, units, and processes (in `hir::fragment`) |
| `CaptureInfo` | What a closure captures and how |
| `CaptureKind` | `Local`, `Capture { index }` (transitive), or `Recursive { binding }` (a self-reference to the enclosing letrec or def binding) |
| `BlockId` | Unique identifier for a block, used by `break` to target the correct block |
| `Analyzer` | Transforms Syntax → HIR; takes `&mut BindingArena` |
| `AnalysisResult` | HIR produced by the analyzer |
| `HirLinter` | HIR-based linter producing Diagnostics (no constructor args) |
| `extract_symbols_from_hir` | Builds SymbolIndex from HIR (3 args: hir, symbols, arena) |

### Analyzer methods

| Method | Purpose |
|--------|---------|
| `analyze(&mut self, syntax: &Syntax) -> Result<AnalysisResult, String>` | Analyze a single Syntax tree into HIR |
| `analyze_file_letrec(&mut self, forms: Vec<FileForm>, span: Span) -> Result<Hir, String>` | Analyze a file's top-level forms, already classified as `Def`, `Var`, `Signal` or `Expr`, as one letrec. Pass 1 pre-binds all names. Pass 2 analyzes initializers in order. Pass 3 re-analyzes lambdas until their signals stop changing. Returns a single `HirKind::Letrec` node. |
| `bind_primitives(&mut self, meta: &PrimitiveMeta)` | Bind every registered primitive as an immutable `Local` binding in the analyzer's initial scope. Call it before analysis. A file-level `def` shadows a primitive. |

## Data flow

```
Syntax (expanded)
    │
    ▼
Analyzer (&mut BindingArena)
    ├─► resolve variables → Binding (u32 index into BindingArena)
    ├─► track mutations → arena.get_mut(b).is_mutated = true
    ├─► track captures → marked by scope lookup + CaptureInfo
    └─► infer signals → Signal
    │
    ▼
HIR passes in regularize (&mut BindingArena)
    │
    ▼
Lowerer (&BindingArena) — read-only access to binding metadata
```

## Dependents

- [lir/lower](../lir/lower/AGENTS.md) - consumes HIR, reads `arena.get(b).needs_capture()` via `&BindingArena`
- [pipeline](../pipeline/AGENTS.md) - orchestrates Syntax → HIR → LIR → Bytecode
- [cli.rs](../lint/cli.rs) - uses `HirLinter` for static analysis
- [state.rs](../lsp/state.rs) - uses `extract_symbols_from_hir` and `HirLinter` for IDE features
- [analysis.rs](../primitives/compile/query/analysis.rs) - uses `HirLinter` and `extract_symbols_from_hir` for `compile/analyze`
- [eval.rs](../vm/eval.rs) - runs the `Analyzer` for `eval`

## Invariants

1. **Every variable reference is a `Binding`.** No symbols in HIR. If you
   see a symbol at this stage, analysis failed.

2. **`Binding` identity is integer equality.** Two references to the same
   binding site have the same `u32` index. `Binding` implements `Hash`/`Eq`
   via the derived `u32` comparison.

3. **`needs_capture()` decides whether a binding gets a capture cell.** A
   local needs one when it is captured and either mutable or prebound. A
   parameter needs one when it is mutated. An immutable, non-prebound
   captured local is captured by value.

4. **Signals combine upward.** A `begin` has the combined signal of its
   children. A `fn` body's signal is stored, but the `fn` node itself is
   Silent. Signal emission uses `HirKind::Emit { signal: SignalBits, value:
   Box<Hir> }`. `yield` is a prelude macro that expands to
   `(emit :yield val)`.

5. **Captures are computed per-fn.** Each `HirKind::Lambda` carries its
   own `Vec<CaptureInfo>` listing what it captures and how.

6. **Empty lists become `HirKind::EmptyList`, not `HirKind::Nil`.** The
   analyzer distinguishes between `nil` (absence) and `()` (empty list).
   Conflating them breaks truthiness semantics.

7. **Binding resolution is scope-aware (hygienic).** See
   [analyze/AGENTS.md](analyze/AGENTS.md) for the subset rule and
   referential transparency.

8. **`HirKind::Define { binding, value }` binds a local.** The lowerer
   allocates its slot before lowering the value, so the value can refer to
   the binding.

9. **The analyzer and the passes in `regularize` write binding metadata;
   the lowerer only reads it.** Both hold `&mut BindingArena`; the lowerer
   holds `&BindingArena`.

10. **`Destructure` decomposes values into pattern bindings.**
    `HirKind::Destructure { pattern, value, strict }` is produced for
    `def`, `var`, `let`, `letrec` and `fn` parameter destructuring. The
    pattern's leaf `Var` bindings are created in the current scope. `let` is
    sequential: a multi-binding `let` becomes nested single-binding lets.
    `let*` is a prelude macro that expands to nested `let`.

11. **Binding forms destructure strictly.** `def`, `var`, `let`, `letrec`,
    required parameters and `&keys` patterns signal `:type-error` on a
    missing element, a missing key, or a wrong type, so a strict
    `Destructure` carries `:error` and adds it to the enclosing function's
    signal. `&opt` and `&named` parameters bind `nil` instead.

12. **Binding forms build `HirPattern::Struct` for `{}` and `@{}`.** The
    lowerer reads entries with `StructGetDestructure` (strict) or
    `StructGetOrNil` (non-strict), and the rest with `StructRest`. `rest` is
    `Some` when the pattern writes `& pat` after its key-pattern pairs. In
    `match`, `{}` builds `Struct` guarded by `IsStruct`, and `@{}` builds
    `Table` guarded by `IsStructMut`; a missing key binds `nil`.

13. **`Block` and `Break` are compile-time control flow.** `HirKind::Block`
    has a `BlockId` and optional name. `HirKind::Break` targets a `BlockId`.
    The analyzer validates: break outside block → error, unknown block name
    → error, break across function boundary → error. The lowerer stores the
    break value into the block's result slot with `StoreLocal`, then jumps to
    the block's exit label. `while` wraps its `While` node in an implicit
    `Block` named `"while"`, so `(break :while val)` or unnamed `(break)` can
    exit a while loop.

14. **`Eval` compiles and executes a datum at runtime.**
    `HirKind::Eval { expr: Box<Hir>, env: Box<Hir> }` is produced for
    `(eval expr)` or `(eval expr env)`. The node's signal is `:error`, and
    the enclosing function's inferred signal includes it: the VM handler
    holds no park of the code it runs, so a yield, an I/O request or a halt
    inside it comes back as an `:eval-error`. Not in tail position. The VM
    handler reaches the symbol table through the driving VM's `symbols_ptr`
    and caches the Expander on the VM.

15. **A docstring is a leading string literal.** `HirKind::Lambda` has a
    `doc: Option<Rc<str>>` field. The analyzer takes a leading string literal
    as the docstring only when the body has two or more forms. The lowerer
    copies it to `LirHead::doc`, and the emitter writes it into the code
    payload, where `ClosureTemplate::doc()` reads it for `(doc name)` and LSP
    hover.

16. **Signal declarations apply to the innermost enclosing function.**
    `(attune! spec)` and `(silent!)` are ceilings, checked against
    `inferred_signals`, the signals a call to the lambda may emit; they never
    replace it. `(bound param spec)` lands on the lambda in `param_bounds:
    Vec<ParamBound>`. A call to a bounded parameter adds the bound's bits, not
    a polymorphic dependency. The function checks the argument against the
    bound on entry (`CheckSignalBound`), and that check may raise, so a lambda
    with a bound carries `:error`. A call whose argument has a known signal is
    checked against the callee's bounds at compile time. `squelch` and
    `attune` are runtime primitives.
    [docs/signals/inference.md](../../docs/signals/inference.md) owns the
    forms.

17. **Set literals become constructor calls.** `|a b|` becomes `(set a b)`
    and `@|a b|` becomes `(@set a b)`. A splice inside a set literal is a
    compile error. All synthesized nodes carry the literal's span.

18. **A lambda's source location is captured for `meta/origin`.**
    `HirKind::Lambda` has an `origin: Option<Span>` field, set in
    `analyze_lambda` from the form's span. The lowerer copies it to
    `LirHead::origin`, and the emitter writes it into the code payload.
    `(meta/origin f)` reads it through `ClosureTemplate::origin()`.

19. **Qualified symbols are desugared to nested `get` calls.**
    `a:b:c` in `SyntaxKind::Symbol` is desugared during analysis to
    `(get (get a :b) :c)`. The first segment is resolved as a variable.
    Subsequent segments become keyword arguments to `get`. This produces
    standard `HirKind::Call` nodes — no special HIR variant. The `get`
    binding always resolves to the primitive. All synthesized nodes carry the
    original symbol's span.

20. **`Parameterize` creates dynamic binding frames.**
    `HirKind::Parameterize { bindings: Vec<(Hir, Hir)>, body: Box<Hir> }`
    is produced for `(parameterize ((p1 v1) (p2 v2) ...) body ...)`. The
    analyzer does not check that each parameter expression is a parameter;
    the VM checks at run time, so the node carries `:error`. The lowerer
    evaluates each pair, emits `PushParamFrame` with them, lowers the body,
    then emits `PopParamFrame`.

21. **Files compile to a single synthetic letrec.** `analyze_file_letrec`
    turns a file's top-level forms into one `HirKind::Letrec`: `def` is an
    immutable binding, `var` a mutable one, `(signal :kw)` and a bare
    expression each a synthetic binding. Pass 1 pre-binds every name, which
    allows mutual recursion. The letrec body is the last binding's name.

22. **Primitives are pre-bound as immutable Local bindings.**
    `bind_primitives` binds every registered primitive in the analyzer's
    initial scope. A file-level `def` shadows a primitive. The lowerer emits
    `LoadConst` for a primitive. Compile-time checks use the `Binding`
    identity: `(assign + 42)` is a compile error.

23. **Tail calls are marked on every `AnalyzeResult`.** `pipeline::analyze`
    and `analyze_file` run `mark_tail_calls` before returning, so `is_tail`
    is a fact on the analyzed tree rather than a default. The linter's
    non-tail-self-recursion rule and the `compile/callees` call graph both
    read it there. `regularize` marks again, because map fusion and the graft
    mint call nodes after that point.

24. **A dead binding takes its initializer with it.** `hir::dead` removes a
    `let`/`letrec` binding with zero uses whose initializer is provably
    effect-free, which deletes the initializer's call. It runs inside
    `regularize`, before `functionalize`, so the region solver never sees the
    deleted call. A silent callee is not enough: silence means no signal
    bits, and `%push-array-mut` is silent and mutates. See
    [docs/impl/hir.md](../../docs/impl/hir.md).

25. **A body that leaves its unit travels as an `HirFragment`.** A `Binding`
    is an index into one arena, so an HIR body alone is meaningless
    elsewhere. `HirFragment::close` renumbers a body's bindings against its
    own table — a `BindingInner` per binding the body introduces, a
    `SymbolId` per free global — and `graft` re-hosts it in any arena.
    Nothing may hoist selected `BindingInner` fields beside a body instead.
    A function's own template carries the fragment of its body, so a later
    unit reaches the body through the function value. See
    [docs/impl/hir.md](../../docs/impl/hir.md).

26. **A call to a known function becomes its body.** The graft replaces a
    call whose callee is an immutable, non-recursive function binding with
    the callee's fragment, at the call's arity: a same-unit function's from
    the tree, another unit's from its value. It records the operand it bound
    to each parameter, and inference joins that record into the parameter
    as it joins a call site, so the definition keeps its proofs. A call in
    a function nothing live reaches stays a call. The graft runs in
    `regularize` after map fusion and before dead-binding elimination, and
    it reads no name. See
    [grafting a call](../../docs/impl/graft.md).

27. **After inference, the fold deletes what the types decide.** A condition,
    a `cond` clause, an `and` or `or` operand, or a `(type-of x)` dispatch
    whose answer the recorded types settle loses its dead side. A raise keeps
    the statements after it, because a resume reaches them, and the operand
    contracts skip a site there. The fold and the inference ascent alternate
    until a fold changes nothing, and the operand contracts run on what the
    last fold left.
