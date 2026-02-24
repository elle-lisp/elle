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
| `expand()` | Entry point: takes `&mut SymbolTable` and `&mut VM` |

## Data flow

```
Syntax (from reader)
    │
    ▼
Expander (with &mut SymbolTable, &mut VM)
    ├─► load prelude macros (when, unless, try, protect, defer, with)
    ├─► desugar defn to (def name (fn params body...))
    ├─► desugar let* to nested let (one binding at a time)
    ├─► check for macro calls
    ├─► compile & eval macro body in VM via pipeline::eval_syntax()
    ├─► convert result Value back to Syntax via from_value()
    ├─► add expansion scope
    ├─► handle macro? (check registry, return #t/#f literal)
    ├─► handle expand-macro (expand quoted form, wrap in quote)
    ├─► resolve module:name to flat primitives
    └─► recurse on result (with depth limit of 200)
    │
    ▼
Syntax (expanded)
    │
    ▼
Analyzer (hir)
```

## Dependents

- `pipeline.rs` - calls `Expander::expand()`, provides `eval_syntax()` for macro bodies
- `hir/analyze.rs` - consumes expanded Syntax

## Invariants

1. **Scopes are additive, with one exception.** `add_scope()` never
   removes. `add_scope_recursive()` skips nodes with `scope_exempt: true`
   (set by `datum->syntax` to prevent intro scope stamping on nodes that
   should resolve at the call site). `scope_exempt` only affects
   `add_scope_recursive`, not `add_scope`. Two identifiers match only if
   their scope sets are compatible.

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

6. **Macro bodies are VM-evaluated.** Macro arguments are quoted and passed
   to the macro body, which is compiled and executed in the real VM via
   `pipeline::eval_syntax()`. The result Value is converted back to Syntax
   via `from_value()`. Macros must use quasiquote to return code templates.

7. **Module-qualified names are resolved at expansion time.** `module:name`
   is recognized by the lexer as a single token, then resolved by the
   Expander to a flat primitive name (e.g., `string:upcase` → `string-upcase`).

## Hygiene

Each macro expansion creates a fresh `ScopeId`. Identifiers introduced by
the macro carry this scope. Identifiers from the call site don't. The
Analyzer uses scope-set subset matching to prevent accidental capture:

```lisp
(defmacro swap (a b)
  `(let ((tmp ,a)) (set! ,a ,b) (set! ,b tmp)))

(let ((tmp 10) (x 1) (y 2))
  (swap x y)
  tmp)  ; Still 10, not affected by macro's tmp
```

### Syntax objects in the Value system

`SyntaxKind::SyntaxLiteral(Value)` is an internal-only variant used by
`expand_macro_call_inner` to inject `Value::syntax(arg)` into the
compilation pipeline. This preserves scope sets through the Value
round-trip during macro expansion. The Analyzer handles it by producing
`HirKind::Quote(value)`.

**Hybrid argument wrapping:** Atoms (nil, bool, int, float, string,
keyword) are wrapped via `Quote` to preserve runtime semantics (e.g.,
`#f` stays falsy). Symbols and compound forms are wrapped via
`SyntaxLiteral(Value::syntax(arg))` to preserve scope sets.

### Hygiene escape hatch: `datum->syntax`

`(datum->syntax context datum)` creates a syntax object with the
context's scope set and `scope_exempt: true`. This prevents
`add_scope_recursive` from adding the intro scope, so the datum
resolves at the call site. Used for anaphoric macros:

```lisp
(defmacro aif (test then else)
  `(let ((,(datum->syntax test 'it) ,test))
     (if ,(datum->syntax test 'it) ,then ,else)))
```

`(syntax->datum stx)` strips scope information, returning the plain value.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~520 | `Syntax`, `SyntaxKind`, `ScopeId`, `set_scopes_recursive` |
| `span.rs` | ~50 | `Span` type |
| `expand/mod.rs` | ~280 | `Expander` struct, context, entry point |
| `expand/macro_expand.rs` | ~80 | VM-based macro expansion via `eval_syntax` |
| `expand/quasiquote.rs` | ~200 | Quasiquote-to-code conversion |
| `expand/threading.rs` | ~150 | Threading macros (`->`, `->>`) |
| `expand/introspection.rs` | ~100 | `macro?`, `expand-macro` |
| `expand/qualified.rs` | ~100 | Module-qualified name resolution |
| `expand/tests.rs` | ~537 | Expansion tests |
| `convert.rs` | ~100 | `Syntax` ↔ `Value` conversion |
| `display.rs` | ~100 | Pretty printing |
