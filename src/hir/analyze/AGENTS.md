# hir/analyze

<!-- audited: 2026-09-23 -->

Syntax to HIR analysis: binding resolution, capture computation, signal inference, and linting.

## Responsibility

Transform expanded Syntax trees into HIR by:
1. Resolving all variable references to `Binding` (u32 arena indices)
2. Computing closure captures and capture cells
3. Inferring signals, across function boundaries within a unit
4. Validating scope rules and control flow
5. Extracting docstrings from function bodies

Does NOT:
- Generate code (that's `lir`)
- Execute anything (that's `vm`)
- Parse source (that's `reader` and `syntax`)

The HIR shape and its invariants belong to [../AGENTS.md](../AGENTS.md); this
file covers how the analyzer produces it.

## Key types

| Type | Purpose |
|------|---------|
| `Analyzer` | Main struct that transforms Syntax → HIR |
| `BlockContext` | Active block for `break` targeting (block_id, name, fn_depth) |
| `SignalSources` | Separates a lambda body's signal sources into parameter calls, direct `emit` bits, and bits from non-parameter callees |
| `ParamBound` | Struct: `{ binding, signal }` — a parameter bound |
| `current_param_bounds` | Maps `Binding` → `Signal` for parameters bounded by `(silence p)` in the lambda being analyzed |
| `current_declared_ceiling` | `Option<Signal>`: the function-level ceiling set by `(silence)` or `(attune! …)` |
| `ScopedBinding` | Binding with its scope set for hygienic resolution |
| `Scope` | Lexical scope with bindings HashMap and local index tracking |

## Data flow

```
Syntax (expanded, with scope sets)
    │
    ▼
Analyzer (&mut BindingArena)
    ├─► resolve variables → Binding (u32 index into BindingArena)
    ├─► track mutations → arena.get_mut(b).is_mutated = true
    ├─► track captures → scope lookup marks them + CaptureInfo
    ├─► infer signals → Signal { bits, propagates }
    ├─► validate scope rules (hygienic resolution)
    ├─► validate control flow (break targeting)
    └─► extract docstrings → Option<Rc<str>>
    │
    ▼
HIR (binding indices are inline — metadata lives in BindingArena)
```

## Interprocedural signal tracking

The analyzer tracks signals across function boundaries (see
[signals/AGENTS.md](../../signals/AGENTS.md) for signal type definitions):

1. **Signal environment**: `signal_env` maps `Binding` → `Signal` for
   locally-defined functions. Top-level defines are file-letrec bindings,
   tracked the same way.
2. **Primitive signals**: Maps `SymbolId` → `Signal` for built-in functions.
   Keyed by the `CompileCtx` setup table's ids, which agree with this
   analyzer's table only on the shared primitive prefix — so a call only
   consults this map when the callee binding `is_primitive`
   (`primitive_signal_of`), which `bind_primitives` also seeds into
   `signal_env`. A same-named *user* binding must never resolve through it
   (a colliding id would hand it an unrelated global's signal).
3. **Call analysis**: Looks up the callee's signal and propagates it.
4. **Mutation invalidation**: `assign` clears the mutated binding's
   `signal_env` entry. This is sound only because the fallback in (2) is
   gated to primitives: a reassigned non-primitive local therefore resolves
   to `Signal::unknown()`, never a stale or same-named global's signal.

## Scope-aware binding resolution

Bindings are resolved using **hygienic scope sets**:

- Each reference carries the scope set of its Syntax node
- Each binding definition carries a `Vec<ScopeId>` from the binding site
- A binding is visible if its scope set is a **subset** of the reference's scope set
- When multiple bindings match, the one with the **largest scope set** wins (most specific)
- Empty scopes `[]` is a subset of everything, so pre-expansion code works identically
- **Referential transparency** ([docs/macros.md](../../../docs/macros.md),
  "Hygiene: sets of scopes"): outside a definition-environment frame (the
  global frame, a file's top-level letrec frame), a binding is visible to a
  reference only if every INTRO scope the reference carries
  (`ScopeId::is_intro`) is on the binding or in the frame's expansion
  provenance (`Scope::intro_provenance` — the intro scopes of the form that
  opened the frame). A call-site shadow therefore cannot capture a macro
  template's free variable; resolution falls through to top level.

This prevents accidental capture in macros while allowing intentional capture via `datum->syntax`.

## Invariants

1. **`letrec*` contexts reject duplicate definitions by binding identity.**
   Explicit `letrec` and fn-body `begin` (the two-pass `in_function` branch)
   check each definition through `DuplicateGuard` (`analyze/scopes.rs`),
   keyed `(SymbolId, sorted scope set)` — the same identity `bind()` records
   — so a macro-template binder and a user binder of one spelling coexist
   while a true re-definition is a compile error (`Err(String)`). `letrec`
   binders are bound with their name syntax's scopes (hygienic, like
   `fileletrec` and `analyze_begin`). See [docs/bindings.md](../../../docs/bindings.md),
   "Duplicates are judged by binding identity" and "Function bodies are an
   implicit letrec".

2. **`letrec*` contexts reject use before initialization.** Prebound
   bindings carry `init_pending`/`prebind_fn_depth` (`BindingInner`), set in
   letrec/begin Pass 1 and cleared after each initializer is analyzed. The
   Symbol arm of `analyze_expr` errors (`'{name}' referenced before its
   initialization`, `Err(String)`) on a value read of a pending binding at
   the SAME `fn_depth`; a read inside a lambda (deeper depth) is the legal
   deferred forward reference. File top level (`fileletrec`) does not check
   use before initialization, and such a read yields nil (#1244). See
   [docs/bindings.md](../../../docs/bindings.md), "Use before
   initialization is an error".

3. **The `(doc name)` form rewrites by what the symbol resolves to.** The
   `doc` arm in `forms/expr.rs` passes a symbol that resolves to a closure
   (user-defined or stdlib) through as the closure, so `prim_doc` can read
   its docstring. A NativeFn, a Parameter, or an unresolved symbol (a
   special form) is rewritten to `(doc "name")` for `vm.docs` lookup. The
   critical field is `primitive_values: HashMap<Binding, Value>` on
   `Analyzer`.

4. **`silence` records a bound for the innermost enclosing function.** It is
   a special form accepted anywhere in a function body. `(silence)` sets the
   function's ceiling to empty; `(silence p)` bounds parameter `p`. Signal
   keywords are rejected: `(silence :kw ...)` and `(silence p :kw ...)` are
   compile errors. The name must be a declared parameter. For a duplicate
   bound on the same parameter, the last one wins. `analyze_lambda` reads the
   accumulators after analyzing the body and checks the ceiling there. No
   call site is checked at compile time; the bound is checked on entry.

## When to modify

- **Adding a new special form**: Add one `SpecialForm` entry to
  `forms/registry.rs` and implement its `analyze_*` method
- **Changing binding semantics**: Update `binding.rs` and `destructure.rs`
- **Changing signal inference**: Update `call.rs` and `lambda.rs`
- **Changing signal bounds**: Update `special.rs` for `analyze_silence` and
  `lambda.rs` for the ceiling check; the entry check is emitted in
  `lir/lower/lambda/body.rs` (`CheckSignalBound`)
- **Changing pattern matching**: Update `special/pattern.rs` and `destructure.rs`
- **Changing scope resolution**: Update `scopes.rs` (`lookup()` and `bind()`)
- **Changing `(doc name)` resolution**: The `doc` arm in `forms/expr.rs`
  (see invariant 3)

## Common pitfalls

- **Marking captures by hand**: Resolve names through `lookup`, which marks
  sibling captures. Do not call `mark_captured` for a self-reference; a
  `Recursive` capture must not mark.
- **Forgetting to mark mutations**: If a binding is assigned via `assign`, set `arena.get_mut(b).is_mutated = true`
- **Conflating nil and empty list**: Use `HirKind::EmptyList` for `()`, not `HirKind::Nil`
- **Not propagating signals**: When combining sub-expressions, use
  `signal.combine()` to merge signals upward. A node's signal alone does not
  reach the enclosing function: add the bits to `current_signal_sources`
  too, as `analyze_match` does (#1243).
- **Breaking scope hygiene**: When creating synthetic bindings, use the correct scope set from the original Syntax node
- **Forgetting to include bounded parameter bits in inferred_signals**: When a parameter has a `silence` bound, its bits must be included in the lambda's `inferred_signals`, not tracked as polymorphic.
