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

## Data flow

```
Syntax (from reader)
    │
    ▼
Expander
    ├─► check for macro calls
    ├─► substitute parameters
    ├─► add expansion scope
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
| `mod.rs` | 450 | `Syntax`, `SyntaxKind`, `ScopeId`, tests |
| `span.rs` | ~50 | `Span` type |
| `expand.rs` | ~300 | `Expander`, `MacroDef` |
| `convert.rs` | ~100 | `Syntax` ↔ `Value` conversion |
| `display.rs` | ~100 | Pretty printing |
