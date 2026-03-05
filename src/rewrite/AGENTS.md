# rewrite

Source-to-source rewriting engine. Token-level rewrites that preserve comments,
whitespace, and formatting.

## Responsibility

- Lex source to tokens with byte offsets
- Apply rewrite rules to tokens, producing edits
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
| `RewriteRule` | Trait: examine a token, optionally produce an Edit |
| `RenameSymbol` | Data-driven symbol rename from HashMap |
| `rewrite_source` | Core: lex + apply rules + produce (new_source, edits) |
| `run` | CLI entry point for `elle rewrite` |

## Data flow

```
Source string
    │
    ▼
Lexer::new(source)
    │
    ├─► next_token_with_loc() → TokenWithLoc (with byte_offset)
    │
    ▼
For each token, apply rules → collect Vec<Edit>
    │
    ▼
apply_edits(source, edits) → new source string
```

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~10 | Module declarations |
| `edit.rs` | ~110 | Edit type, apply_edits, overlap detection |
| `rule.rs` | ~110 | RewriteRule trait, RenameSymbol |
| `engine.rs` | ~80 | rewrite_source function |
| `run.rs` | ~120 | CLI entry point |
