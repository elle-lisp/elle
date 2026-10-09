# Macro Expansion

<!-- audited: 2026-10-07 -->

The expander turns each macro call into the form its transformer returns, with hygienic scopes.

## How macros work

1. **Definition**: `(defmacro name (params) body)` registers a definition.
2. **First expansion**: the body is compiled once, as the closure
   `(fn (params) body)`, and cached on the definition.
3. **Every expansion**: the closure runs on the VM with the call's arguments,
   and its result converts back to `Syntax` and replaces the call.
4. **Hygiene**: identifiers the macro introduces do not capture identifiers at
   the call site.

## Key files

| File | Purpose |
|------|---------|
| [`mod.rs`](mod.rs) | `Expander` struct, context, entry point |
| [`macrodef.rs`](macrodef.rs) | `MacroDef`, its parameters, and its transformer cell |
| [`define.rs`](define.rs) | `defmacro` |
| [`macro_expand.rs`](macro_expand.rs) | VM-based macro expansion |
| [`quasiquote.rs`](quasiquote.rs) | Quasiquote-to-code conversion |
| [`introspection.rs`](introspection.rs) | `macro?`, `expand-macro` |

## See also

- [AGENTS.md](AGENTS.md) — hygiene, the transformer cache, and the invariants
- [docs/macros.md](../../../docs/macros.md) — macros as a user writes them, and the prelude's macros
- [`src/syntax/`](../) — syntax tree types
- [`src/hir/`](../../hir/) — consumes expanded syntax
