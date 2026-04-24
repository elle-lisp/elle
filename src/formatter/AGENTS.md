# formatter

Opinionated code formatter for Elle source. Wadler-style pretty printing
with column-aware alignment via `Align`. One canonical style. Zero
configuration beyond line width and indent width.

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
Source → strip shebang → lex_for_format (separate tokens + comments)
       → parse to Syntax → collect trivia → attach trivia
       → generate Doc → render → strip leading newline
       → prepend shebang + trailing newline
```

Five phases:
1. **Lex with comments** (`comments.rs`): separates regular tokens from comment tokens
2. **Parse** (`SyntaxReader`): regular tokens → Syntax tree
3. **Trivia** (`trivia.rs`): merge comments + blank lines → attach to Syntax nodes
4. **Doc generation** (`format.rs` + `forms.rs`): walk AnnotatedSyntax → Doc tree
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
| `Align(d)` | no effect | sets indent = current column |

**Indent is tracked in absolute columns** (not indent levels).
`Nest(n)` adds `n * indent_width` to the indent. `Align` sets indent
to the current cursor column, enabling columnar alignment that works
correctly for inline-nested forms.

**CommentBreak** solves the double-newline idempotency problem: comments
extend to end-of-line and need a newline after them, but the inter-sibling
HardBreak also produces a newline. CommentBreak is absorbed by any
adjacent newline-producing variant (HardBreak, Break, SoftBreak, BreakTo).

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

| Head | Handler | Behavior |
|------|---------|----------|
| `def` | `format_def` | Inline if fits |
| `defn` | `format_defn` | Always break before body |
| `fn` | `format_fn` | Single body: try inline; multi: break. Align-wrapped. |
| `let`, `let*`, `letrec` | `format_let` | One binding pair per line, exact column alignment |
| `if` | `format_if` | Trivial: inline. Compound: Align-wrapped, +2 body |
| `cond` | `format_cond` | Flat pairs `(cond test body ...)` |
| `match` | `format_match` | Flat pairs `(match expr pat body ...)` |
| `case` | `format_case` | Flat pairs `(case expr key result ...)` |
| `while` | `format_while` | Break if multi-expression body |
| `defmacro` | `format_defmacro` → `format_defn` | Same as defn |
| `begin` | `format_begin` | Always break, +2 body |
| `forever` | `format_forever` | Single body: try inline. Multi: break like begin |
| `block` | `format_block` | Like begin, `:name` stays on block line |
| `parameterize` | `format_parameterize` | Bindings Align-wrapped, one per line |
| `->`, `->>`, `some->`, `some->>` | `format_threading` | Always break |
| `when`, `unless` | `format_when` | Trivial: group. Compound: +2 body |
| `and`, `or`, `not`, `emit` | `format_generic_call` | Columnar via generic call |
| `each` | `format_each` | Header on one line, `in` optional, body +2 |
| `try`, `protect` | `format_try` | Always break (same as begin) |
| `assign` | `format_assign` | Inline if fits |
| *other* | `format_generic_call` | Short head: Align columnar. Long head: +2 fallback |

## Alignment strategy

Generic calls use two strategies based on head width:

- **Short head** (`first_arg_col <= line_length / 4`): `Align` captures the
  first arg's column. Subsequent args align to that column via `Break`.
- **Long head**: fall back to `nest(1)` (+2 indent).

Forms like `fn`, `if` (compound), and struct/collection literals use `Align`
to ensure their content indents relative to the form's actual column position,
not the Nest level. This is critical for forms nested inline inside bindings
or arguments.

## Dependents

- `lsp/formatting.rs` — document formatting
- CLI — `elle fmt` command

## Files

| File | Content |
|------|---------|
| `mod.rs` | Module declarations and re-exports |
| `config.rs` | `FormatterConfig` with indent_width and line_length |
| `core.rs` | Entry point `format_code()`, pipeline orchestration, idempotency tests |
| `format.rs` | AnnotatedSyntax → Doc walk, trivia emission, collection/struct formatting |
| `forms.rs` | Per-special-form formatting rules, generic call |
| `doc.rs` | Doc algebra (Empty, Text, Concat, Nest, Break, Group, HardBreak, CommentBreak, Align) |
| `render.rs` | Doc → String renderer with absolute-column indent tracking |
| `comments.rs` | CommentMap, lex_for_format(), strip_shebang() |
| `trivia.rs` | Trivia types, collection, attachment pass |
| `run.rs` | CLI entry point for `elle fmt`, --check column enforcement |

## Invariants

1. **Trivia is pre-attached.** The attachment pass runs before the Doc walk.
   The Doc generator is a pure function from AnnotatedSyntax → Doc with no
   mutable state.

2. **Trivia inside string spans is skipped.** Blank lines inside multi-line
   string literals are not trivia. The attachment pass advances past them.

3. **Shebang is stripped once.** `strip_shebang()` produces a single stripped
   source that both the parser and trivia collector use, keeping byte offsets
   consistent.

4. **String literals are source-sliced.** `SyntaxKind::String(s)` stores the
   unescaped value. The formatter slices `source[span.start..span.end]` to
   preserve the raw literal including quotes and escapes.

5. **Output always ends with `\n`.** The entry point enforces a trailing newline.

6. **Formatting is idempotent.** `format(format(x)) == format(x)` is a
   testable invariant with dedicated tests for every special form.

7. **Indent is absolute columns.** The renderer tracks indent as a column
   count (number of spaces), not as a multiple of indent_width. Nest adds
   `n * indent_width`; Align sets indent to the current column.

8. **CommentBreak absorption is symmetric.** Any newline-producing variant
   (HardBreak, Break) absorbs a preceding CommentBreak. Both must be at the
   same Nest level to avoid indent mismatch.
