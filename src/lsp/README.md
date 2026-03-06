# Language Server Protocol (LSP)

The LSP implementation provides IDE support for Elle, including code completion, hover information, diagnostics, and navigation.

## Features

- **Hover**: Display type information and docstrings
- **Diagnostics**: Real-time linting and error reporting
- **Completion**: Suggest variables, functions, and keywords
- **Go to Definition**: Navigate to variable/function definitions
- **Find References**: Find all uses of a variable
- **Rename**: Refactor variable names across the file
- **Formatting**: Format code according to Elle style

## Running the Language Server

```bash
# Start the LSP server
cargo run -- lsp

# Configure your editor to use it
# (See docs/lsp.md for editor-specific setup)
```

## Protocol Implementation

The LSP implementation uses:

- **JSON-RPC 2.0** for message transport
- **stdio** for communication (standard LSP transport)
- **Incremental parsing** for fast response times
- **Caching** of analysis results for performance

## Integration Points

The LSP server uses:

- [`src/hir/`](../hir/) — for binding resolution and type information
- [`src/lint/`](../lint/) — for diagnostics
- [`src/symbols/`](../symbols/) — for symbol indexing and navigation
- [`src/formatter/`](../formatter/) — for code formatting

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`docs/lsp.md`](../../docs/lsp.md) - LSP setup and configuration
- [`src/symbols/`](../symbols/) - symbol indexing for IDE features
