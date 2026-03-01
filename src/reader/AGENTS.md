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
| `Token` | Token variants (LParen, Int, Symbol, etc.) |
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

## Invariants

1. **Shebang lines are stripped.** `#!` at start of input is ignored.

2. **Empty input returns error.** Not `Ok(Nil)`. Check before parsing.

3. **`SourceLoc` is 1-indexed.** Line 1, column 1 is the first character.

4. **`SyntaxReader` checks for trailing tokens.** Use `check_exhausted()`
   to detect garbage after the expression.

5. **Qualified symbols are single tokens.** `module:name` is lexed as one
   token, not three. The Expander resolves qualified symbols to flat
   primitive names at expansion time.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | 130 | Entry points: `read_str`, `read_syntax`, `read_syntax_all` |
| `lexer.rs` | ~300 | Tokenization |
| `token.rs` | ~100 | Token types, SourceLoc |
| `parser.rs` | ~200 | Token → Value parsing |
| `syntax.rs` | ~200 | Token → Syntax parsing |
