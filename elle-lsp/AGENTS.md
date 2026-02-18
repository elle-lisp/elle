# elle-lsp

Language Server Protocol implementation for Elle.

## Responsibility

Provide IDE features for Elle via the LSP protocol:
- Hover information (symbol name, kind, arity, docs)
- Go-to-definition
- Find references
- Rename symbol
- Code completion
- Document formatting
- Diagnostics (via linting)

## Architecture

elle-lsp is a synchronous LSP server reading JSON-RPC from stdin and writing
to stdout. It uses the new pipeline exclusively:

```
Source → analyze_all_new → HIR + bindings
                              ↓
                    extract_symbols_from_hir → SymbolIndex
                    HirLinter → Diagnostics
```

`CompilerState` holds per-document state. On every open/change, it re-analyzes
the document and rebuilds the `SymbolIndex` and diagnostics. IDE features
(hover, completion, definition, references, rename) query the `SymbolIndex`.

## Files

| File | Lines | Content |
|------|-------|---------|
| `src/main.rs` | ~550 | LSP message loop, JSON-RPC dispatch |
| `src/lib.rs` | ~20 | Module declarations |
| `src/compiler_state.rs` | ~200 | Document state, compilation, symbol extraction |
| `src/hover.rs` | ~110 | Hover provider |
| `src/completion.rs` | ~180 | Completion provider |
| `src/definition.rs` | ~110 | Go-to-definition |
| `src/references.rs` | ~160 | Find references |
| `src/rename.rs` | ~350 | Rename with validation |
| `src/formatting.rs` | ~110 | Document formatting via `elle::formatter` |

## Dependencies

- `elle` — core library (analyze_all_new, HirLinter, extract_symbols_from_hir, SymbolIndex)
- `elle-lint` — not used directly (linting is done via elle's HirLinter)
- `lsp-types` — LSP type definitions
- `serde` / `serde_json` — JSON-RPC serialization

## Invariants

1. **Uses new pipeline only.** No `Expr`, no `value_to_expr`, no old pipeline.
2. **Synchronous I/O.** No async runtime. Reads stdin, writes stdout.
3. **Per-document state.** Each open document has its own `SymbolIndex` and diagnostics.
4. **Formatting is pipeline-independent.** Uses `elle::formatter::format_code` on source text directly.
