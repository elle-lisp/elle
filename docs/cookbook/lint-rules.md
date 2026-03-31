# Adding a New Lint Rule


Linting operates on HIR trees. Rules live in `src/lint/rules.rs`; the
tree walker lives in `src/hir/lint.rs`.

### Files to modify (in order)

1. **`src/lint/rules.rs`** — Implement the rule function.

2. **`src/hir/lint.rs`** — Call the rule from the appropriate `HirKind`
   arm in `HirLinter::check()`.

3. **`src/lint/mod.rs`** — Re-export if the rule is public.

### Step by step

**Step 1: `src/lint/rules.rs`** — Write the rule. Rules take context and
push `Diagnostic`s:

```rust
use super::diagnostics::{Diagnostic, Severity};
use crate::reader::SourceLoc;

pub fn check_my_rule(
    context_data: &str,
    location: &Option<SourceLoc>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if /* violation detected */ {
        diagnostics.push(Diagnostic::new(
            Severity::Warning,
            "W004",                    // unique code
            "my-rule-name",            // kebab-case rule name
            "description of the issue",
            location.clone(),
        ).with_suggestions(vec![
            "how to fix it".to_string(),
        ]));
    }
}
```

**Step 2: `src/hir/lint.rs`** — Call the rule from the tree walker. Find
the appropriate `HirKind` match arm in `check()`:

```rust
// Example: check all let bindings for some property
HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
    for (binding, init) in bindings {
        // Call your rule here:
        if let Some(sym_name) = symbols.name(binding.name()) {
            rules::check_my_rule(sym_name, &loc, &mut self.diagnostics);
        }
        self.check(init, symbols);
    }
    self.check(body, symbols);
}
```

### Key types

| Type | Location | Purpose |
|------|----------|---------|
| `Diagnostic` | `src/lint/diagnostics.rs` | Finding with severity, code, message, location |
| `Severity` | `src/lint/diagnostics.rs` | `Info`, `Warning`, `Error` |
| `HirLinter` | `src/hir/lint.rs` | Tree walker that calls rules |
| `Linter` | `src/lint/cli.rs` | CLI wrapper that runs `HirLinter` |

### Diagnostic codes

- `W001` — naming-kebab-case
- `W002` — arity-mismatch
- `W003` — non-exhaustive-match
- Use `W004+` for new warnings, `E00x` for errors, `I00x` for info.

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
