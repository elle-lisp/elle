# Adding a New Lint Rule


Linting operates on HIR trees. Rules live in `src/lint/rules.rs`; the
tree walker lives in `src/hir/lint.rs`.

### Files to modify (in order)

1. **`src/lint/diagnostics.rs`** — Claim a code: add a `LintCode` const and
   list it in `WARNINGS`.

2. **`src/lint/rules.rs`** — Implement the rule function.

3. **`src/hir/lint.rs`** — Call the rule from the appropriate `HirKind`
   arm in `HirLinter::check()`.

4. **This file** — Add the code to the table below.

### Step by step

**Step 1: `src/lint/diagnostics.rs`** — Claim a code. The const binds the
code to the rule name, and `WARNINGS` is what the table below is checked
against:

```rust
pub const MY_RULE: LintCode = LintCode::new("W006", "my-rule-name");

pub const WARNINGS: &[LintCode] = &[
    ARITY_MISMATCH,
    MUTABLE_BINDING_NEVER_ASSIGNED,
    UNUSED_BINDING,
    NON_TAIL_SELF_RECURSION,
    MY_RULE,
];
```

**Step 2: `src/lint/rules.rs`** — Write the rule. Rules take context and
push `Diagnostic`s:

```rust
use super::diagnostics::{Diagnostic, MY_RULE};
use crate::reader::SourceLoc;

pub(crate) fn check_my_rule(
    context_data: &str,
    location: &Option<SourceLoc>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if /* violation detected */ {
        let mut diag = Diagnostic::warn(
            MY_RULE,
            "description of the issue",
            location.clone(),
        );
        diag.suggestions.push("how to fix it".to_string());
        diagnostics.push(diag);
    }
}
```

**Step 3: `src/hir/lint.rs`** — Call the rule from the tree walker. A rule
about a binding goes in `check_binding_site`, which `Let`, `Letrec`, and
`Define` all route through; a rule about any other form goes in that form's
`HirKind` arm in `check()`:

```rust
fn check_binding_site(
    &mut self,
    binding: crate::hir::Binding,
    init: &Hir,
    loc: &Option<SourceLoc>,
    symbols: &SymbolTable,
    arena: &BindingArena,
) {
    // Call your rule here:
    if let Some(sym_name) = symbols.name(arena.get(binding).name) {
        rules::check_my_rule(sym_name, loc, &mut self.diagnostics);
    }
    // ...existing rules and the descent into `init`
}
```

Loop, match, and pattern bindings never reach `check_binding_site`, so a
rule placed there does not see them.

### Key types

| Type | Location | Purpose |
|------|----------|---------|
| `Diagnostic` | `src/lint/diagnostics.rs` | Finding with severity, code, message, location |
| `LintCode` | `src/lint/diagnostics.rs` | A code and the rule name it travels with |
| `Severity` | `src/lint/diagnostics.rs` | `Info`, `Warning`, `Error` |
| `HirLinter` | `src/hir/lint.rs` | Tree walker that calls rules |
| `Linter` | `src/lint/cli.rs` | CLI wrapper that runs `HirLinter` |

### Diagnostic codes

The linter raises these warnings:

| Code | Rule | Fires on |
|------|------|----------|
| `W002` | `arity-mismatch` | A call to a built-in with the wrong argument count. |
| `W003` | `mutable-binding-never-assigned` | A `var`/`@` binding that no `assign` targets. |
| `W004` | `unused-binding` | A `def`/`let`/`letrec` binding with zero uses. |
| `W005` | `non-tail-self-recursion` | A function whose self-call is not in tail position. |

`diagnostics::WARNINGS` (`src/lint/diagnostics.rs`) is the source of this
table, and a test in `src/lint/diagnostics/tests.rs` compares the two. A rule
that lands without its row here fails that test.

`W004` reads the def-use chains `src/hir/defuse.rs` builds, and `W005`
reads the `is_tail` flag `src/hir/tailcall.rs` sets. Neither rule tracks
its own state, and neither adds a field to `BindingInner`.

`W001` is unclaimed and no rule raises it, so the table has no row for it.
Use `W006+` for new warnings. `E000`–`E005` are analysis failures the lint
CLI reports (`src/lint/cli.rs`), not rules; take `E006` for a new error and
`I001` for the first info diagnostic.

### How linting runs

1. `Linter::lint_str()` (in `src/lint/cli.rs`) calls `analyze_all()` to
   get HIR.
2. For each analysis result, it creates a `HirLinter` and calls
   `hir_linter.lint(&analysis.hir, &symbols)`.
3. `HirLinter::check()` recursively walks the HIR tree, calling rule
   functions that push `Diagnostic`s.
4. The LSP (`src/lsp/state.rs`) uses the same `HirLinter` for real-time
   diagnostics.

---

---

## See also

- [Cookbook index](index.md)
