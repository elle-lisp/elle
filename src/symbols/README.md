# Symbol index

<!-- audited: 2026-10-23 -->

The types of the symbol index: each binding's definition, its location and its
uses, which the language server and `compile/*` read.

[mod.rs](mod.rs) holds the types and nothing that builds them.
[src/hir/symbols.rs](../hir/symbols.rs) walks an analyzed tree and fills an
index. The module imports nothing from `hir`, so any front end can build one.
Interning a name and spelling it belong to [symbol.rs](../symbol.rs)
([symbol](../../docs/impl/symbol.md)).

## Types

| Type | Purpose |
|------|---------|
| `SymbolIndex` | The definitions, the definition sites, the usage sites and the completion candidates, each keyed by `DefId` |
| `SymbolDef` | A definition's name, kind, location, arity and docstring |
| `SymbolKind` | `Function`, `Variable`, `Builtin`, `Macro` or `Module`, with the LSP codes each one maps to |
| `DefId` | One binding's identity, a `u32` |

## A key per binding

`SymbolIndex` is keyed by `DefId`, never by the interned name. Two locals both
named `x` in different scopes get two `DefId`s, so rename and find-references
never merge them.

`Binding::def_id` derives a `DefId` from the binding's index in the HIR arena
when the index is built. The arena is gone afterward, so a `DefId` is opaque,
and only its equality means anything. It stays a bare `u32` rather than a
`Binding`, which keeps this module free of `hir`.

`SymbolDef::id` still holds the `SymbolId`, for a caller that groups by name.
A binding the file uses and never defines, a primitive for example, gets a
placeholder `SymbolDef` of kind `Builtin` with `location: None`. A completion
candidate is a definition with a location.

## Readers

| Reader | What it does with the index |
|--------|-----------------------------|
| [src/lsp](../lsp/README.md) | Hover, completion, go-to-definition, find-references, rename and the document symbols |
| [compile/mod.rs](../primitives/compile/mod.rs) | The `compile/*` reflection API, which finds the binding that carries a name's data through `binding_for_name` |
