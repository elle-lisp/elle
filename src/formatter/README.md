# Formatter

<!-- audited: 2026-10-25 -->

How `elle fmt` turns source into a Doc tree and renders it, and what keeps a
second pass from moving anything.

[The formatter rules](../../docs/fmt.md) own the layout each form gets and the
command line. This document covers how the module produces that layout.

## The pipeline

`format_code` strips the shebang once, so the lexer, the parser and the trivia
scan agree on every byte offset. It measures the shebang with
`reader::shebang_len`, as the reader and the rewriter do. It then prescans the
stripped source for its epoch and lexes under the rules that epoch declares
([lexicon](../../docs/impl/lexicon.md)). The lexer hands comments aside, and
`SyntaxReader` parses the remaining tokens.

`collect_trivia` merges the comments with the runs of blank lines it scans
from the source. One attachment pass then hangs every trivia item on a node,
and the Doc walk after it is a pure function of the annotated tree. Last, the
renderer lays the Doc out, and `format_code` trims the leading newline and the
trailing spaces of every line, and ends the output with one newline.

The formatter copies the text of a number or a string literal from the source,
so escapes, quotes and spellings such as `0xff` leave as they arrived. Comment
text leaves verbatim too.

## Attachment

A node holds three kinds of trivia. Its leading trivia is everything between
the previous sibling and the node. Its trailing trivia is a comment on the line
where the node ends. Its closing trivia exists on a last child only: the own-line
comments and blank lines between that child and the parent's closing
delimiter.

So an own-line comment is always the leading trivia of the node below it, or
the closing trivia of the last node above it. The attachment pass drops blank
lines that no comment follows from the closing trivia, and from the trivia
after the last top-level form. Blank lines inside a multi-line string are not
trivia, and the pass steps over them.

## The Doc algebra

The Doc algebra is Wadler's, with `Align` for columns and four variants for
comments and blank lines.

| Variant | Flat | Broken |
|---------|------|--------|
| `Empty` | nothing | nothing |
| `Text(s)` | literal string | literal string |
| `Concat(ds)` | sequence | sequence |
| `Nest(n, d)` | no effect | indent += n * indent_width |
| `Break` | space | newline + indent |
| `Space` | space, absorbed right after a CommentBreak | same |
| `Group(d)` | try flat; break if too wide | — |
| `HardBreak` | newline (forces Group to break) | newline + indent |
| `CommentBreak` | like HardBreak | absorbs the separator after it |
| `FreshLine` | newline unless at a line start (forces Group to break) | same |
| `BlankLine` | newline at a line start, else nothing; measures zero | same |
| `Align(d)` | no effect | sets indent = current column |

The renderer tracks indent in absolute columns. `Nest(n)` adds `n *
indent_width`, and `Align` sets the indent to the cursor's column, so a form
nested inline inside a binding or an argument indents from its own position.
Past half the line width `Align` opens no new alignment point, and deep nesting
keeps the enclosing indent instead of marching off the right edge.

A comment runs to the end of its line, so a CommentBreak follows each inline
comment. The separator right after it, whether a HardBreak, a Break or a
Space, then adds nothing, because the CommentBreak has already ended the line.
The next element therefore starts at the indent in force where the comment
ended. A handler places a child's trailing trivia at the Nest level that the
next element indents to, and it joins two elements with a Space, never with a
literal `" "`.

A FreshLine opens every leading list that holds a comment. A handler may join
a child to the text before it with a Space, and the FreshLine then moves the
comment onto a line of its own at the current indent. Where a separator has
already started the line, the FreshLine does nothing.

A BlankLine stands for one blank line of the input, at most two in a run. It
measures zero, so no Group decision reads it, and it prints only where the
layout has already started a line. A file with its blank lines removed
therefore gets the layout of the file itself, and that is what keeps a second
pass from moving anything.

## Handlers

`format_list` dispatches on the head symbol to a handler under `forms/`, and a
handler whose arity guard fails falls back to `format_generic_call`. A handler
receives the children with the head as `children[0]`, formats each child with
`format_annotated`, and returns the whole form, parentheses included. A handler
that formats a child's elements itself, as `let` does its bindings vector, also
emits that child's own leading and trailing trivia.

A header child sits inside the form's `Nest`, so a comment there indents +2
from the open paren. A key and its value go through `format_pair`, which puts
the value below its key when a comment comes between them.

## Interface

`format_code(source, &FormatterConfig)` returns the formatted text, or the
parse error. `FormatterConfig` holds the line width and the indent width, and
nothing else. [The language server](../lsp/formatting.rs) and the `elle fmt`
command in [run.rs](run.rs) are its callers. The column check of `--check`
reads the formatted text with the reader's lexer, in
[columns.rs](columns.rs).
