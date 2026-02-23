# Hygienic Macros: Implementation Plan

Reference: docs/MACROS.md

Four PRs, each independently shippable.

---

## PR 1: Dead Code Cleanup ✓

### Changes (completed)

**src/symbol.rs**
- Remove `MacroDef` struct (lines 16-21)
- Remove `gensym_id()` function and `GENSYM_COUNTER` static (lines 6-13)
- Remove `macros` field from `SymbolTable` (line 40)
- Remove `define_macro()`, `get_macro()`, `is_macro()` methods (lines 82-96)

**src/primitives/meta.rs**
- Remove `prim_is_macro` (lines 49-65) — never registered, duplicate of macros.rs
- Remove `prim_expand_macro` (lines 26-46) — never registered, duplicate of macros.rs
- Keep `prim_gensym` (registered at registration.rs:743, may have runtime consumers)

**src/primitives/macros.rs**
- Remove `prim_is_macro` and `prim_expand_macro` — registered but useless stubs
  (always return false / passthrough)

**src/primitives/registration.rs**
- Remove `use super::macros::{prim_expand_macro, prim_is_macro}` (line 46)
- Remove registration of `expand-macro` (line 789) and `macro?` (line 797)
- Keep `use super::meta::prim_gensym` (line 51) and its registration (line 743)

**Verification**: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`

---

## PR 2: Tier 1 — Compile-Time Gensym

### Design

The `(let ((tmp (gensym))) ...)` pattern requires evaluating `let` at
expansion time, which the Expander can't do. Instead, we add
`with-gensyms` — a special form recognized by `handle_defmacro` that
declares names to be auto-gensymed at each expansion:

```lisp
(defmacro swap (a b)
  (with-gensyms (tmp)
    `(let ((,tmp ,a)) (set! ,a ,b) (set! ,b ,tmp))))
```

Gensym names are treated as additional macro parameters that are
automatically filled with fresh symbols at each expansion. The existing
substitution machinery handles them with zero changes.

Tier 3's procedural evaluator will make `with-gensyms` unnecessary
(macros can use `let` + `gensym` directly). Existing `with-gensyms`
macros will be migrated at that point — no backward-compatibility
machinery.

### Step 1: Add gensym to the Expander

**src/syntax/expand/mod.rs**
- Add `gensym_counter: u32` field to `Expander`
- Initialize to 0 in `new()`
- Add method: `fn gensym(&mut self, prefix: &str) -> String` returning
  `format!("__G{}{}", prefix, id)` with incrementing counter

### Step 2: Add `with-gensyms` to MacroDef

**src/syntax/expand/mod.rs**

```rust
pub struct MacroDef {
    pub name: String,
    pub params: Vec<String>,
    pub gensyms: Vec<String>,  // NEW
    pub template: Syntax,
    pub definition_scope: ScopeId,
}
```

In `handle_defmacro`: when body is `(with-gensyms (names...) template)`,
extract gensym names and store the inner template. Otherwise `gensyms`
is empty (backward compatible).

### Step 3: Generate gensyms during expansion

**src/syntax/expand/macro_expand.rs** — in `expand_macro_call`:

After arity check, before substitution:
```rust
let mut gensym_args: Vec<Syntax> = Vec::new();
for prefix in &macro_def.gensyms {
    let name = self.gensym(prefix);
    gensym_args.push(Syntax::new(SyntaxKind::Symbol(name), call_site.span.clone()));
}

let mut all_params = macro_def.params.clone();
all_params.extend(macro_def.gensyms.iter().cloned());
let mut all_args: Vec<Syntax> = args.to_vec();
all_args.extend(gensym_args);

let substituted = self.substitute(&macro_def.template, &all_params, &all_args);
```

### Step 4: Handle standalone `(gensym)` in expand()

**src/syntax/expand/mod.rs** — in `expand()`, add `gensym` to the
precedence chain (after `expand-macro`, before `define` shorthand).
Produces a fresh symbol at expansion time. Limited utility until Tier 3
but harmless and useful for testing.

### Step 5: REPL Expander persistence

**src/pipeline.rs** — change `compile_new` to accept an optional
`&mut Expander` parameter. When provided, use it instead of creating a
fresh one. The REPL (`main.rs`, `run_repl`) holds a persistent `Expander`
and passes it to `compile_new` for each input. `eval_new` delegates to
`compile_new`, so it gets the same treatment. File execution via
`compile_all_new` already shares an Expander across forms — no change
needed there.

### Step 6: Tests

**src/syntax/expand/tests.rs**:
- `test_gensym_unique`: two calls produce different names
- `test_gensym_with_prefix`: prefix appears in name
- `test_defmacro_with_gensyms`: `with-gensyms` stores and expands correctly
- `test_gensyms_fresh_per_expansion`: two expansions produce different gensyms

**tests/integration/pipeline_point.rs**:
- `test_macro_swap_with_gensym`: swap doesn't capture caller's `tmp`
- Counterfactual: remove `with-gensyms`, verify capture occurs

### Step 7: Update docs

- `src/syntax/AGENTS.md` — document `with-gensyms`
- `docs/MACROS.md` — update Tier 1 with actual syntax
- `docs/DEBUGGING.md` — remove "blocked" note from bench macro

---

## PR 3: Tier 2 — Sets-of-Scopes Hygiene

### Step 1: Fix scope stamping in the Expander

**src/syntax/expand/macro_expand.rs**

Scope the template BEFORE substitution, not after. Arguments from the
call site replace scoped template nodes with their own unscoped versions:

```rust
pub(super) fn expand_macro_call(...) -> Result<Syntax, String> {
    // ... arity check, gensym generation ...

    let intro_scope = self.fresh_scope();

    // Scope template BEFORE substitution
    let scoped_template = self.add_scope_recursive(macro_def.template.clone(), intro_scope);

    // Substitute — call-site args replace scoped nodes, keeping their own scopes
    let substituted = self.substitute(&scoped_template, &all_params, &all_args);

    // Quasiquote evaluation
    let resolved = match &substituted.kind {
        SyntaxKind::Quasiquote(inner) => self.eval_quasiquote_to_syntax(inner)?,
        _ => substituted,
    };

    // No post-substitution scope stamping — already done
    self.expand(resolved)
}
```

**Why this works**: `substitute_quasiquote` wraps substituted args in
`Unquote(...)` with `template.scopes.clone()` (which now includes intro
scope), but `eval_quasiquote_to_syntax` discards the Unquote wrapper and
returns the inner arg with its original scopes. Template-originated
symbols (like `let`, `set!`, `tmp`) keep the intro scope.

### Step 2: Thread scope sets into the Analyzer

**src/hir/analyze/mod.rs**

New types:
```rust
struct ScopedBinding {
    scopes: Vec<ScopeId>,
    id: BindingId,
}

struct Scope {
    bindings: HashMap<String, Vec<ScopedBinding>>,
    is_function: bool,
    next_local: u16,
}
```

Change `bind()` signature:
```rust
fn bind(&mut self, name: &str, scopes: &[ScopeId], kind: BindingKind) -> BindingId
```

Change `lookup()` signature and algorithm:
```rust
fn lookup(&mut self, name: &str, ref_scopes: &[ScopeId]) -> Option<BindingId>
```

The algorithm:
1. Walk scopes innermost to outermost, tracking `crossed_function_boundary`
2. For each scope, find all candidates with matching name where
   `is_subset(candidate.scopes, ref_scopes)` is true
3. Track the best match (largest scope set) along with its depth and
   whether it crossed a function boundary
4. After the scope walk, apply capture logic to the **winning** binding
   based on **its** position relative to function boundaries

Helper:
```rust
fn is_subset(subset: &[ScopeId], superset: &[ScopeId]) -> bool {
    subset.iter().all(|s| superset.contains(s))
}
```

**Backward compatibility**: empty scopes `[]` is a subset of everything,
so pre-expansion code (empty scopes) works identically to before.

Change `lookup_in_current_scope()`:
```rust
fn lookup_in_current_scope(&self, name: &str, ref_scopes: &[ScopeId]) -> Option<BindingId>
```
Find the best scope-compatible match in the current scope only.

Change `is_binding_in_current_scope()`: no change needed — searches by
`BindingId`, not by name.

### Step 3: Update parent_captures lookup path

**src/hir/analyze/mod.rs** — lines 293-332

The parent_captures path currently matches by `SymbolId`. For Tier 2,
it needs scope-aware matching. Change to compare both name AND scope
sets. The `BindingInfo` struct may need a `scopes: Vec<ScopeId>` field,
or the scope set can be stored alongside the capture info.

### Step 4: Pass scope sets through ALL bind/lookup call sites

Complete list of call sites (verified against codebase):

**`bind()` call sites** — all need `scopes` parameter added:

| File | Function | Line | Source of scopes |
|------|----------|------|-----------------|
| `forms.rs` | `analyze_begin` (two-pass) | 163 | Need to extract from define's name syntax node. Change `is_define_form` to return `(&str, &[ScopeId])` |
| `forms.rs` | `analyze_for` (keyword `each`) | 259 | `items[1].scopes` (loop variable) |
| `binding.rs` | `analyze_let` | 41 | `pair[0].scopes` |
| `binding.rs` | `analyze_let_star` | 106 | `pair[0].scopes` |
| `binding.rs` | `analyze_letrec` | 164 | `pair[0].scopes` |
| `binding.rs` | `analyze_define` (local) | 256 | `items[1].scopes` |
| `binding.rs` | `analyze_define` (global) | 288 | `&[]` (globals resolve by symbol name at runtime, scopes irrelevant) |
| `binding.rs` | `analyze_lambda` | 391 | `param.scopes` |
| `special.rs` | `analyze_pattern` | 79 | `syntax.scopes` (pattern variable) |

**`lookup()` call sites** — all need `ref_scopes` parameter added:

| File | Function | Line | Source of scopes |
|------|----------|------|-----------------|
| `forms.rs` | `analyze_expr` (Symbol) | 24 | `syntax.scopes` |
| `binding.rs` | `analyze_define` | 251 | `items[1].scopes` (via `lookup_in_current_scope`) |
| `binding.rs` | `analyze_set` | 330 | `items[1].scopes` |

### Step 5: Set `definition_scope` correctly

**src/syntax/expand/mod.rs** — in `handle_defmacro`:
- Track a `current_expansion_scope: ScopeId` on the Expander (default 0)
- Set `definition_scope` to the current expansion scope
- During expansion, add `definition_scope` to free variables in the
  template (variables that are NOT parameters and NOT gensyms). This
  makes them resolve in the macro's definition environment.

Note: full referential transparency is complex. For the initial Tier 2
implementation, defer this to a follow-up. The scope-stamping fix alone
provides the critical "no accidental capture" property. Referential
transparency can be added incrementally.

### Step 6: Tests

**New test file**: `tests/integration/hygiene.rs`

Core hygiene tests:
- `test_macro_no_capture`: macro `tmp` doesn't shadow caller's `tmp`
- `test_macro_no_leak`: caller can't see macro's internal bindings
- `test_nested_macro_hygiene`: nested expansions with overlapping names
- `test_backward_compat_no_macros`: non-macro code works identically
- `test_backward_compat_existing_macros`: existing macros still work

Capture interaction tests (highest risk area):
- `test_macro_closure_captures_callsite`: macro-generated closure captures
  a call-site variable correctly
- `test_macro_closure_across_boundary`: macro code inside a closure that
  captures across a function boundary
- `test_set_through_macro`: `set!` on call-site variable works in macro
- `test_nested_closure_macro`: nested macro expansions generating closures

LSP/linter smoke tests:
- `test_analyze_all_with_macros`: `analyze_all_new` produces correct
  bindings for macro-expanded code
- Verify `elle-lint` and `elle-lsp` still build and pass their tests

Counterfactual: revert scope-before-substitution, verify capture occurs

### Step 7: Update docs

- `src/hir/AGENTS.md` — document scope-aware lookup
- `src/syntax/AGENTS.md` — fix hygiene section (now accurate)
- `docs/MACROS.md` — update Tier 2 as implemented

---

## PR 4: Tier 3 — Procedural Macros

### Step 1: Create SyntaxEvaluator

**src/syntax/expand/eval.rs** (new file, ~300 lines)

Tree-walking interpreter over `Syntax`. Environment is
`HashMap<String, Syntax>`. Takes `&mut Expander` for `gensym()` access.

Supports: literals, symbols (env lookup), `if`, `let`, `begin`, `fn`
(closures), application, `quote`, `quasiquote`/`unquote`, `gensym`,
`cons`/`car`/`cdr`/`list`/`append`, `empty?`/`first`/`rest`,
`symbol?`/`list?`/`string?`, `=`, `error`.

Returns `Result<Syntax, String>` with source location from the macro
definition for error messages.

### Step 2: Change expand_macro_call to use the evaluator

**src/syntax/expand/macro_expand.rs**

Replace template substitution with evaluation:
```rust
let mut env = HashMap::new();
for (param, arg) in macro_def.params.iter().zip(args) {
    env.insert(param.clone(), arg.clone());
}
let result = self.eval_syntax(&macro_def.template, &mut env)?;
```

Backward compatibility: quasiquote bodies evaluate identically to the
old substitution path — unquote looks up names in the environment.

### Step 3: Migrate `with-gensyms` macros

With the evaluator, macros can use `(let ((tmp (gensym "tmp"))) ...)`
directly. Migrate existing `with-gensyms` macros to use `let` + `gensym`.
Remove `MacroDef.gensyms` field. Per AGENTS.md: no backward-compatibility
machinery.

### Step 4: Add variadic macro parameters

Extend `MacroDef` with `rest_param: Option<String>`. Detect dotted-pair
syntax in parameter list. Bind excess arguments as a Syntax list.

### Step 5: Tests

- Procedural macro with `if`, recursion, list processing
- Gensym in procedural body
- Error handling (bad macro body → clear error with location)
- Variadic macros
- Backward compat: existing template macros still work

### Step 6: Implement key macros

**lib/macros.lisp** (loaded by stdlib):
- `try`/`catch` — fiber-based exception handling (see docs/EXCEPT.md)
- `defer` — cleanup on scope exit
- `bench` — timing macro (see docs/DEBUGGING.md)

### Step 7: Update docs

- `docs/MACROS.md` — update Tier 3 as implemented
- `docs/EXCEPT.md` — update try/catch as implemented
- `docs/DEBUGGING.md` — update bench as implemented
- `src/syntax/AGENTS.md` — document SyntaxEvaluator

---

## Risk Assessment

**Highest risk: PR 3 (Tier 2) `lookup()` rewrite.** This is the most
complex function in the Analyzer (~110 lines of capture tracking, function
boundary detection, transitive capture resolution). The scope-aware
matching adds a new dimension. Write capture-interaction tests FIRST.

**Second risk: PR 3 scope stamping in quasiquote path.** Three interacting
functions (`substitute`, `substitute_quasiquote`, `eval_quasiquote_to_syntax`)
with scopes flowing through all three. The `template.scopes.clone()` calls
in wrapper node construction could carry intro scopes to wrong places.
Test `UnquoteSplicing` specifically.

**Lower risk: PR 2 (Tier 1).** Additive change, no existing behavior
modified. The `with-gensyms` mechanism is clean (gensyms as auto-filled
parameters).

**Lowest risk: PR 1 (cleanup).** Removing unused code. Full test suite
catches any accidental dependency.
