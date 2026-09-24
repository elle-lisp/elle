# Lexicon: epoch-aware lexing

<!-- audited: 2026-09-23 -->

An epoch selects the lexer rules that tokenize a file, so a breaking change can
reach below the syntax tree to the tokens themselves.

Epoch 13 is the first epoch whose lexicon differs from its predecessor's. It
changes which escapes a string literal accepts (see
[../epochs.md](../epochs.md)).

Epochs rewrite parsed syntax trees (see [../epochs.md](../epochs.md)). This
document extends the epoch system down one level, to the lexer, so that an
epoch bump can change tokenization itself. It owns the argument for the
mechanism and the alternatives it beat.

## Problem

The migration pass runs after the reader:

```
Source → Reader → [epoch migration] → Expander → HIR → LIR → Bytecode
```

Every `MigrationRule` operates on `Syntax` trees, so the current lexer must
tokenize a file before the file's epoch can help it. A lexical change — a
different comment character, a removed reader token — breaks that assumption.
An old file misreads before migration ever runs, and the `(elle/epoch N)`
promise stops at the token boundary.

The failure can be silent. This text is legal under both a splice-`;` grammar
and a comment-`;` grammar, with different values:

```text
[1 ;xs
 2]
```

Under splice, `xs` spreads into the array. Under comment, the rest of the
line disappears. No error fires in either reading. The epoch must therefore
be *declared*, never inferred from whether the file happens to parse.

## Design

Two additions: a frozen **epoch prescan** that finds the declaration before
lexing, and a **`Lexicon`** value that carries the epoch-gated lexer rules.

```
Source → [shebang strip] → [epoch prescan] → Lexer(Lexicon) → Parser
       → [epoch tree migration] → Expander → …
```

The reader entry points in `src/reader/mod.rs` (`read_str`, `lex_all`, and
everything built on them) prescan internally and build the lexicon
themselves. The public reader API does not change; the formatter, LSP, lint,
`elle rewrite`, and embedders keep their current signatures.

## The epoch prescan

The prescan answers one question — which lexicon tokenizes this source — with
a fixed micro-grammar that no future epoch may change:

1. If the source starts with `#!`, skip the first line (the reader already
   strips shebangs before lexing).
2. Skip whitespace.
3. Match the literal shape `(elle/epoch <digits>)`: the character `(`,
   optional whitespace, the symbol `elle/epoch`, whitespace, decimal digits,
   optional whitespace, `)`.
4. On a match, the digits name the epoch. On anything else — including a
   comment in either style — the source targets `CURRENT_EPOCH`.

The frozen subset is the whole trick. The declaration form contains only
characters whose lexical meaning is identical in every epoch, past and
future, so the prescan needs no epoch to read it. This invariant binds all
future epochs: no epoch may change the meaning of `(`, `)`, whitespace,
decimal digits, or the constituent characters of the symbol `elle/epoch` in
the positions the prescan examines.

`extract_epoch` (`src/epoch/mod.rs`) stays the authority for the epoch
number: it still validates the declaration on the parsed tree, consumes it,
and rejects duplicates. The prescan only selects the lexicon. The two must
agree where it matters:

- If `Lexicon::for_epoch(declared) == Lexicon::for_epoch(prescanned)`, any
  disagreement is harmless and allowed. A file that declares the current
  epoch below a comment therefore still reads. Epochs 0 to 12 share one
  lexicon, and epoch 13 has another, so a file that declares epoch 12 or
  earlier must put the declaration first.
- If the two lexicons differ, compilation fails with an error telling the
  author to move the declaration above everything except the shebang. A
  declaration the prescan cannot see cannot have selected the lexer that read
  it, so the file's reading is unreliable by construction.

The pipeline runs this check immediately after the read, while it still holds
both the source text and the tree read from it (`src/pipeline/compile.rs` and
`src/pipeline/compile/frontend.rs`). The comparison is over lexicons, never
over epoch numbers: comparing the numbers would reject every file that carries
a comment above its declaration.

## The Lexicon

`Lexicon` is a struct of the epoch-gated lexer rules — the comment
introducer, whether `;` lexes as `Splice`, whether `,;` fuses into
`UnquoteSplicing`, and which escapes a string literal accepts.
`Lexicon::for_epoch(n)` builds it, and it lives in `src/epoch/rules.rs` next
to the `MigrationRule` tables, so one file describes everything an epoch
changed.

The string escapes are a `StringEscapes` value (`src/reader/escape.rs`).
Its one decoder reads the escape that follows a backslash, so the lexer and
the respelling below cannot disagree about what an escape means. `Lenient`
is the rule of epochs 0 to 12: five escapes, and any other escape drops its
backslash. `Strict` is the rule of epoch 13: the same five, `\0`, `\xHH` up
to `7f` and `\u{…}`, and any other escape is a read error.

The lexer (`src/reader/lexer.rs`) holds a `Lexicon` and consults it in the
affected match arms instead of hard-coding the rules. A lexical change adds
the `Lexicon` field that names the behavior, and flips it in the `for_epoch`
table. The lexer and `respell` read the field; neither compares epochs.

`Lexicon::respell` reads the same fields in the other direction: given a
token lexed under one lexicon, it returns the text that spells the same
token under another, or nothing when the two spell it alike. That single
method is what `elle rewrite` migrates tokens with, so the rules and the
rewrite of the rules cannot drift apart.

`respell` takes the token's source text as well as the token. A string token
carries the decoded string, and the spelling of its escapes is gone from it.
The respelling walks the source text escape by escape. It keeps each escape
that both lexicons decode alike. It writes each other escape as the character
the source lexicon read from it, spelled the way the printer spells a string
(see "Printing a string" below). Under that rule `"\x41\n"`, read under epoch
12, becomes `"x41\n"`.

Each lexical change also gets a `LexicalChange` descriptor (a name and a
summary) in the epoch's `Migration` entry, so `elle rewrite --list-rules` and
the epoch history in [../epochs.md](../epochs.md) can enumerate token-level
changes alongside tree rules. A test pins the pairing in both directions: an
epoch declares a `LexicalChange` exactly when its lexicon differs from its
predecessor's.

## Scope

One source unit, one epoch: the prescanned epoch governs every byte of the
unit. The declaration sits above everything except a shebang and whitespace.

| Source | Epoch resolution |
|--------|------------------|
| `.lisp` file | prescan |
| Literate `.md` | prescan of the stripped text; the declaration is the first form of the first fence |
| stdin program | prescan |
| REPL input | always current epoch (`read_syntax_all_current`) |
| `eval` / `read` of a string | prescan (a data string without a declaration is current-epoch) |
| stdlib / prelude | must be current-epoch text (unchanged rule; the WASM backend concatenates stdlib without migration) |
| `.lua`, `.js`, `.py` syntax modes | out of scope — those lexers do not consult the lexicon |

`MigrationRule::Replace` templates are current-epoch syntax and are lexed
with the current lexicon, as today.

## Tooling

**`elle rewrite`** lexes a file under its own epoch's lexicon — every pass it
runs, not only the new one — and applies a token-level pass alongside the
tree rules. `Lexicon::respell` gives each token its current-epoch spelling,
and the rewriter applies the difference in place by byte span. Token spans
make this formatting-preserving by construction, matching the tool's existing
contract. `--check`, `--dry-run`, and `--list-rules` cover lexical changes
the same way they cover tree rules.

Two shapes fall outside a respelling. The shebang line is not Elle text, so
no token inside it is respelled, however the lexer happened to tokenize it.
And a token the target lexicon cannot spell at all is not a byte-span
rewrite: `respell` refuses it, the rewriter names the position, and the
epoch that removed the spelling carries a `MigrationRule::Desugar` for it.

## Desugaring a reader shorthand

A reader shorthand is a prefix token that wraps the form written after it.
`;expr` stands for `(splice expr)`, and `,;expr` stands for
`(unquote-splicing expr)`. An epoch can take the spelling away without
touching the form, so the migration replaces the prefix with the form.

`respell` cannot express that rewrite. It answers about one token, and the
replacement has to reach past the prefix to the end of the form the prefix
wraps. The token stream gives up that extent only by walking it.

`MigrationRule::Desugar { shorthand }` names the prefix token, and nothing
else: `Token::shorthand_form` already says which form each shorthand stands
for, because the reader decided it. One authority, so no epoch can pair a
shorthand with a form the reader never built.

`collect_desugar_edits` walks one balanced form and emits two edits. The
first runs from the prefix to the start of the form and becomes `(<form> `,
which absorbs any whitespace the author left between them. The second
inserts `)` after the form. Two small edits rather than one span over the
whole shorthand, so a shorthand nested inside another shorthand's form
rewrites on its own instead of overlapping the outer edit.

The compiler's tree transform has no arm for this rule. The shorthand and
the form read to the same tree, so an old file already compiles: `;x` reads
to `SyntaxKind::Splice`, and `Analyzer::unwrap_splice` accepts `(splice x)`
in every position that node reaches. Only the file's text needs migrating,
and text is `elle rewrite`'s job.

That agreement is the condition on the rule, and an epoch must check it
before declaring one. A `Desugar` whose form does not read back to the same
tree changes what programs mean, which is a language change, not a
migration.

`,;` does not meet the condition today. `quasiquote_list_to_code`
(`src/syntax/expand/quasiquote.rs`) matches `SyntaxKind::UnquoteSplicing`,
the node the reader builds from the fused token. To that pass the list form
`(unquote-splicing x)` is an ordinary list, quoted as data instead of
spliced. A rule for `,;` would therefore compile and silently change what a
macro expands to. The epoch that removes `,;` teaches the expander the list
form first.

**`elle fmt`** runs the epoch rewrite before it formats, unless `--no-epoch`
skips it (`src/formatter/run.rs`), so the file it writes is current-epoch text
under a current tag. It lexes its input under the epoch that input declares,
exactly as `elle rewrite` does. It emits comment text verbatim, so a file
formatted with `--no-epoch` keeps the spelling it arrived with.

**Diagnostics.** When lexing fails at text that an older lexicon reads, the
error names the newest such epoch N and appends: "if this file targets epoch
N or earlier, declare `(elle/epoch N)` as its first form, then run `elle
rewrite`". The declaration restores the old reading, and the rewrite then
migrates the file to the current epoch. This is the loud path for untagged
old files; the declaration requirement exists for the silent ones. A string
escape is the one rule that differs today, so the escape error is the one
error that carries the hint.

## Printing a string

`Syntax`'s `Display` prints a string node, and so does the formatter when a
node has no source text to copy. The printed text is read again, and not
always under the lexicon that read the original. An epoch migration template
reads it under the current lexicon. The WASM backend's include splice reads
an included file's forms under the lexicon of the file that included it.

So the printer writes only the escapes every lexicon reads — `\n`, `\t`,
`\r`, `\\` and `\"` — and writes every other character as itself. Inside a
string, every lexicon takes any other character literally, so the printed
text reads back to the same string under each of them. One function in
`src/reader/escape.rs` does this for both printers.

## Alternatives considered

**Infer the grammar (try current, retry older on failure).** Beaten by the
silent-divergence example above: both grammars accept overlapping texts with
different meanings, so failure is not a reliable signal. Rejected outright.

**A superset lexer that defers the decision to the parser.** Tokenization
cannot be deferred: a comment consumes to end of line during lexing, so the
two grammars produce different token streams, not one stream with two
readings.

**Out-of-band declaration (file extension, manifest).** Elle files must be
self-describing: `elle script.lisp` has no manifest, and files travel through
pipes, fences, and transcripts without their directory. Rust editions live in
`Cargo.toml` because cargo owns every build; Elle has no such owner.

**Flag day with a converter, no compatibility.** Breaks the epoch promise
for external corpora, and the promise is the epoch system's whole point.

Prior art: Racket's `#lang` line is the same shape — a frozen prefix, read
before the body, that selects the reader for the rest of the file.

## Invariants

- The prescan micro-grammar never changes. The characters it examines keep
  their lexical meaning in every epoch.
- One declaration per source unit, above everything except a shebang and
  whitespace, when the unit crosses a lexical boundary.
- The prescan selects the lexicon; `extract_epoch` owns the number. If their
  lexicons differ, compilation fails.
- All lexical differences between epochs live in `Lexicon::for_epoch`. The
  lexer contains no bare epoch comparisons.
- A source unit's lexicon comes from its own text. The REPL is the single
  exception: prompt input is always current-epoch, so a pasted declaration
  cannot change how the prompt lexes.

## Where the pieces live

| Piece | Home |
|-------|------|
| The prescan | `src/epoch/mod.rs::prescan_epoch` |
| The lexicon and its `respell` | `src/epoch/rules.rs` |
| The string escapes, their respelling, and the printer's spelling | `src/reader/escape.rs` |
| The lexer's consultation of it | `src/reader/lexer.rs::in_lexicon` |
| Reader entry points that prescan | `src/reader/mod.rs` |
| The REPL's current-epoch entry point | `src/reader/mod.rs::read_syntax_all_current` |
| The mismatch check | `src/epoch/mod.rs::check_lexicon_agreement` |
| The token-level rewrite pass | `src/rewrite/run.rs::collect_lexical_edits` |
| The shorthand desugar pass | `src/rewrite/run/edits.rs::collect_desugar_edits` |

Epoch 13 reaches the prescan, the mismatch check, the string respelling and
the diagnostic through registered epochs, and its tests go through
`Lexicon::for_epoch`. No registered epoch changes the comment character or
the splice, and no epoch declares a `Desugar` rule yet, so those paths cannot
be reached through `Lexicon::for_epoch` or `MIGRATIONS`. Tests build the
differing pair and the rule directly — `Lexicon::divergent`,
`Lexicon::no_semicolon`, a rule slice passed to `collect_desugar_edits` —
rather than leave those paths to run for the first time on the epoch that
needs them. The comment/splice swap proposed in #983 reaches them through a
registered epoch, and lands with its own migration tests.
