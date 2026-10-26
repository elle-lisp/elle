# Lint

<!-- audited: 2026-10-26 -->

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

`elle lint PATH…` analyzes each file on a fresh `Runtime` and prints every
diagnostic at or above `--level`: `error`, `warning` or `info`. The default is
`LintConfig::default()`'s, `info`, and `--help` prints it from there.

A file argument is linted whatever its name. A directory argument lints every
`.lisp` file below it, in sorted path order, and skips a subdirectory whose
name starts with `.`. The linter reads s-expressions only, so `.lisp` is the
one extension it takes from a directory. The arguments are linted in the order
given, so two runs over the same tree print the same output.

`--format text`, the default, prints each diagnostic as a line, the message
under it, and the suggestions under that:

```text
src/a.lisp:4:2 error: undefined-variable
  message: undefined variable: g
```

The path is the one the command was given or found in a directory, never the
location's own file. `--format json` prints one object whose `diagnostics`
array holds each diagnostic with its `file`, `severity`, `code`, `rule`,
`message`, `line`, `column` and `suggestions`.

The command exits 1 on an error diagnostic, 2 on warnings alone, and 0
otherwise. It also exits 1, with the reason on stderr, when:

- no path is given;
- a path names nothing;
- a file cannot be read, or a directory cannot be listed;
- an option is unknown, or `--format` or `--level` has no value or an unknown
  one.
