# hir/analyze

Syntax to HIR analysis: binding resolution, capture computation, effect inference, and linting.

## Responsibility

Transform expanded Syntax trees into HIR by:
1. Resolving all variable references to `Binding` (NaN-boxed heap objects)
2. Computing closure captures and lbox requirements
3. Inferring effects (including interprocedural effect tracking)
4. Validating scope rules and control flow
5. Extracting docstrings from function bodies

Does NOT:
- Generate code (that's `lir`)
- Execute anything (that's `vm`)
- Parse source (that's `reader` and `syntax`)

## Key types

| Type | Purpose |
|------|---------|
| `Analyzer` | Main struct that transforms Syntax → HIR |
| `Binding` | NaN-boxed Value wrapping `HeapObject::Binding(RefCell<BindingInner>)` — Copy, identity by bit-pattern |
| `BindingScope` | `Parameter`, `Local`, or `Global` |
| `CaptureInfo` | What a closure captures and how (`Local`, `Capture`, or `Global`) |
| `BlockContext` | Active block for `break` targeting (block_id, name, fn_depth) |
| `EffectSources` | Tracks Yields sources within a lambda body for polymorphic inference |
| `current_param_bounds` | Maps `Binding` → `Effects` for parameters with declared bounds (during lambda analysis) |
| `param_bounds_env` | Maps `Binding` → `Vec<(usize, Effects)>` for call-site checking of bounded parameters |
| `ScopedBinding` | Binding with its scope set for hygienic resolution |
| `Scope` | Lexical scope with bindings HashMap and local index tracking |

## Data flow

```
Syntax (expanded, with scope sets)
    │
    ▼
Analyzer
    ├─► resolve variables → Binding (heap-allocated, shared by reference)
    ├─► track mutations → binding.mark_mutated()
    ├─► track captures → binding.mark_captured() + CaptureInfo
    ├─► infer effects → Effect (Inert, Yields, Polymorphic)
    ├─► validate scope rules (hygienic resolution)
    ├─► validate control flow (break targeting)
    └─► extract docstrings → Option<Value>
    │
    ▼
HIR (bindings are inline — no separate HashMap)
```

## Interprocedural effect tracking

The analyzer tracks effects across function boundaries:

1. **Effect environment**: Maps `Binding` → `Effect` for locally-defined functions
2. **Global effects**: Maps `SymbolId` → `Effect` for top-level defines (from previous forms)
3. **Primitive effects**: Maps `SymbolId` → `Effect` for built-in functions
4. **Call analysis**: When analyzing a call, looks up the callee's effect and propagates it
5. **Polymorphic effects**: For higher-order functions like `map`, examines the argument's effect
6. **Mutation invalidation**: `set!` clears the effect tracking for the mutated binding

## Scope-aware binding resolution

Bindings are resolved using **hygienic scope sets**:

- Each binding carries a `Vec<ScopeId>` from the Syntax node
- Each binding definition carries a `Vec<ScopeId>` from the binding site
- A binding is visible if its scope set is a **subset** of the reference's scope set
- When multiple bindings match, the one with the **largest scope set** wins (most specific)
- Empty scopes `[]` is a subset of everything, so pre-expansion code works identically

This prevents accidental capture in macros while allowing intentional capture via `datum->syntax`.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~620 | `Analyzer` struct, scope management, entry point, binding resolution |
| `forms.rs` | ~725 | Core form analysis: `analyze_expr`, literals, operators, collections |
| `binding.rs` | ~530 | Binding forms: `let`, `letrec`, `def`/`var`, `set!` |
| `fileletrec.rs` | ~360 | File-scope letrec compilation for top-level forms |
| `destructure.rs` | ~415 | Destructuring pattern analysis, define-form detection, rest-pattern splitting |
| `lambda.rs` | ~160 | Lambda/fn analysis with captures, params, effects, docstrings |
| `special.rs` | ~345 | Special forms: `match`, `yield`, pattern matching |
| `call.rs` | ~200 | Call analysis and effect tracking |

## Invariants

1. **Every variable reference is a `Binding`.** No symbols in HIR. If you see a symbol at this stage, analysis failed.

2. **`Binding` identity is bit-pattern equality.** Two references to the same binding site share the same NaN-boxed pointer. `Binding` implements `Hash`/`Eq` via `Value::to_bits()`.

3. **`needs_lbox()` determines lbox boxing.** A local binding needs an lbox if captured. A parameter needs an lbox if mutated. Globals never need lboxes.

4. **Effects combine upward.** A `begin` has the combined effect of its children. A `fn` body's effect is stored but the fn itself is Inert.

5. **Captures are computed per-fn.** Each `HirKind::Lambda` carries its own `Vec<CaptureInfo>` listing what it captures and how.

6. **Empty lists become `HirKind::EmptyList`, not `HirKind::Nil`.** The analyzer distinguishes between `nil` (absence) and `()` (empty list). Conflating them breaks truthiness semantics.

7. **Binding resolution is scope-aware (hygienic).** `bind()` stores a `Vec<ScopeId>` alongside each binding. `lookup()` uses subset matching: a binding is visible to a reference if the binding's scope set is a subset of the reference's scope set. When multiple bindings match, the one with the largest scope set wins (most specific). Empty scopes `[]` is a subset of everything, so pre-expansion code works identically.

8. **`Define` and `LocalDefine` are unified.** There is a single `HirKind::Define { binding, value }`. The lowerer checks `binding.is_global()` to decide between global and local define semantics.

9. **Binding metadata is mutable during analysis, read-only after.** The analyzer calls `mark_mutated()`, `mark_captured()`, `mark_immutable()`. The lowerer only reads via `needs_lbox()`, `is_global()`, `name()`, etc.

10. **`Destructure` decomposes values into pattern bindings.** `HirKind::Destructure { pattern: HirPattern, value: Box<Hir> }` is produced by the analyzer for `def`, `var`, `let`, and `fn` parameter destructuring. The pattern's leaf `Var` bindings are created in the current scope. `let*` is desugared to nested `let` in the expander, so the analyzer never sees `let*`.

11. **Destructured bindings use silent nil semantics.** Missing list/@array/@struct elements produce `nil`, not errors. Wrong-type values produce `nil` for all bindings. No runtime type checks.

12. **`HirPattern::Table` supports @struct/struct destructuring.** `HirPattern::Table { entries: Vec<(PatternKey, HirPattern)> }` maps keyword or symbol keys to sub-patterns. `PatternKey::Keyword(String)` for `:foo` keys, `PatternKey::Symbol(SymbolId)` for `'foo` keys. In binding forms (`def`, `var`, `let`, `fn` params), uses `TableGetOrNil` with silent nil. In `match` patterns, emits an `IsStructMut` type guard first so non-@struct values fall through to the next arm.

13. **`Block` and `Break` are compile-time control flow.** `HirKind::Block` has a `BlockId` and optional name. `HirKind::Break` targets a `BlockId`. The analyzer validates: break outside block → error, unknown block name → error, break across function boundary → error. The lowerer compiles break to `Move` + `Jump` — no new bytecode instructions needed. `while` wraps its `While` node in an implicit `Block` named `"while"`, so `(break :while val)` or unnamed `(break)` can exit a while loop.

14. **`Eval` compiles and executes a datum at runtime.** `HirKind::Eval { expr: Box<Hir>, env: Box<Hir> }` is produced by the analyzer for `(eval expr)` or `(eval expr env)`. The effect is always `Yields` (conservative — eval'd code can do anything). Not in tail position. The VM handler accesses the symbol table via thread-local context and caches the Expander on the VM for reuse.

15. **Docstrings are extracted from leading string literals.** `HirKind::Lambda` has a `doc: Option<Value>` field. The analyzer extracts the first string literal in a function body and stores it in `doc`. This field is threaded through LIR into `Closure.doc` and used by the `(doc name)` primitive and LSP hover.

16. **Effect bounds are parsed from `restrict` preambles.** After docstring extraction and before body analysis, the analyzer scans for `restrict` forms in the lambda body preamble. `(restrict)` declares the function is inert. `(restrict :kw ...)` declares the function may emit only these signals. `(restrict param)` declares the parameter must be inert. `(restrict param :kw ...)` declares the parameter may emit at most these signals. Multiple `restrict` forms are allowed (one per parameter + one function-level). Keywords must be registered in the global signal registry. Parameter names must match declared parameters. Duplicate restrictions for the same parameter or function-level are compile-time errors. The first non-restrict form ends the preamble.

17. **Qualified symbols are desugared to nested `get` calls.** `a:b:c` in `SyntaxKind::Symbol` is desugared during analysis to `(get (get a :b) :c)`. The first segment is resolved as a variable (local or global). Subsequent segments become keyword arguments to `get`. This produces standard `HirKind::Call` nodes — no special HIR variant. The `get` binding always resolves to the global primitive, matching the pattern used for array/@array/struct/@struct literal desugaring. All synthesized nodes carry the original symbol's span.

18. **`Parameterize` creates dynamic binding frames.** `HirKind::Parameterize { bindings: Vec<(Hir, Hir)>, body: Box<Hir> }` is produced by the analyzer for `(parameterize ((p1 v1) (p2 v2) ...) body ...)`. Each binding is a (parameter, value) pair. The analyzer validates that each parameter expression is a parameter (or will be at runtime). The lowerer emits `PushParamFrame` before evaluating bindings, stores them in the frame, then emits `PopParamFrame` after the body.

## When to modify

- **Adding a new special form**: Add a case in `forms.rs::analyze_expr`, implement `analyze_your_form` method
- **Changing binding semantics**: Update `binding.rs` and `destructure.rs`
- **Changing effect inference**: Update `call.rs` and `lambda.rs`
- **Changing effect bounds**: Update `lambda.rs::parse_restrict_preamble()` and `call.rs` for call-site checking
- **Changing pattern matching**: Update `special.rs` and `destructure.rs`
- **Changing scope resolution**: Update `mod.rs::lookup()` and `bind()`

## Common pitfalls

- **Forgetting to mark captures**: If a binding is referenced in a nested lambda, call `binding.mark_captured()` during analysis
- **Forgetting to mark mutations**: If a binding is assigned via `set!`, call `binding.mark_mutated()`
- **Conflating nil and empty list**: Use `HirKind::EmptyList` for `()`, not `HirKind::Nil`
- **Not propagating effects**: When combining sub-expressions, use `effect.combine()` to merge effects upward
- **Breaking scope hygiene**: When creating synthetic bindings, use the correct scope set from the original Syntax node
- **Forgetting to include bounded parameter bits in inferred_effect**: When a parameter has a `restrict` bound, its bits must be included in the lambda's `inferred_effect`, not tracked as polymorphic
- **Not checking effect bounds at call sites**: When a concrete function is passed to a parameter with a bound, the analyzer must check the argument's effect against the bound
