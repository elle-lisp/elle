# elle-lint

Static analysis tool for Elle source files.

## Responsibility

Lint Elle code for naming conventions, arity mismatches, and style issues.
Outputs diagnostics in human-readable or JSON format.

## Architecture

elle-lint is a normal application consuming the `elle` library. It uses the
new pipeline exclusively:

```
Source → Reader → Syntax → Expander → Analyzer → HIR
                                                    ↓
                                             HirLinter → Diagnostics
```

Entry point: `elle::analyze_all()` → `elle::hir::HirLinter`

## Files

| File | Lines | Content |
|------|-------|---------|
| `src/main.rs` | ~120 | CLI: arg parsing, file/directory traversal |
| `src/lib.rs` | ~160 | `Linter` wrapper, config, output formatting |

## Key types

| Type | Purpose |
|------|---------|
| `Linter` | Main linter instance with config |
| `LintConfig` | Severity filter, output format |
| `OutputFormat` | `Human` or `Json` |

## Dependencies

- `elle` — core library (analyze_all, HirLinter, Diagnostic, Severity)
- `serde_json` — JSON output formatting

## Invariants

1. **Uses new pipeline only.** No `Expr`, no `value_to_expr`, no old pipeline.
2. **Lint rules live in `elle::lint::rules`.** elle-lint does not define its own rules.
3. **Exit codes:** 0 = clean, 1 = errors, 2 = warnings only.
