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

| Function | Code | Purpose |
|----------|------|---------|
| `check_call_arity` | `W002` | Warns on a built-in call with the wrong argument count |
| `check_mutable_never_assigned` | `W003` | Warns on a `var`/`@` binding no `assign` targets |
| `check_unused_binding` | `W004` | Warns on a `def`/`let`/`letrec` binding with zero uses |
| `check_non_tail_self_recursion` | `W005` | Warns on a self-call outside tail position |

Every binding rule shares one exemption set — synthetic, primitive, and
`_`-prefixed names — through `reportable_name`.

## Dependents

- `hir/lint.rs` — HIR linter calls rules and produces Diagnostics
- `lint/cli.rs` — Linter wrapper for CLI output
- `lsp/state.rs` — uses Diagnostic/Severity for LSP diagnostics
