# lint

Pipeline-agnostic lint types and rules.

## Responsibility

Define diagnostic types and lint rules that can be used by any pipeline.
The actual linting logic (tree walking) lives in `hir/lint.rs`; this module
provides the shared types and rule implementations.

## Interface

| Type | Purpose |
|------|---------|
| `Diagnostic` | Lint finding with severity, code, message, location |
| `Severity` | `Info`, `Warning`, `Error` |
| `DiagnosticContext` | Optional source text for context display |

| Function | Purpose |
|----------|---------|
| `check_naming_convention` | Warns on non-kebab-case identifiers |
| `check_call_arity` | Warns on wrong argument count for known functions |

## Dependents

- `hir/lint.rs` — HIR linter calls rules and produces Diagnostics
- `elle-lint` — re-exports Diagnostic/Severity for CLI output
- `elle-lsp` — uses Diagnostic/Severity for LSP diagnostics
- `compiler/linter/` — backward-compat re-export

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~5 | Module declarations, re-exports |
| `diagnostics.rs` | ~180 | `Diagnostic`, `Severity`, formatting |
| `rules.rs` | ~190 | Naming convention checks, arity checks |
