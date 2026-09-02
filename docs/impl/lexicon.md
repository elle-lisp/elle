# Lexicon: epoch-aware lexing

Status: the prescan (`prescan_epoch`) and the `Lexicon` seam are implemented
and tested; the reader entry points do not consult them yet. Issue #1020
tracks the remaining wiring. The first client is the comment/splice swap
proposed in issue #983.

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

```lisp
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
  disagreement is harmless and allowed. All epochs ≤ 12 share one lexicon, so
  every existing file — including a file with comments above its declaration
  — keeps working unchanged.
- If the two lexicons differ, compilation fails with an error telling the
  author to move the declaration above everything except the shebang. A
  declaration the prescan cannot see cannot have selected the lexer that read
  it, so the file's reading is unreliable by construction.

## The Lexicon

`Lexicon` is a struct of the epoch-gated lexer rules — the comment
introducer, whether `;` lexes as `Splice`, whether `,;` fuses into
`UnquoteSplicing`, the dispatch table behind `#`, and whatever later epochs
add. `Lexicon::for_epoch(n)` builds it, and it lives in `src/epoch/rules.rs`
next to the `MigrationRule` tables, so one file describes everything an epoch
changed.

The lexer (`src/reader/lexer.rs`) holds a `Lexicon` and consults it in the
affected match arms instead of hard-coding the rules. A lexical change
touches exactly two places: the `Lexicon` field that names the behavior, and
the `for_epoch` table that flips it.

Each lexical change also gets a `LexicalChange` descriptor (a name and a
summary) in the epoch's `Migration` entry, so `elle rewrite --list-rules` and
the epoch history in [../epochs.md](../epochs.md) can enumerate token-level
changes alongside tree rules.

## Scope

One source unit, one epoch: the prescanned epoch governs every byte of the
unit. The declaration sits above everything except a shebang and whitespace.

| Source | Epoch resolution |
|--------|------------------|
| `.lisp` file | prescan |
| Literate `.md` | prescan of the stripped text; the declaration is the first form of the first fence |
| stdin program | prescan |
| REPL input | always current epoch |
| `eval` / `read` of a string | prescan (a data string without a declaration is current-epoch) |
| stdlib / prelude | must be current-epoch text (unchanged rule; the WASM backend concatenates stdlib without migration) |
| `.lua`, `.js`, `.py` syntax modes | out of scope — those lexers do not consult the lexicon |

`MigrationRule::Replace` templates are current-epoch syntax and are lexed
with the current lexicon, as today.

## Tooling

**`elle rewrite`** gains a token-level pass that runs before the tree rules:
re-lex the file under its source-epoch lexicon, rewrite each changed token in
place by byte span, then apply tree rules and bump the tag. Token spans make
this formatting-preserving by construction, matching the tool's existing
contract. `--check`, `--dry-run`, and `--list-rules` cover lexical changes
the same way they cover tree rules.

**`elle fmt`** lexes under the file's declared epoch and re-emits under the
same epoch. Formatting never migrates; only `elle rewrite` does.

**Diagnostics.** When lexing or parsing fails at a token whose rule differs
in an older lexicon, the error appends: "if this file targets epoch ≤ N, add
`(elle/epoch N)` as its first form, or run `elle rewrite`". This is the
loud path for un-tagged old files; the declaration requirement exists for the
silent ones.

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

## Landing order

Documentation, then tests, then code. The prescan
(`src/epoch/mod.rs::prescan_epoch`), the `Lexicon` seam
(`src/epoch/rules.rs`), and the lexer's consultation of its lexicon are in
place, with tests pinning the prescan edges and the token-stream divergence.
What remains, in order:

1. Wire the prescan into the reader entry points (`read_str` and `lex_all`
   in `src/reader/mod.rs`): prescan the original source, then lex with
   `Lexer::with_lexicon(input, Lexicon::for_epoch(n))`.
2. The mismatch check after `extract_epoch` in `src/pipeline/compile.rs`:
   reject the file when the declared and prescanned epochs select
   different lexicons.
3. Update `detect_epoch_in_source` (`src/epoch/mod.rs`) to lex under the
   prescanned lexicon instead of the current one.
4. The `elle rewrite` token-level pass (`src/rewrite/`).
5. The first real lexical epoch (#983) exercises the mechanism end to end
   and lands with its own migration tests.
