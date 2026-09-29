# Reader

<!-- audited: 2026-09-28 -->

The reader transforms source text into syntax trees, from s-expressions, Lua, JavaScript, Python or literate markdown.

## Pipeline

```text
Source text → [shebang strip] → [epoch prescan] → Lexer(Lexicon) → Tokens
            → Parser(SyntaxArena) → Vec<Syntax>
```

The parser takes the arena its nodes are born in: a `Syntax` is region data
(see [impl/syntax.md](syntax.md)), so every `read_syntax*` entry point names
the arena the tree lives in and the caller owns that region's lifetime.

The entry points in [mod.rs](../../src/reader/mod.rs) prescan the source for
its `(elle/epoch N)` declaration and lex under the lexicon that epoch selects
(see [impl/lexicon.md](lexicon.md)). A declaration naming an epoch this
compiler does not know is a read error: no lexicon exists to tokenize it.
`read_syntax_all_current` skips the prescan and lexes under the current
epoch; the REPL is its one caller.

## Lexer

`Lexer` ([lexer.rs](../../src/reader/lexer.rs)) produces `TokenWithLoc`
values, each bundling a `Token`, its `SourceLoc`, its byte length and its
byte offset.

### Token types

```text
Symbol             names, including & and _ and @name
Keyword            :foo — self-evaluating
Integer            42, 0xFF, 0b1010
Float              3.14
String             "hello"
Bool, Nil          true, false, nil
LeftParen/RightParen      ( )
LeftBracket/RightBracket  [ ]
LeftBrace/RightBrace      { }
Pipe               | (set delimiter)
AtPipe             @| (mutable set)
ListSugar          @ before [ { or " (the mutable form)
BytesBracket       b[
AtBytesBracket     @b[
Quote              '
Quasiquote         `
Unquote            ,
UnquoteSplicing    ,;
Splice             ;
Comment            # to the end of the line
```

The parser skips `Comment` tokens, and the formatter reads them.

## Source locations

`SourceLoc` tracks `file`, `line`, and `col` for every token. Error
messages reference these positions back to the original source — even
for `.md` files, where blank-line padding preserves line numbers.

## Parser

`SyntaxReader` ([syntax.rs](../../src/reader/syntax.rs)) builds `Syntax`
trees from tokens. A `Syntax` node carries a `SyntaxKind` (symbol, keyword,
integer, list, array, struct, set, etc.) plus a `Span` for error
reporting.

At end of input inside an unfinished collection, the error points to the
outermost unclosed collection, names its type, and counts every collection
still open:

```lisp
(defn read-error [text] (get (protect (read text)) 1))
(assert (= (read-error "(a (b (c")
           {:error :read-error
            :message "<read>:1:1: unterminated list (3 closing parens needed)"}))
(assert (= (read-error "[1 2")
           {:error :read-error
            :message "<read>:1:1: unterminated array (missing closing bracket)"}))
(assert (= (read-error "(a [b")
           {:error :read-error
            :message "<read>:1:1: unterminated list (2 closing delimiters needed)"}))
```

## Dispatch

`read_syntax_all_for` dispatches on file extension:
- `.lua` → `lua_parser::parse_lua_file`
- `.js` → `js_parser::parse_js_file`
- `.py` → `py_parser::parse_py_file`
- `.md` → `strip_markdown`, then the s-expression reader
- everything else → the s-expression reader

---

## See also

- [impl/hir.md](hir.md) — analysis phase after reading
- [impl/lexicon.md](lexicon.md) — epoch-aware lexing: the prescan and the `Lexicon`
- [syntax.md](../syntax.md) — user-facing syntax reference
