# Syntax

<!-- audited: 2026-10-07 -->

The syntax tree the reader builds, the expander rewrites, and the analyzer consumes.

A `Syntax` node keeps what a runtime `Value` does not: a source span on every
node, and a scope set on every identifier for macro hygiene. A node is `Copy`
region data, so every constructor names the `SyntaxArena` it allocates in.

| Aspect | Syntax | Value |
|--------|--------|-------|
| Purpose | Compilation | Runtime |
| Symbols | Names, interned at analysis | Interned `SymbolId` |
| Locations | A `Span` on every node | None |

## Macro expansion

The `Expander` rewrites macro calls until no macro heads a form. It is built
over the heap of the VM its transformers run on. A definition comes from a
`defmacro` form, or from `MacroDef::new`:

```rust
let mut expander = Expander::on_vm(&mut vm);
let params = MacroParams::fixed(vec!["test".to_string()]).with_rest(Some("body".to_string()));
expander.define_macro(MacroDef::new("my-when", params, template));
let expanded = expander.expand(syntax, &mut symbols, &mut vm)?;
```

## See also

- [AGENTS.md](AGENTS.md) — the types, the arenas and the invariants
- [expand/AGENTS.md](expand/AGENTS.md) — macro expansion and hygiene
- [docs/impl/syntax.md](../../docs/impl/syntax.md) — where a node lives, and how spans are packed
- [docs/macros.md](../../docs/macros.md) — macros as a user writes them
