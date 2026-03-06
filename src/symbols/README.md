# Symbol Indexing

The symbols module provides symbol table management and IDE feature support. It tracks all symbols in a program for code completion, navigation, and refactoring.

## Symbol Table

The symbol table maintains:

- **Symbol interning**: Map from string names to unique `SymbolId` values
- **Global bindings**: Map from `SymbolId` to `Value` (for runtime lookup)
- **Symbol metadata**: Type information, docstrings, source locations

## IDE Features

The symbol index enables:

- **Code completion**: Suggest available symbols at cursor position
- **Go to definition**: Navigate to where a symbol is defined
- **Find references**: Find all uses of a symbol
- **Rename**: Refactor symbol names across the file
- **Hover**: Display type information and docstrings

## Usage

```rust
// Create a symbol table
let mut symbols = SymbolTable::new();

// Intern a symbol
let sym_id = symbols.intern("my-function");

// Look up a symbol
let name = symbols.name(sym_id);
```

## Integration

The symbol system integrates with:

- **HIR analysis**: Extracts symbols from analyzed code
- **LSP**: Provides symbol information for IDE features
- **VM**: Looks up global bindings by symbol ID

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`src/hir/symbols.rs`](../hir/symbols.rs) - symbol extraction from HIR
- [`src/lsp/`](../lsp/) - IDE feature implementation
