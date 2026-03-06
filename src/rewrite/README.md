# Source Rewriting

The rewrite module provides source-to-source transformation capabilities for Elle code. It enables refactoring, code generation, and automated transformations.

## Rewriting Capabilities

- **Variable renaming**: Rename variables while respecting scope
- **Code generation**: Generate Elle code from templates
- **Refactoring**: Automated code transformations
- **Macro expansion**: Expand macros in place

## Usage

```bash
# Rewrite a file
cargo run -- rewrite --rule rename-var old-name new-name input.lisp > output.lisp
```

## Integration

The rewrite module integrates with:

- **LSP**: Rename refactoring via `textDocument/rename`
- **CLI**: `elle rewrite` command
- **Macros**: Macro expansion produces rewritten code

## Implementation

Rewriting is implemented via:

1. **Parsing**: Convert source to `Syntax` tree
2. **Analysis**: Resolve bindings and compute scope information
3. **Transformation**: Apply rewrite rules to the tree
4. **Emission**: Convert back to source code

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`src/syntax/`](../syntax/) - syntax tree types
- [`src/hir/`](../hir/) - binding resolution
- [`src/formatter/`](../formatter/) - code formatting
