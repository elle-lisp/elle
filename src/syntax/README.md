# Syntax

The `Syntax` type represents parsed Elle code before analysis. Unlike `Value`
(the runtime representation), `Syntax` preserves source locations and supports
hygienic macro expansion.

## Syntax vs Value

| Aspect | Syntax | Value |
|--------|--------|-------|
| Purpose | Compilation | Runtime |
| Symbols | Strings | Interned SymbolId |
| Locations | Preserved | Lost |
| Macros | Expandable | Already expanded |

## Structure

```rust
pub struct Syntax {
    pub kind: SyntaxKind,
    pub span: Span,
    pub scopes: Vec<ScopeId>,  // For hygiene
}

pub enum SyntaxKind {
    Nil, Bool(bool), Int(i64), Float(f64),
    Symbol(String), Keyword(String), String(String),
    List(Vec<Syntax>), Array(Vec<Syntax>),
    Quote(Box<Syntax>), Quasiquote(Box<Syntax>),
    Unquote(Box<Syntax>), UnquoteSplicing(Box<Syntax>),
}
```

## Macro Expansion

The `Expander` transforms macro calls into their expanded forms:

```rust
let mut expander = Expander::new();

// Define a macro
expander.define_macro(MacroDef {
    name: "when".to_string(),
    params: vec!["cond".to_string(), "body".to_string()],
    rest_param: None,
    template: /* `(if ,cond ,body nil) */,
    definition_scope: ScopeId(0),
});

// Expand code
let expanded = expander.expand(syntax)?;
```

## Hygiene

Elle macros are hygienic - identifiers introduced by the macro won't
accidentally capture identifiers from the call site.

Each expansion adds a fresh `ScopeId` to introduced identifiers. Two
identifiers only match if their scope sets are compatible (one is a
subset of the other).

Example:
```lisp
(defmacro inc (x) `(+ ,x 1))
(let ((+ -))  ; Shadow + with -
  (inc 5))    ; Still uses +, not -, because macro's + has different scope
```

## Spans

Every `Syntax` node carries a `Span` indicating its source location:

```rust
pub struct Span {
    pub start: usize,   // Byte offset
    pub end: usize,     // Byte offset
    pub line: usize,    // 1-indexed
    pub col: usize,     // 1-indexed
    pub file: Option<String>,
}
```

Use `span.merge(&other)` to combine spans (e.g., for a list spanning
multiple lines).

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- `src/reader/` - produces Syntax trees
- `src/hir/` - consumes expanded Syntax
