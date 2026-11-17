# rewrite

<!-- audited: 2026-11-17 -->

Source-to-source rewriting engine. Edits by byte span preserve comments,
whitespace, and formatting.

[Epochs](../../docs/epochs.md) owns the migration rules, `elle rewrite`'s
command line, and the order the rules run in.

## Responsibility

- Lex source to tokens with byte offsets, under the file's own epoch
- Apply rewrite rules to tokens, producing edits
- Respell tokens whose lexical rules changed since the file's epoch
- Move each def that stands outside a body into a let. This pass reads the
  syntax tree that `crate::epoch::placement` plans over, and still edits by
  byte span
- Apply edits back-to-front to preserve byte offsets
- CLI interface for batch rewriting files

Does NOT:
- Format code (that's `formatter/`, which calls `rewrite_file` first)
- Understand scoping or bindings, except the reads the def placement plan
  checks

## Interface

| Type / Function | Purpose |
|-----------------|---------|
| `Edit` | Byte-range replacement: offset, length, new text |
| `apply_edits` | Apply edits to source (sorts back-to-front, errors on overlap) |
| `SourceText` | Source text, its name, and the lexicon that tokenizes it |
| `RewriteRule` | Trait: examine a token, optionally produce an Edit |
| `RenameSymbol` | Data-driven symbol rename from HashMap |
| `collect_edits` | Lex a `SourceText` and collect the edits a set of rules makes |
| `collect_lexical_edits` | Respell tokens between two lexicons, by byte span |
| `place_defs` | The def placement pass over rewritten text: new text, defs moved, notes |
| `rewrite_file_with_notes` | Every migration rule over one file; answers `Rewritten` |
| `Rewritten` | The new text and edit count, or `None`, and each note `LINE:COL: why` |
| `rewrite_file` | `rewrite_file_with_notes` without the notes (`elle fmt`) |
| `apply_library_rules` | Rename/replace/report rules as data, applied to a fixpoint (`compile/apply-rules`) |
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
    │
    ▼
place_defs(new source) → read the tree, plan, edit → final text and notes
```

Every token pass reads its tokens through `SourceText`, never through a bare
`Lexer`. A file written before a token-level change tokenizes under its own
epoch's rules, and one pass reaching for the current epoch's would silently
disagree with the others ([lexicon](../../docs/impl/lexicon.md)). The def
placement pass reads text that is already in the current epoch's spelling.
