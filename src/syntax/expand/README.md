# Macro Expansion

The macro expansion module transforms macro calls into their expanded forms using hygienic macro semantics.

## How Macros Work

1. **Definition**: `(defmacro name (params) body)` defines a macro
2. **Expansion**: When a macro is called, its body is compiled and executed in the VM
3. **Result**: The result is converted back to `Syntax` and substituted for the call
4. **Hygiene**: Identifiers introduced by the macro don't capture call-site identifiers

## Prelude Macros

The expander loads prelude macros before user code:

- `defn` — Define a function (desugars to `def` + `fn`)
- `let*` — Sequential let bindings (desugars to nested `let`)
- `when` — Conditional without else (desugars to `if`)
- `unless` — Conditional with inverted test (desugars to `if`)
- `try`/`catch` — Exception handling
- `protect` — Cleanup on exit
- `defer` — Deferred execution
- `with` — Resource management
- `->` — Thread-first macro
- `->>` — Thread-last macro

## Hygiene

Elle macros are hygienic — identifiers introduced by the macro won't accidentally capture identifiers from the call site.

Each expansion adds a fresh `ScopeId` to introduced identifiers. Two identifiers only match if their scope sets are compatible (one is a subset of the other).

## Example

```janet
(defmacro inc (x) `(+ ,x 1))
(let [+ -]  ; Shadow + with -
  (inc 5))    ; Still uses +, not -, because macro's + has different scope
```

## Key Files

| File | Purpose |
|--------|---------|
| [`mod.rs`](mod.rs) | `Expander` struct, context, entry point |
| [`macro_expand.rs`](macro_expand.rs) | VM-based macro expansion |
| [`quasiquote.rs`](quasiquote.rs) | Quasiquote-to-code conversion |
| [`introspection.rs`](introspection.rs) | `macro?`, `expand-macro` |

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`src/syntax/`](../) - syntax tree types
- [`src/hir/`](../../hir/) - consumes expanded syntax
