# symbols

Pipeline-agnostic symbol index types for IDE features.

## Responsibility

Define the data types used by IDE features (hover, completion, go-to-definition,
find-references, rename). The actual extraction logic lives in `hir/symbols.rs`;
this module provides the shared types.

## Interface

| Type | Purpose |
|------|---------|
| `SymbolIndex` | Collection of definitions, usages, and locations |
| `SymbolDef` | Definition info: name, kind, location, arity, docs |
| `SymbolKind` | `Function`, `Variable`, `Builtin`, `Macro`, `Module` |

## Dependents

- `hir/symbols.rs` — HIR-based symbol extraction builds SymbolIndex
- `lsp/` — all IDE features query SymbolIndex

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~175 | Symbol index types (SymbolIndex, SymbolDef, SymbolKind) |
