# Lint

<!-- audited: 2026-10-23 -->

The linter's diagnostic type, its codes, its rules, and the `elle lint`
command.

[The cookbook](../../docs/cookbook/lint-rules.md) owns the table of codes, how
a lint run proceeds, and how to add a rule. [The HIR linter](../hir/lint.rs)
walks the tree and calls each rule.

## The rules

A rule reads facts analysis already holds and pushes a `Diagnostic`, raised
through `Diagnostic::warn` or `Diagnostic::hint` with its `LintCode`, so a code
and its rule name cannot drift apart.

| Rule function | Code |
|---------------|------|
| `check_call_arity` | `W002` |
| `check_mutable_never_assigned` | `W003` |
| `check_unused_binding` | `W004` |
| `check_non_tail_self_recursion` | `W005` |
| `check_fiber_body` | `I001`, `I002` |

`W003`, `W004` and `W005` share one exemption through `reportable_name`: a
synthetic binding, a primitive's binding, or a name that starts with `_`.

## The command

`elle lint FILE…` analyzes each file on a fresh `Runtime` and prints every
diagnostic at or above `--level`: `error`, `warning` or `info`, which is the
default. `--format json` prints them as one JSON object, and `--format text`
as lines. It exits 1 on an error, 2 on warnings alone, and 0 otherwise.
