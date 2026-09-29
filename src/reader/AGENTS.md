# reader

<!-- audited: 2026-09-28 -->

Lexing and parsing: source text becomes `Syntax` trees, or `Value` trees for an embedder.

[docs/impl/reader.md](../../docs/impl/reader.md) holds the design, and
[docs/impl/lexicon.md](../../docs/impl/lexicon.md) holds the epoch-gated
lexicon.

## Responsibility

- Tokenize source text
- Parse tokens into tree structures
- Track source locations for error reporting
- Handle shebang lines

Does NOT:
- Expand macros (that's `syntax/expand.rs`)
- Resolve bindings (that's `hir`)
- Intern symbols (caller provides `SymbolTable`)

## Interface

| Type | Purpose |
|------|---------|
| `Lexer` | Tokenizes input string under a `Lexicon` |
| `Token`, `OwnedToken` | The borrowed and owned token sets, generated from one declaration in [token.rs](token.rs) |
| `SourceLoc` | File, line and column |
| `Reader` | Parses tokens to `Value`; `read_str` is its one caller in `src/` |
| `SyntaxReader` | Parses tokens to `Syntax` |

## Entry points

```rust
// Parse to Value, for an embedder. Nothing in src/ calls it.
let value = read_str(source, runtime.heap(), &mut symbols)?;

// Parse one form to Syntax, born in `arena`
let syntax = read_syntax(arena, source, source_name)?;

// Parse multiple forms
let forms = read_syntax_all(arena, source, source_name)?;

// Parse multiple forms, dispatching on the file extension
let forms = read_syntax_all_for(arena, source, source_name)?;

// Parse multiple forms under the current epoch, whatever the text declares
let forms = read_syntax_all_current(arena, source, source_name)?;
```

Each entry point prescans the source for its `(elle/epoch N)` declaration
and lexes under `Lexicon::for_epoch(N)`. `prescanned_epoch_for` reports that
choice without reading, so the pipeline can check it against the declaration
in the tree. `read_syntax_all_current` is the one exception, and the REPL is
its one caller: prompt input is always current-epoch, so a pasted declaration
cannot change how the prompt lexes.

`shebang_len` gives the byte length of a leading `#!` line. Everything that
translates between original-source offsets and lexer offsets measures it
here.

## Data flow

```
Source string
    │
    ├─► prescan_epoch() → Lexicon::for_epoch(n)
    │
    ▼
Lexer::with_file(source, name).in_lexicon(lexicon)
    │
    ├─► next_token_with_loc() → Token + SourceLoc
    │
    ▼
Collect all tokens
    │
    ▼
SyntaxReader / Reader
    │
    ▼
Syntax / Value tree
```

## Dependents

- `pipeline/` - compiles through `read_syntax`, `read_syntax_all` and `read_syntax_all_for`
- `repl/read.rs` - reads prompt input through `read_syntax_all_current`
- `primitives/read.rs` - the `read` primitive

## Delimiters

The lexer recognizes these delimiters (characters that cannot appear in symbol names):

| Delimiter | Token | Purpose |
|-----------|-------|---------|
| `(` `)` | `LeftParen`, `RightParen` | List forms |
| `[` `]` | `LeftBracket`, `RightBracket` | Array literals (immutable) |
| `{` `}` | `LeftBrace`, `RightBrace` | Struct literals (immutable) |
| `\|` | `Pipe` | Set literal delimiter |
| `@\|` | `AtPipe` | @set literal prefix (mutable) |
| `@` | `ListSugar` | Before `[`, `{` or `"`: the mutable form of that literal |
| `b[` | `BytesBracket` | Bytes literal |
| `@b[` | `AtBytesBracket` | @bytes literal (mutable) |
| `'` | `Quote` | Quote reader macro |
| `` ` `` | `Quasiquote` | Quasiquote reader macro |
| `,` | `Unquote` | Unquote reader macro (inside quasiquote) |
| `,;` | `UnquoteSplicing` | Unquote-splicing reader macro |
| `;` | `Splice` | Splice reader macro |
| `#` | `Comment` | Line comment |

A `:` that starts a token makes a `Keyword`; it has no token of its own.

## Keyword syntax

Keywords are prefixed with `:`. The lexer supports `:@name` syntax for mutable type keywords:
- `:set` — immutable set type keyword
- `:@set` — mutable set type keyword
- `:@array` — mutable @array type keyword
- `:@string` — mutable @string type keyword

The `@` in `:@name` is consumed by the lexer and prepended to the keyword name.

## Set literals

- `|...|` reads as `SyntaxKind::Set(RegionSlice<Syntax>)` — immutable set literal
- `@|...|` reads as `SyntaxKind::SetMut(RegionSlice<Syntax>)` — mutable set literal
- Inside any collection, a bare `|` starts a nested set literal, producing a
  `SyntaxKind::Set` node. `|` is purely a set delimiter in all contexts.

## Invariants

1. **Shebang lines are stripped.** `#!` at start of input is ignored.

2. **Empty input to a one-form entry point is an error.** `read_str` and
   `read_syntax` return `Err("No input")`, not `Ok(Nil)`. The `_all` entry
   points return an empty `Vec`.

3. **`SourceLoc` is 1-indexed.** Line 1, column 1 is the first character.

4. **`SyntaxReader` checks for trailing tokens.** Use `check_exhausted()`
   to detect garbage after the expression.

5. **Qualified symbols are single tokens.** `module:name` is lexed as one
   token, not three. The Analyzer desugars qualified symbols to nested
   `get` calls during analysis.

6. **`|` is a delimiter for set literals.** `|1 2 3|` is lexed as `Pipe`, elements,
   `Pipe` (for immutable sets). `@|1 2 3|` is lexed as `AtPipe`, elements, `Pipe`
   (for mutable sets). It cannot appear in symbol names.

7. **`:@name` keywords are valid.** The lexer recognizes `:@` as a keyword
   prefix variant. The `@` is consumed and prepended to the keyword name.

8. **The epoch declaration selects the lexicon.** `(elle/epoch N)` at the top
   of a source unit decides how that unit tokenizes, before any token is
   produced. An epoch this compiler has no lexicon for is a read error, not a
   parse that guesses.

9. **Comments are tokens.** The lexer emits `#` line comments as
   `Token::Comment(String)`. Both `SyntaxReader` and `Reader` skip them, so
   they do not appear in the output tree. The formatter collects them through
   `lex_for_format()` in
   [formatter/comments.rs](../formatter/comments.rs).

10. **A collection that fails leaves no open form behind.** The unterminated
    message names the outermost collection still open, so a reader that
    reads again after an error must not count the failed one.
