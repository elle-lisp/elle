# Reader

The reader transforms source text into syntax trees. It supports three
input formats: s-expressions (`.lisp`), Lua (`.lua`), and literate
markdown (`.md`).

## Pipeline

```text
Source text → [shebang strip] → [epoch prescan] → Lexer(Lexicon) → Tokens
            → Parser → Vec<Syntax>
```

The entry points in `src/reader/mod.rs` prescan the source for its
`(elle/epoch N)` declaration and lex under the lexicon that epoch selects
(see [impl/lexicon.md](lexicon.md)). A declaration naming an epoch this
compiler does not know is a read error: no lexicon exists to tokenize it.
`read_syntax_all_current` skips the prescan and lexes under the current
epoch; the REPL is its one caller.

## Lexer

`Lexer` (`src/reader/lexer.rs`) produces `TokenWithLoc` values, each
bundling a `Token`, `SourceLoc`, and byte length.

### Token types

```text
Symbol         variable and function names
Keyword        :foo — self-evaluating
Integer        42, 0xFF, 0b1010
Float          3.14
String         "hello"
LParen/RParen  ( )
LBracket/RBracket  [ ]
LBrace/RBrace  { }
Pipe           | (set delimiters)
At             @ (mutability prefix)
Quote          '
Quasiquote     `
Unquote        ,
UnquoteSplice  ,;
Splice         ;
Hash           # (comment — skipped by lexer)
Ampersand      & (rest/keys marker)
Underscore     _ (wildcard)
```

## Source locations

`SourceLoc` tracks `file`, `line`, and `col` for every token. Error
messages reference these positions back to the original source — even
for `.md` files, where blank-line padding preserves line numbers.

## Parser

`SyntaxReader` (`src/reader/syntax.rs`) builds `Syntax` trees from
tokens. A `Syntax` node carries a `SyntaxKind` (symbol, keyword,
integer, list, array, struct, set, etc.) plus a `Span` for error
reporting.

## Dispatch

`read_syntax_all_for` dispatches on file extension:
- `.lua` → `lua_parser::parse_lua_file`
- `.md` → `strip_markdown` then standard reader
- everything else → standard s-expression reader

---

## See also

- [impl/hir.md](hir.md) — analysis phase after reading
- [impl/lexicon.md](lexicon.md) — epoch-aware lexing: the prescan and the `Lexicon`
- [syntax.md](../syntax.md) — user-facing syntax reference
