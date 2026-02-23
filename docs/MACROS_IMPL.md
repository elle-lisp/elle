# Hygienic Macros: Implementation Plan

Reference: docs/MACROS.md

Three PRs total. PR 1 (dead code cleanup) and PR 2 (VM-based expansion)
are complete. PR 3 (hygiene) remains.

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

## PR 2: VM-Based Macro Expansion ✓

### Changes (completed)

Macro bodies are now compiled and executed in the real VM during
expansion via `pipeline::eval_syntax()`. Template substitution
(`substitute()`, `eval_quasiquote_to_syntax()`) is deleted.

Key implementation details:
- `Expander::expand()` takes `&mut SymbolTable` and `&mut VM`
- `expand_macro_call()` builds `(let ((p1 'a1) ...) body)` and evals it
- Recursion guard: `MAX_MACRO_EXPANSION_DEPTH = 200` (matching Janet)
- `compile`/`compile_all` create internal VMs (no VM in public API)
- `analyze`/`analyze_all` take `&mut VM` parameter
- `eval` shares caller's VM for expansion and execution
- Result `Value` → `Syntax` via `from_value()`, then intro scope added

### Deferred to follow-up PRs

- **Variadic macro parameters** (`rest_param` on `MacroDef`)
- **REPL Expander persistence** (optional `&mut Expander` on pipeline fns)
- **Key macros** (`try`/`catch`, `defer`, `with`, `bench`)
- **gensym returns string not symbol** (#306)

---

## PR 3: Sets-of-Scopes Hygiene

### Design

With VM-based expansion, gensym provides manual hygiene. This PR adds
automatic hygiene via scope-aware binding resolution. Macro-introduced
bindings can't capture call-site names and vice versa.

### Step 1: Fix scope stamping

The intro scope must be added to the macro result *after* VM evaluation
converts the Value back to Syntax. Step 5 of `expand_macro_call` above
already does this via `add_scope_recursive`. Call-site arguments (which
were quoted data during evaluation) get reconstructed from Values without
the intro scope — they keep their original (empty) scopes.

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

**Pre-expansion code**: empty scopes `[]` is a subset of everything,
so code that hasn't been through macro expansion works identically.

### Step 3: Pass scope sets through ALL bind/lookup call sites

**`bind()` call sites** — all need `scopes` parameter added:

| File | Function | Source of scopes |
|------|----------|-----------------|
| `forms.rs` | `analyze_begin` (two-pass) | define's name syntax node |
| `forms.rs` | `analyze_for` | `items[1].scopes` (loop variable) |
| `binding.rs` | `analyze_let` | `pair[0].scopes` |
| `binding.rs` | `analyze_let_star` | `pair[0].scopes` |
| `binding.rs` | `analyze_letrec` | `pair[0].scopes` |
| `binding.rs` | `analyze_define` (local) | `items[1].scopes` |
| `binding.rs` | `analyze_define` (global) | `&[]` (globals are runtime) |
| `binding.rs` | `analyze_lambda` | `param.scopes` |
| `special.rs` | `analyze_pattern` | `syntax.scopes` |

**`lookup()` call sites** — all need `ref_scopes` parameter added:

| File | Function | Source of scopes |
|------|----------|-----------------|
| `forms.rs` | `analyze_expr` (Symbol) | `syntax.scopes` |
| `binding.rs` | `analyze_define` | `items[1].scopes` |
| `binding.rs` | `analyze_set` | `items[1].scopes` |

### Step 4: Tests

**tests/integration/hygiene.rs** (new file):

Core hygiene tests:
- `test_macro_no_capture`: macro `tmp` doesn't shadow caller's `tmp`
- `test_macro_no_leak`: caller can't see macro's internal bindings
- `test_nested_macro_hygiene`: nested expansions with overlapping names
- `test_non_macro_code`: non-macro code works identically

Capture interaction tests:
- `test_macro_closure_captures_callsite`: macro-generated closure captures
  a call-site variable correctly
- `test_set_through_macro`: `set!` on call-site variable works in macro
- `test_nested_closure_macro`: nested macro expansions generating closures

Counterfactual: remove scope stamping, verify capture occurs

### Step 5: Update docs

- `src/hir/AGENTS.md` — document scope-aware lookup
- `src/syntax/AGENTS.md` — update hygiene section
- `docs/MACROS.md` — update hygiene section as implemented

---

## Open Questions

1. **Performance.** Every macro call compiles and executes bytecode.
   For hot macros (e.g., `when` used hundreds of times), this could be
   slow. Mitigation: cache compiled bytecode per MacroDef. Deferred —
   correctness first per AGENTS.md.

2. **Error provenance.** When a macro body errors, the error should
   point to the macro definition, not the call site. The wrapping
   let-expression has synthetic spans — errors in argument binding
   might be confusing. Needs testing.

## Risk Assessment

**PR 3 `lookup()` rewrite** is the highest remaining risk. This is the
most complex function in the Analyzer (~110 lines of capture tracking,
function boundary detection, transitive capture resolution). The
scope-aware matching adds a new dimension.
