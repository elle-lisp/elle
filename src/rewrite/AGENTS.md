# rewrite

<!-- audited: 2026-11-20 -->

Source-to-source rewriting engine. Edits by byte span preserve comments,
whitespace, and formatting.

[Epochs](../../docs/epochs.md) owns the migration rules, `elle rewrite`'s
command line, and the order the rules run in.

## Responsibility

- Lex source to tokens with byte offsets, under the file's own epoch
- Apply rewrite rules to tokens, producing edits
- Respell tokens whose lexical rules changed since the file's epoch
- Fold a file of several forms into its last `fn` form (`fold_module`). This
  pass reads the tree that `crate::epoch::fold` plans over, before the def
  placement
- Turn each def outside a body that a read after its form reaches into an
  assign, then move each other def that stands outside a body into a let
  (`place_defs`). This pass reads the syntax tree that
  `crate::epoch::placement` plans over, re-reads the text after the assigns,
  and still edits by byte span
- Splice each begin in a body that the epoch 18 rule names, then move each
  def a forward read reaches above the first form that reads it
  (`move_defs`). This pass reads the tree `crate::epoch::forward` orders,
  after the placement pass, and re-reads the text after the splices and
  before each body
- Bind each run of defs a lambda still reads ahead of in one letrec, and each
  def that reads its own name in a let (`bind_forward`). This pass reads the
  tree `crate::epoch::forward` plans over, after the moves
- Apply edits back-to-front to preserve byte offsets
- CLI interface for batch rewriting files

Does NOT:
- Format code (that's `formatter/`, which calls `rewrite_file` first)
- Understand scoping or bindings, except the reads the def placement, move
  and letrec plans check

## Interface

| Type / Function | Purpose |
|-----------------|---------|
| `Edit` | Byte-range replacement: offset, length, new text; `Edit::replace` and `Edit::insert` build one |
| `apply_edits` | Apply edits to source (sorts back-to-front, errors on overlap) |
| `SourceText` | Source text, its name, and the lexicon that tokenizes it |
| `RewriteRule` | Trait: examine a token, optionally produce an Edit |
| `RenameSymbol` | Data-driven symbol rename from HashMap |
| `collect_edits` | Lex a `SourceText` and collect the edits a set of rules makes |
| `collect_lexical_edits` | Respell tokens between two lexicons, by byte span |
| `fold_module` | The epoch 16 fold over rewritten text: new text, and one rewrite or none |
| `place_defs` | The epoch 17 assigns and def placement over folded text: new text, defs assigned or moved, notes |
| `move_defs` | The epoch 18 splices and moves over the placed text: new text, begins spliced and defs moved; notes move with the text |
| `bind_forward` | The epoch 18 letrec pass over the moved text: new text, defs bound, notes |
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
fold_module(new source) → read the tree, plan, edit → folded text
    │
    ▼
place_defs(folded text) → read the tree, assign, read again, plan, edit → placed text and notes
    │
    ▼
move_defs(placed text) → read the tree, splice, read again, order each body, edit → moved text
    │
    ▼
bind_forward(moved text) → read the tree, plan, edit → final text and notes
```

Every token pass reads its tokens through `SourceText`, never through a bare
`Lexer`. A file written before a token-level change tokenizes under its own
epoch's rules, and one pass reaching for the current epoch's would silently
disagree with the others ([lexicon](../../docs/impl/lexicon.md)). The fold
and the def placement, move and letrec passes read text that is already in the
current epoch's spelling.
