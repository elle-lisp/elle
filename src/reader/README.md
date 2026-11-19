# Reader

<!-- audited: 2026-11-18 -->

The reader turns Elle source text into `Syntax` trees for the compiler, or into `Value` trees for an embedder.

[AGENTS.md](AGENTS.md) lists the entry points, and
[docs/impl/reader.md](../../docs/impl/reader.md) holds the design.

## Two output forms

A `Syntax` tree keeps a source span on every node, and the macro expander
reads it. Every `read_syntax*` entry point takes the `SyntaxArena` its nodes
are born in:

```rust
use elle::reader::read_syntax;

let syntax = read_syntax(arena, "(+ 1 2)", "example.lisp")?;
// syntax.span holds the line and column
// syntax.kind is SyntaxKind::List(...)
```

A `Value` tree interns its symbols as it reads. `read_str` builds it in a fresh
region on the heap the caller passes:

```rust
use elle::reader::read_str;

let value = read_str("(+ 1 2)", runtime.heap(), &mut symbols)?;
```

## Lexer

The lexer tokenizes under the `Lexicon` its epoch selects
([docs/impl/lexicon.md](../../docs/impl/lexicon.md)). The entry points prescan
the source for `(elle/epoch N)` and hand the lexer that epoch's rules. A lexer
built directly uses the current epoch's rules:

```rust
let mut lexer = Lexer::new("(+ 1 2)");
while let Some(tok) = lexer.next_token_with_loc()? {
    println!("{:?} at {}:{}", tok.token, tok.loc.line, tok.loc.col);
}
```

## Several forms

`read_syntax_all` reads every top-level form of a source:

```rust
let forms = read_syntax_all(arena, source, source_name)?;
for form in forms {
    // Process each form
}
```
