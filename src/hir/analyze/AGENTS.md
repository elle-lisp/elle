# hir/analyze

<!-- audited: 2026-10-05 -->

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
| `SignalSources` | Separates a lambda body's signal sources into parameter calls, direct `emit` bits, and bits from non-parameter callees. `add_inherent_bits` is how a form that may raise reports itself |
| `keyword_collectors` | The bindings whose function collects `&keys` or `&named`, from a lambda initializer or a closure value; a call to one may raise at the call |
| `arity_env` | The arity of each binding whose callee is known: a primitive, a closure value from the stdlib or core, and a binding initialized with a lambda. A call to anything else may raise at the call |
| `ParamBound` | Struct: `{ binding, signal }` — a parameter bound |
| `current_param_bounds` | Maps `Binding` → `Signal` for parameters bounded by `(bound p spec)` in the lambda being analyzed |
| `bound_env` | The parameter bounds of each binding initialized with a lambda that declares one, by parameter position; a call checks an argument whose signal is known against them |
| `current_declared_ceiling` | `Option<Signal>`: the ceiling `(silent!)` or `(attune! …)` declares, the intersection when a body declares several. `analyze_lambda` checks the inferred signal against it and does not replace the signal with it |
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
   A `SymbolId` is the name's hash, so a by-name lookup would find a
   primitive's signal for any binding of the same spelling. A call therefore
   consults this map only when the callee binding `is_primitive`
   (`primitive_signal_of`), which `bind_primitives` also seeds into
   `signal_env`. A same-named *user* binding must never resolve through it,
   or it would inherit the primitive's signal.
3. **Call analysis**: Looks up the callee's signal and propagates it. A
   call to a polymorphic callee that passes a parameter in a propagated
   position adds that parameter to the sources, and still adds the callee's
   own bits.
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

4. **`bound` records a bound for the innermost enclosing function.** It is
   a special form accepted anywhere in a function body. `(bound p spec)`
   bounds parameter `p` to `spec`, a keyword or a literal keyword set, `||`
   included. The name must be a declared parameter, and a second bound on one
   parameter is a compile error. `analyze_lambda` reads the accumulators after
   analyzing the body. The bound is checked on entry, and that check may
   raise, so a lambda with a bound carries `:error`. `analyze_call` also
   checks each argument whose signal it knows against the callee's
   `bound_env` entry, and a violation is a compile error.

5. **A ceiling checks and never replaces.** `(attune! spec)` and
   `(silent!)`, which is `(attune! ||)`, record a ceiling. `analyze_lambda`
   checks the inferred signal against it after the body, and an excess bit or
   a propagated parameter is a compile error.

6. **A construct that can raise carries `:error`, on its node and in the
   enclosing function's sources.** The list is in
   [docs/signals/inference.md](../../../docs/signals/inference.md), "What
   raises". Each site reports through `add_inherent_bits`; a strict
   `Destructure` is built by `destructure`, which reports for it. A call
   reports `:error` unless the compiler checked its argument count against a
   known arity and knows whether the callee collects keywords
   (`analyze_call`).

## When to modify

- **Adding a new special form**: Add one `SpecialForm` entry to
  `forms/registry.rs` and implement its `analyze_*` method
- **Changing binding semantics**: Update `binding.rs` and `destructure.rs`
- **Changing signal inference**: Update `call.rs` and `lambda.rs`
- **Changing signal bounds**: Update `special.rs` for `analyze_bound` and
  `lambda.rs` for the ceiling check, and `call.rs` for the check at a call;
  the entry check is emitted in
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
  reach the enclosing function: report the bits through `add_inherent_bits`
  too, as `analyze_match` and `analyze_eval` do.
- **Breaking scope hygiene**: When creating synthetic bindings, use the correct scope set from the original Syntax node
- **Forgetting to include bounded parameter bits in inferred_signals**: When a parameter has a `bound`, its bits must be included in the lambda's `inferred_signals`, not tracked as polymorphic.
