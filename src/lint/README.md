# Lint System

The lint system performs static analysis on Elle code to detect potential bugs, style issues, and performance problems.

## Lint Rules

The linter checks for:

- **Unused variables**: Variables defined but never referenced
- **Shadowing**: Variables that shadow outer bindings
- **Type mismatches**: Obvious type errors (e.g., calling non-function)
- **Arity errors**: Calling functions with wrong number of arguments
- **Dead code**: Unreachable expressions
- **Performance issues**: Inefficient patterns (e.g., repeated list traversal)

## Running the Linter

```bash
# Lint a file
cargo run -- lint path/to/file.lisp

# Lint with verbose output
cargo run -- lint --verbose path/to/file.lisp
```

## Diagnostic Types

Diagnostics are categorized by severity:

- **Error**: Code that will definitely fail at runtime
- **Warning**: Code that might fail or is inefficient
- **Info**: Style suggestions and best practices

## Integration

The linter is integrated into:

- **CLI**: `elle lint` command
- **LSP**: Hover diagnostics and code actions
- **REPL**: Optional linting on each expression

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`src/hir/lint.rs`](../hir/lint.rs) - HIR-based linter implementation
- [`src/lsp/`](../lsp/) - language server integration
