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
| `Lexer` | Tokenizes input string |
| `Token` | Token variants (LParen, Int, Symbol, Pipe, AtPipe, etc.) |
| `SourceLoc` | Line/column position |
| `Reader` | Parses tokens to `Value` |
| `SyntaxReader` | Parses tokens to `Syntax` |

## Entry points

```rust
// Parse to Value (legacy)
let value = read_str(source, &mut symbols)?;

// Parse to Syntax (preferred)
let syntax = read_syntax(source)?;

// Parse multiple forms
let forms = read_syntax_all(source)?;
```

## Data flow

```
Source string
    │
    ▼
Lexer::new(source)
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
| `#` | Comment | Line comment (not a token) |

## Keyword syntax

Keywords are prefixed with `:`. The lexer supports `:@name` syntax for mutable type keywords:
- `:set` — immutable set type keyword
- `:@set` — mutable set type keyword
- `:@array` — mutable array type keyword
- `:@string` — mutable string (buffer) type keyword

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

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | 130 | Entry points: `read_str`, `read_syntax`, `read_syntax_all` |
| `lexer.rs` | ~300 | Tokenization |
| `token.rs` | ~100 | Token types, SourceLoc |
| `parser.rs` | ~200 | Token → Value parsing |
| `syntax.rs` | ~425 | Token → Syntax parsing |
| `syntax_tests.rs` | ~484 | Tests for SyntaxReader |
