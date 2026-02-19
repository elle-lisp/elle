# syntax

Syntax tree types and macro expansion. Bridge between parsing and analysis.

## Responsibility

- Define `Syntax` and `SyntaxKind` types
- Track source spans
- Support hygienic macros via scope sets
- Expand macros before analysis

Does NOT:
- Parse source (that's `reader`)
- Resolve bindings (that's `hir`)
- Generate code (that's `lir`, `compiler`)

## Interface

| Type | Purpose |
|------|---------|
| `Syntax` | Tree node with kind, span, scopes |
| `SyntaxKind` | Node variants (Int, Symbol, List, Quote, etc.) |
| `Span` | Source range with line/col |
| `ScopeId` | Unique scope identifier for hygiene |
| `Expander` | Macro expansion engine |
| `MacroDef` | Macro definition |
| `resolve_qualified_symbol()` | Resolve `module:name` to flat primitive name |
| `eval_quasiquote_to_syntax()` | Evaluate quasiquote template to Syntax tree |

## Data flow

```
Syntax (from reader)
    │
    ▼
Expander
    ├─► check for macro calls
    ├─► substitute parameters
    ├─► add expansion scope
    ├─► handle macro? (check registry, return #t/#f literal)
    ├─► handle expand-macro (expand quoted form, wrap in quote)
    ├─► resolve module:name to flat primitives
    └─► recurse on result
    │
    ▼
Syntax (expanded)
    │
    ▼
Analyzer (hir)
```

## Dependents

- `pipeline.rs` - calls `Expander::expand()`
- `hir/analyze.rs` - consumes expanded Syntax

## Invariants

1. **Scopes are additive.** `add_scope()` never removes. Two identifiers
   match only if their scope sets are compatible.

2. **Quote forms are not expanded.** `'x` remains `Quote(Symbol("x"))`.
   The analyzer handles quote specially.

3. **Quasiquote/unquote must be expanded.** If analysis sees raw
   `Quasiquote`, `Unquote`, or `UnquoteSplicing`, expansion failed.

4. **Macro arity is checked.** Wrong argument count → error, not silent
   misbehavior.

5. **macro? and expand-macro are compile-time.** Both are handled by the
   Expander during expansion, not as runtime primitives. `macro?` checks
   the macro registry and returns a literal `#t` or `#f`. `expand-macro`
   expands a quoted form and wraps the result in quote.

6. **Quasiquote templates produce Syntax trees.** `eval_quasiquote_to_syntax`
   evaluates quasiquote templates directly to Syntax, not to `(list ...)`
   runtime calls. This ensures macro-generated code has proper spans.

7. **Module-qualified names are resolved at expansion time.** `module:name`
   is recognized by the lexer as a single token, then resolved by the
   Expander to a flat primitive name (e.g., `string:upcase` → `string-upcase`).

## Hygiene

Each macro expansion creates a fresh `ScopeId`. Identifiers introduced by
the macro carry this scope. Identifiers from the call site don't. This
prevents accidental capture:

```lisp
(defmacro swap (a b)
  `(let ((tmp ,a)) (set! ,a ,b) (set! ,b tmp)))

(let ((tmp 10) (x 1) (y 2))
  (swap x y)
  tmp)  ; Still 10, not affected by macro's tmp
```

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | 454 | `Syntax`, `SyntaxKind`, `ScopeId` |
| `span.rs` | ~50 | `Span` type |
| `expand/mod.rs` | ~280 | `Expander` struct, context, entry point |
| `expand/macro_expand.rs` | ~250 | Macro expansion logic |
| `expand/quasiquote.rs` | ~200 | Quasiquote/unquote evaluation |
| `expand/threading.rs` | ~150 | Threading macros (`->`, `->>`) |
| `expand/introspection.rs` | ~100 | `macro?`, `expand-macro` |
| `expand/qualified.rs` | ~100 | Module-qualified name resolution |
| `expand/tests.rs` | ~537 | Expansion tests |
| `convert.rs` | ~100 | `Syntax` ↔ `Value` conversion |
| `display.rs` | ~100 | Pretty printing |
