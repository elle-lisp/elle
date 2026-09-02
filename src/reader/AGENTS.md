# reader

Lexing and parsing. Transforms source text into `Syntax` trees or `Value` trees.

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
| `Lexer` | Tokenizes input string under a `Lexicon` (epoch-gated rules, `docs/impl/lexicon.md`) |
| `Token` | Token variants (LParen, Int, Symbol, Pipe, AtPipe, etc.) |
| `SourceLoc` | Line/column position |
| `Reader` | Parses tokens to `Value` |
| `SyntaxReader` | Parses tokens to `Syntax` |

## Entry points

```rust
// Parse to Value (legacy)
let value = read_str(source, runtime.heap(), &mut symbols)?;

// Parse to Syntax (preferred)
let syntax = read_syntax(source, source_name)?;

// Parse multiple forms
let forms = read_syntax_all(source, source_name)?;

// Parse multiple forms under the current epoch, whatever the text declares
let forms = read_syntax_all_current(source, source_name)?;
```

Each entry point prescans the source for its `(elle/epoch N)` declaration
and lexes under `Lexicon::for_epoch(N)` (`docs/impl/lexicon.md`).
`prescanned_epoch_for` reports that choice without reading, so the pipeline
can check it against the declaration in the tree.
`read_syntax_all_current` is the one exception, and the REPL is its one
caller: prompt input is always current-epoch, so a pasted declaration
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

- `pipeline.rs` - uses `read_syntax`
- `repl.rs` - uses `read_str`
- `main.rs` - file execution

## Delimiters

The lexer recognizes these delimiters (characters that cannot appear in symbol names):

| Delimiter | Token | Purpose |
|-----------|-------|---------|
| `(` `)` | `LParen`, `RParen` | List forms |
| `[` `]` | `LBracket`, `RBracket` | Array literals (immutable) |
| `{` `}` | `LBrace`, `RBrace` | Struct literals (immutable) |
| `\|` | `Pipe` | Set literal delimiter |
| `@[` | `AtBracket` | @array literal prefix (mutable) |
| `@{` | `AtBrace` | @struct literal prefix (mutable) |
| `@\|` | `AtPipe` | @set literal prefix (mutable) |
| `'` | `Quote` | Quote reader macro |
| `` ` `` | `Quasiquote` | Quasiquote reader macro |
| `,` | `Unquote` | Unquote reader macro (inside quasiquote) |
| `;` | `Splice` | Splice reader macro |
| `:` | `Colon` | Keyword prefix; also `:@name` for mutable type keywords |
| `@` | `At` | Mutable collection prefix (when not followed by `[`, `{`, or `\|`) |
| `#` | `Comment` | Line comment (now emitted as a token) |

## Keyword syntax

Keywords are prefixed with `:`. The lexer supports `:@name` syntax for mutable type keywords:
- `:set` — immutable set type keyword
- `:@set` — mutable set type keyword
- `:@array` — mutable @array type keyword
- `:@string` — mutable @string type keyword

The `@` in `:@name` is consumed by the lexer and prepended to the keyword name.

## Set literals

- `|...|` reads as `SyntaxKind::Set(Vec<Syntax>)` — immutable set literal
- `@|...|` reads as `SyntaxKind::SetMut(Vec<Syntax>)` — mutable set literal
- Inside a list `(...)`, `[...]`, `{...}`, or `@{...}`, a bare `|` starts a
  nested set literal (delegates to `read_set`), producing a `SyntaxKind::Set`
  node. `|` is purely a set delimiter in all contexts.

## Invariants

1. **Shebang lines are stripped.** `#!` at start of input is ignored.

2. **Empty input returns error.** Not `Ok(Nil)`. Check before parsing.

3. **`SourceLoc` is 1-indexed.** Line 1, column 1 is the first character.

4. **`SyntaxReader` checks for trailing tokens.** Use `check_exhausted()`
   to detect garbage after the expression.

5. **Qualified symbols are single tokens.** `module:name` is lexed as one
   token, not three. The Analyzer desugars qualified symbols to nested
   `get` calls during analysis.

6. **`|` is a delimiter for set literals.** `|1 2 3|` is lexed as `Pipe`, elements,
   `Pipe` (for immutable sets). `@|1 2 3|` is lexed as `AtPipe`, elements, `Pipe`
   (for mutable sets). Inside lists, `|` starts a nested set literal (delegates
   to `read_set`), producing a `SyntaxKind::Set` node. `|` is purely a set
   delimiter in all contexts. It cannot appear in symbol names.

7. **`:@name` keywords are valid.** The lexer recognizes `:@` as a keyword
   prefix variant. The `@` is consumed and prepended to the keyword name.

8. **The epoch declaration selects the lexicon.** `(elle/epoch N)` at the top
   of a source unit decides how that unit tokenizes, before any token is
   produced (`docs/impl/lexicon.md`). An epoch this compiler has no lexicon
   for is a read error, not a parse that guesses.

9. **Comments are tokens.** `#` line comments are emitted as `Token::Comment(String)`
   by the lexer. Both `SyntaxReader` and `Reader` skip comment tokens during
   parsing — they do not appear in the output tree. The formatter collects
   them separately via `lex_with_comments()` for comment preservation.
