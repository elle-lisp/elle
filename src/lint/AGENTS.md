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

## Dependents

- `hir/lint.rs` — HIR linter calls rules and produces Diagnostics
- `lint/cli.rs` — Linter wrapper for CLI output
- `lsp/state.rs` — uses Diagnostic/Severity for LSP diagnostics
