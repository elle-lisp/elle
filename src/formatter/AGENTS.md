# formatter

<!-- audited: 2026-10-25 -->

Opinionated code formatter for Elle source. Wadler-style pretty printing
with column-aware alignment via `Align`. One canonical style. Zero
configuration beyond line width and indent width.

The layout each form gets is in [the formatter rules](../../docs/fmt.md). This
file covers how the module produces it.

## Responsibility

- Format Elle source code
- Apply form-specific formatting rules (defn, let, if, cond, etc.)
- Preserve comments and blank lines via trivia attachment
- Produce idempotent output (format(format(x)) == format(x))
- Column enforcement in `--check` mode (warn 60, error 80)

Does NOT:
- Parse code (uses `reader` SyntaxReader)
- Validate code (just formats)
- Modify file system (caller handles I/O)

## Architecture

```
Source → strip shebang → epoch prescan
       → lex_for_format (separate tokens + comments)
       → parse to Syntax → collect trivia → attach trivia
       → generate Doc → render → strip leading newline
       → prepend shebang + trailing newline
```

Five phases:
1. **Lex with comments** (`comments.rs`): separates regular tokens from comment tokens
2. **Parse** (`SyntaxReader`): regular tokens → Syntax tree
3. **Trivia** (`trivia.rs`): merge comments + blank lines → attach to Syntax nodes
4. **Doc generation** (`format.rs` + `forms/`): walk AnnotatedSyntax → Doc tree
5. **Render** (`render.rs`): Doc tree → string with optimal line breaks

## Doc algebra

The formatter uses a Wadler-style document algebra extended with
column-aware alignment:

| Variant | Flat | Broken |
|---------|------|--------|
| `Empty` | nothing | nothing |
| `Text(s)` | literal string | literal string |
| `Concat(ds)` | sequence | sequence |
| `Nest(n, d)` | no effect | indent += n * indent_width |
| `Break` | space | newline + indent |
| `Group(d)` | try flat; break if too wide | — |
| `HardBreak` | newline (forces Group to break) | newline + indent |
| `CommentBreak` | like HardBreak | absorbed by adjacent HardBreak/Break |
| `FreshLine` | newline unless at a line start (forces Group to break) | same |
| `BlankLine` | newline at a line start, else nothing; measures zero | same |
| `Align(d)` | no effect | sets indent = current column |

**Indent is tracked in absolute columns** (not indent levels).
`Nest(n)` adds `n * indent_width` to the indent. `Align` sets indent
to the current cursor column, enabling columnar alignment that works
correctly for inline-nested forms.

**CommentBreak** solves the double-newline idempotency problem: comments
extend to end-of-line and need a newline after them, but the inter-sibling
HardBreak also produces a newline. A HardBreak, a broken Break or another
CommentBreak right after a CommentBreak adds no second newline.

**FreshLine** opens a node's leading comments. A form handler may join a
child to the text before it with a plain space, and FreshLine moves the
child's first comment to a line of its own there. Where a separator has
already started the line, it does nothing.

**BlankLine** stands for one blank line of the input. It measures zero, so it
never decides whether a Group breaks, and it prints only where the layout has
already started a line.

## Interface

| Type | Purpose |
|------|---------|
| `FormatterConfig` | Style configuration (indent_width, line_length) |
| `format_code(src, config)` | Format source string → `Result<String, String>` |

## Usage

```rust
use elle::formatter::{format_code, FormatterConfig};

let config = FormatterConfig::default();
let formatted = format_code(source, &config)?;
```

## Dispatch table

The formatter dispatches on the head symbol of list forms:

| Head | Handler |
|------|---------|
| `def`, `defn` | `format_def`, which hands a form with a parameter vector to `format_defn` |
| `fn` | `format_fn` |
| `let`, `let*`, `letrec` | `format_let` |
| `if` | `format_if` |
| `cond` | `format_cond` |
| `match` | `format_match` |
| `case` | `format_case` |
| `while` | `format_while` |
| `defmacro` | `format_defmacro` → `format_defn` |
| `begin`, `do`, `defer` | `format_begin` |
| `forever` | `format_forever` |
| `block` | `format_block` |
| `parameterize` | `format_parameterize` |
| `->`, `->>`, `some->`, `some->>` | `format_threading` |
| `when`, `unless` | `format_when` |
| `and`, `or`, `not`, `emit` | `format_generic_call` |
| `each` | `format_each` |
| `try`, `protect` | `format_try` |
| `assign` | `format_assign` |
| *other* | `format_generic_call` |

A handler whose arity guard fails falls back to `format_generic_call`.

## Alignment strategy

A generic call keeps its first argument beside the head, and `Align` captures
that argument's column for the rest. The renderer opens no new alignment point
past half the line width; a Break there keeps the enclosing indent, so deep
nesting cannot march off the right edge.

Forms like `fn`, `if` (compound), and struct/collection literals use `Align`
to ensure their content indents relative to the form's actual column position,
not the Nest level. This is critical for forms nested inline inside bindings
or arguments.

## Dependents

- `lsp/formatting.rs` — document formatting
- CLI — `elle fmt` command

## Invariants

1. **Trivia is pre-attached.** The attachment pass runs before the Doc walk.
   The Doc generator is a pure function from AnnotatedSyntax → Doc with no
   mutable state.

2. **Trivia inside string spans is skipped.** Blank lines inside multi-line
   string literals are not trivia. The attachment pass advances past them.

3. **Shebang is stripped once.** `strip_shebang()` produces a single stripped
   source that both the parser and trivia collector use, keeping byte offsets
   consistent. It measures the line with `reader::shebang_len`, so it agrees
   with the reader and the rewriter about where Elle's text begins.
   `format_code` then prescans that stripped source and hands
   `lex_for_format` the rules of the epoch it declares
   ([lexicon](../../docs/impl/lexicon.md)). Comment text is emitted verbatim,
   so the spelling that arrived is the spelling that leaves.

4. **String literals are source-sliced.** `SyntaxKind::String(s)` stores the
   unescaped value. The formatter slices `source[span.start..span.end]` to
   preserve the raw literal including quotes and escapes.

5. **Output always ends with `\n`.** The entry point enforces a trailing newline.

6. **Formatting is idempotent.** `format(format(x)) == format(x)` is a
   testable invariant with dedicated tests for every special form.

7. **Indent is absolute columns.** The renderer tracks indent as a column
   count (number of spaces), not as a multiple of indent_width. Nest adds
   `n * indent_width`; Align sets indent to the current column.

8. **A CommentBreak is absorbed by the break after it.** A HardBreak or a
   broken Break right after a CommentBreak emits nothing. Both must be at the
   same Nest level to avoid indent mismatch.

9. **No layout decision reads a blank line.** Every blank line of the input
   reaches the Doc as a BlankLine, never as a HardBreak. The layout of a file
   with its blank lines removed is then the layout of the file itself, which
   is what keeps a second pass from moving anything.

10. **An own-line comment stays on its own line.** The attachment pass makes
    it leading trivia of the next sibling. After the last child it trails that
    child, and the formatter still gives it a line of its own, with the
    closing delimiter below it.
