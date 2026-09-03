# rewrite

Source-to-source rewriting engine. Token-level rewrites that preserve comments,
whitespace, and formatting.

## Responsibility

- Lex source to tokens with byte offsets, under the file's own epoch
- Apply rewrite rules to tokens, producing edits
- Respell tokens whose lexical rules changed since the file's epoch
- Apply edits back-to-front to preserve byte offsets
- CLI interface for batch rewriting files

Does NOT:
- Parse to AST (uses raw tokens only)
- Format code (that's `formatter/`)
- Understand scoping or bindings (purely textual)

## Interface

| Type / Function | Purpose |
|-----------------|---------|
| `Edit` | Byte-range replacement: offset, length, new text |
| `apply_edits` | Apply edits to source (sorts back-to-front, panics on overlap) |
| `SourceText` | Source text, its name, and the lexicon that tokenizes it |
| `RewriteRule` | Trait: examine a token, optionally produce an Edit |
| `RenameSymbol` | Data-driven symbol rename from HashMap |
| `collect_lexical_edits` | Respell tokens between two lexicons, by byte span |
| `rewrite_source` | Core: lex + apply rules + produce (new_source, edits) |
| `run` | CLI entry point for `elle rewrite` |

## Data flow

```
Source string
    │
    ├─► detect_epoch_in_source() → the epoch the file declares
    │
    ▼
SourceText { text, name, Lexicon::for_epoch(epoch) }
    │
    ├─► tokens() / code_tokens() → TokenWithLoc (with byte_offset)
    │
    ▼
For each token, apply rules and respell → collect Vec<Edit>
    │
    ▼
apply_edits(source, edits) → new source string
```

Every pass reads its tokens through `SourceText`, never through a bare
`Lexer`. A file written before a token-level change tokenizes under its own
epoch's rules, and one pass reaching for the current epoch's would silently
disagree with the others (`docs/impl/lexicon.md`).
