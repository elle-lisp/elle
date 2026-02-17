# Reader

The reader transforms Elle source text into tree structures. It's the first
stage of the compilation pipeline.

## Two Output Formats

**Syntax trees** (preferred): Used by the new compilation pipeline. Preserves
source locations and supports hygienic macro expansion.

```rust
use elle::reader::read_syntax;

let syntax = read_syntax("(+ 1 2)")?;
// syntax.span contains line/column info
// syntax.kind is SyntaxKind::List(...)
```

**Value trees** (legacy): Used by the older pipeline. Symbols are interned
immediately.

```rust
use elle::reader::read_str;

let value = read_str("(+ 1 2)", &mut symbols)?;
// value is a cons cell created via Value::cons(...)
```

## Lexer

The lexer tokenizes input character by character:

```rust
let mut lexer = Lexer::new("(+ 1 2)");
while let Some(tok) = lexer.next_token_with_loc()? {
    println!("{:?} at {}:{}", tok.token, tok.loc.line, tok.loc.col);
}
```

Tokens include: `LParen`, `RParen`, `LBracket`, `RBracket`, `Quote`,
`Quasiquote`, `Unquote`, `UnquoteSplicing`, `Int`, `Float`, `String`,
`Symbol`, `Keyword`, `True`, `False`, `Nil`.

## Source Locations

Every token and syntax node carries source location information:

```rust
pub struct SourceLoc {
    pub line: usize,    // 1-indexed
    pub col: usize,     // 1-indexed
}
```

This enables precise error messages like:
```
Error at 5:12: undefined variable 'foo'
```

## Multiple Forms

To parse a file with multiple top-level expressions:

```rust
let forms = read_syntax_all(source)?;
for form in forms {
    // Process each form
}
```

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- `src/syntax/` - macro expansion, Syntax type
- `src/hir/` - next stage after reading
