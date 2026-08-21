# symbols

Pipeline-agnostic symbol index types for IDE features.

## Responsibility

Define the data types used by IDE features (hover, completion, go-to-definition,
find-references, rename). The actual extraction logic lives in `hir/symbols.rs`;
this module provides the shared types.

## Interface

| Type | Purpose |
|------|---------|
| `SymbolIndex` | Definitions, usages, and locations, keyed by `DefId` |
| `SymbolDef` | Definition info: name, kind, location, arity, docs |
| `SymbolKind` | `Function`, `Variable`, `Builtin`, `Macro`, `Module` (+ `lsp_kind`/`lsp_completion_kind`/`lsp_symbol_kind`) |
| `DefId` | Per-binding identity (a `u32`) |

`SymbolIndex` is keyed by `DefId`, not the interned name (`SymbolId`): two
locals both named `x` in different scopes get distinct `DefId`s, so
rename/find-references never collapse them. `DefId` is derived from the HIR
`Binding` arena index (`Binding::def_id`) at extraction time and is opaque
afterward. It is kept a bare `u32` newtype (not a `Binding`) so this module
stays pipeline-agnostic. `SymbolDef.id` still holds the `SymbolId` for callers
that group by name; usage-only bindings (e.g. primitives) get a placeholder
`SymbolDef` with `location: None`.

## Dependents

- `hir/symbols.rs` — HIR-based symbol extraction builds SymbolIndex (keys via `Binding::def_id`)
- `lsp/` — all IDE features query SymbolIndex
- `primitives/compile/` — the `compile/*` reflection API (selects the
  data-bearing binding by name via `binding_for_name`)
