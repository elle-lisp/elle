# lsp

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

Synchronous LSP server reading JSON-RPC from stdin, writing to stdout.
Uses the new pipeline exclusively:

```
Source → analyze_all → HIR + bindings
                           ↓
                 extract_symbols_from_hir → SymbolIndex
                 HirLinter → Diagnostics
```

`CompilerState` holds per-document state. On every open/change, it re-analyzes
the document and rebuilds the `SymbolIndex` and diagnostics. IDE features
(hover, completion, definition, references, rename) query the `SymbolIndex`.

Invoked via `elle --lsp`.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~12 | Module declarations |
| `run.rs` | ~470 | LSP message loop, JSON-RPC dispatch |
| `state.rs` | ~180 | Document state, compilation, symbol extraction |
| `hover.rs` | ~110 | Hover provider |
| `completion.rs` | ~175 | Completion provider |
| `definition.rs` | ~110 | Go-to-definition |
| `references.rs` | ~160 | Find references |
| `rename.rs` | ~350 | Rename with validation |
| `formatting.rs` | ~106 | Document formatting via `elle::formatter` |

## Invariants

1. **Uses new pipeline only.** No `Expr`, no `value_to_expr`, no old pipeline.
2. **Synchronous I/O.** No async runtime. Reads stdin, writes stdout.
3. **Per-document state.** Each open document has its own `SymbolIndex` and diagnostics.
4. **Formatting is pipeline-independent.** Uses `elle::formatter::format_code` on source text directly.
