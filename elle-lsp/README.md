# Elle Language Server Protocol (LSP)

A Language Server Protocol implementation for Elle Lisp providing real-time IDE support including diagnostics, hover information, and symbol navigation.

## Features

### Implemented
- ✅ **Real-time Diagnostics**: Integration with elle-lint for on-the-fly error checking
- ✅ **Text Synchronization**: Full document sync protocol
- ✅ **Hover Information**: Show function and symbol information on hover
- ✅ **Server Initialization**: Proper LSP handshake and shutdown
- ✅ **Go to Definition**: Navigate to symbol definitions
- ✅ **Find References**: Find all usages of a symbol
- ✅ **Code Completion**: Suggest available symbols
- ✅ **Symbol Renaming**: Refactor with rename-all capability
- ✅ **Document Formatting**: Format Elle source code

### Planned
- [ ] Workspace Diagnostics
- [ ] Semantic Tokens (syntax highlighting via LSP)

## Installation

### Build from Source

```bash
cd elle/elle-lsp
cargo build --release
```

The binary will be at `target/release/elle-lsp`.

### Configuration

The LSP server reads from stdin and writes to stdout, following the LSP protocol specification.

## VS Code Integration

### Prerequisites
- VS Code 1.60+
- Node.js 14+

### Setup

1. Build the LSP server:
```bash
cd elle-lsp
cargo build --release
```

2. Create or update `.vscode/settings.json`:
```json
{
  "[elle]": {
    "editor.defaultFormatter": "disruptek.elle",
    "editor.formatOnSave": true
  }
}
```

3. Install the VS Code extension (from `vscode-extension/`):
```bash
cd vscode-extension
npm install
npm run compile
# Then load it in VS Code via "Load Unpacked" in Extensions panel
```

### Usage

Once installed, open any `.lisp` or `.lisp` file:
- Diagnostics appear automatically as you type
- Hover over symbols to see information
- Use standard editor commands for navigation

## Language Server Protocol

The server implements the following LSP capabilities:

### Supported Methods

#### Initialization
- `initialize`: Initialize the server with client capabilities
- `shutdown`: Graceful shutdown
- `exit`: Clean exit

#### Text Document Synchronization
- `textDocument/didOpen`: Handle document open
- `textDocument/didChange`: Handle document changes (full sync)
- `textDocument/didClose`: Handle document close

#### Features
- `textDocument/hover`: Provide hover information
- `textDocument/definition`: Go to symbol definition
- `textDocument/references`: Find symbol references
- `textDocument/completion`: Code completion
- `textDocument/rename`: Symbol renaming
- `textDocument/formatting`: Document formatting

### Diagnostic Publishing
Diagnostics are published via `textDocument/publishDiagnostics` using the elle-lint linter.

## Architecture

### Message Flow

```
Client                          Server
  |                               |
  |---initialize request--------->|
  |                               |
  |<------initialize response------|
  |                               |
  |---textDocument/didOpen------->|
  |                               |
  |<---publishDiagnostics---------|
  |                               |
  |---textDocument/didChange----->|
  |                               |
  |<---publishDiagnostics---------|
  |                               |
  |---textDocument/hover--------->|
  |                               |
  |<------hover response----------|
  |                               |
  |---shutdown request----------->|
  |                               |
  |<------shutdown response--------|
```

### Components

**`main.rs`**: Core server loop handling LSP message protocol
- Reads `Content-Length` headers
- Parses JSON-RPC messages
- Delegates to request handlers
- Sends responses back to client

**`lib.rs`**: Library interface for embedding LSP in other tools

**`compiler_state.rs`**: Document state, compilation, symbol extraction

**`hover.rs`**: Hover provider

**`completion.rs`**: Completion provider

**`definition.rs`**: Go-to-definition

**`references.rs`**: Find references

**`rename.rs`**: Rename with validation

**`formatting.rs`**: Document formatting via `elle::formatter`

## Development

### Running the Server

```bash
# Run server for manual testing (reads from stdin)
cargo run --bin elle-lsp

# In another terminal, send test requests via netcat or similar
# Or use an LSP client like VS Code
```

### Testing

```bash
# Run unit tests
cargo test

# Run with main Elle to ensure no regressions
cargo test --release
```

### Debugging

Set environment variable for debug output:
```bash
RUST_LOG=debug ./target/debug/elle-lsp
```

## LSP Specification Compliance

This implementation follows LSP 3.17.0 specification.

Key references:
- [Language Server Protocol Specification](https://microsoft.github.io/language-server-protocol/specifications/specification-current/)
- [LSP Implementation Guide](https://microsoft.github.io/language-server-protocol/implementationGuide/implementation/)

## Integration Points

### elle-lint Integration
The LSP server integrates with `elle-lint` to provide real-time diagnostics:

```rust
let mut linter = elle_lint::Linter::new(elle_lint::LintConfig::default());
linter.lint_str(text, "document")?;
for diag in linter.diagnostics() {
    // Convert to LSP Diagnostic and publish
}
```

### Symbol Table Integration
The LSP server integrates with Elle's `SymbolIndex` (built from HIR) for:
- Hover information (symbol name, kind, arity, docs)
- Go to definition (symbol resolution)
- Find references (symbol usage tracking)
- Code completion (available symbols)
- Rename (with validation)

## Performance Considerations

- **Latency**: Server processes changes immediately upon `textDocument/didChange`
- **Memory**: Maintains open document texts in memory
- **CPU**: Runs linter on each change (can be optimized with debouncing)

## Future Enhancements

1. **Workspace Support**: Handle multi-file workspaces
2. **Incremental Sync**: Optimize large document changes
3. **Semantic Tokens**: Provide syntax highlighting via LSP

## Troubleshooting

### Server not responding
- Check that stdin/stdout are properly connected
- Verify `Content-Length` headers are present
- Enable debug logging: `RUST_LOG=debug`

### Diagnostics not appearing
- Verify elle-lint is compiled and linked
- Check that document URI is valid
- Review server output for lint errors

### VS Code extension not working
- Ensure LSP server binary path is correct
- Check VS Code output panel for error messages
- Verify file extension is `.lisp` or `.lisp`

## Contributing

Contributions welcome! Please:
1. Follow Rust code standards (`cargo fmt`, `cargo clippy`)
2. Add tests for new features
3. Update documentation
4. Test with VS Code before submitting PR

## License

Same as Elle interpreter

## References

- [Language Server Protocol](https://microsoft.github.io/language-server-protocol/)
- [JSON-RPC 2.0 Specification](https://www.jsonrpc.org/specification)
- [Tower LSP Documentation](https://docs.rs/tower-lsp/latest/tower_lsp/) (for reference only; we use simpler lsp-types)
