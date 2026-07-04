# lsp

Language Server Protocol implementation for Elle.

## Responsibility

Provide IDE features for Elle via the LSP protocol:
- Hover information (symbol name, kind, arity, docs)
- Go-to-definition
- Find references
- Rename symbol (single-document, per-binding)
- Document & workspace symbols
- Code completion
- Document formatting
- Diagnostics (via linting)

## Architecture

Synchronous LSP server reading JSON-RPC from stdin, writing to stdout.
Uses the file-as-letrec pipeline exclusively:

```
Source → analyze_file → HIR (single letrec)
                            ↓
                extract_symbols_from_hir → SymbolIndex
                snap_definition_locations  (fix def columns)
                HirLinter → Diagnostics
```

`CompilerState` holds per-document state. On every open/change/save, it
re-analyzes the document and rebuilds the `SymbolIndex` and diagnostics. IDE
features (hover, completion, definition, references, rename, documentSymbol)
query the `SymbolIndex`.

### Locating a symbol under the cursor

`locate::symbol_at` is the single, precise cursor→symbol resolver shared by
hover, definition, references, and rename. It tests exact identifier
containment using the name length recorded in the index. `locate` also owns the `file://` URI and
`name_range` JSON builders so every provider emits identical range shapes.

### Why definition columns are snapped

The analyzer records a definition at its *initializer* span — for `(def foo 1)`
that is the `1`, not `foo`. Usages are accurate. `locate::snap_definition_locations`
(called from `compile_document`) moves each definition column onto the real name
token using the source text, so rename edits and go-to-definition target the
identifier rather than its value. (The `compile/*` reflection API does not snap;
it tolerates the approximate location.)

### Per-binding identity (DefId)

`SymbolIndex` is keyed by `crate::symbols::DefId` (derived from the HIR
`Binding` arena index via `Binding::def_id`), not by the interned name
(`SymbolId`). Two locals both named `x` in different scopes therefore stay
distinct — renaming one no longer rewrites the other. `SymbolDef.name` carries
the spelling so providers need no symbol table to resolve names; usage-only
bindings (primitives) get a placeholder `SymbolDef` (kind `Builtin`, no
location) so hover still works on them.

The hover/completion providers use `vm.docs` (via `CompilerState::docs()`) for
builtin documentation; completion sources its *builtin* candidate list from that
same map rather than a hand-maintained list, so it never drifts.

Invoked via `elle lsp`.
## Capabilities advertised

`textDocumentSync` (openClose + full sync + save with text), `hoverProvider`,
`definitionProvider`, `referencesProvider`, `renameProvider`,
`documentSymbolProvider`, `workspaceSymbolProvider`, `documentFormattingProvider`,
`completionProvider`. File-change handling: `textDocument/didSave` and
`workspace/didChangeWatchedFiles` both recompile the affected open document and
republish diagnostics.

## Invariants

1. **Uses file-as-letrec pipeline.** `analyze_file` for file-level analysis. No
   `Expr`, no `value_to_expr`, no old pipeline.
2. **Synchronous I/O.** No async runtime. Reads stdin, writes stdout.
3. **Per-document state.** Each open document has its own `SymbolIndex` and
   diagnostics.
4. **Document path is the source name.** `compile_document` passes the document's
   real path (from its `file://` URI) to `analyze_file`, so every index location
   carries the actual file. This is what lets definition/references/rename emit
   client-acceptable URIs whose reconstructed paths match the request URI.
5. **Formatting is pipeline-independent.** Uses `elle::formatter::format_code` on
   source text directly.

## Known limitations

- **Shared analysis state.** One `VM` + `SymbolTable` + `CompileCtx` is reused
  across all documents and all recompiles. Top-level `def`s and `defmacro`s
  therefore accumulate: a name defined in document A is resolvable while
  analyzing document B, and a macro removed from a file's source still expands
  on the next recompile. The user-visible impact is bounded because the analyzer
  does not flag undefined symbols, and each document's `SymbolIndex` is rebuilt
  fresh from its own HIR (so completion/symbols never show another file's defs).
  A future refactor should snapshot a pristine post-stdlib baseline and analyze
  each document from an isolated clone.
- **Rename is single-document.** Edits are emitted only for the requested
  document's URI; cross-file rename would need a workspace-wide index.
- **`workspace/didChangeWatchedFiles`** only refreshes documents already open;
  there is no cross-file workspace index to update for closed files.
