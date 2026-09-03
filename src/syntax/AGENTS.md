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
| `Syntax` | Tree node with kind, span, scopes — `Copy` POD, region-resident |
| `SyntaxKind` | Node variants (Int, Symbol, List, Quote, Set, SetMut, etc.) |
| `SynRef` | A pointer to one node: the payload of the single-child kinds |
| `Span` | Source range with line/col, over an interned `FileId` |
| `ScopeId` | Unique scope identifier for hygiene |
| `SyntaxArena` | Where nodes are born: a heap and a region on it |
| `SyntaxHeap` | An arena plus the heap under it, for callers with no runtime |
| `Expander` | Macro expansion engine |
| `MacroDef` | Macro definition |
| `expand()` | Entry point: takes `&mut SymbolTable` and `&mut VM` |

## The tree is region data

Nodes, child slices, and string payloads live in region pages, and a node is
`Copy` POD with no `Drop`. Every constructor therefore names a `SyntaxArena`:
`Syntax::list(arena, &items, span)`, `Syntax::symbol(arena, name, span)`.
`Syntax::new(kind, span)` still takes no arena, because the payload inside
`kind` was already built through one.

Payloads dereference to what the owned representation held — `&str` for a
name, `&[Syntax]` for children, `&Syntax` for a wrapped form — so a reader of
the tree matches and walks it unchanged.

`SyntaxKind::children` and `SyntaxKind::rebuild` are the pair every recursive
pass goes through: they take a compound apart and put an equivalent one back
together, so a walk states its own logic and not a fifteen-arm match.

[docs/impl/syntax.md](../../docs/impl/syntax.md) owns the model — which arena
a node belongs to, why a scope walk copies, and when in-place mutation is
legal.

## SyntaxKind variants for sets

| Variant | Purpose |
|---------|---------|
| `Set(RegionSlice<Syntax>)` | Immutable set literal `\|...\|` |
| `SetMut(RegionSlice<Syntax>)` | Mutable set literal `@\|...\|` |

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
     ├─► handle macro? (check registry, return true/false literal)
    ├─► handle expand-macro (expand quoted form, wrap in quote)
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
    the macro registry and returns a literal `true` or `false`. `expand-macro`
   expands a quoted form and wraps the result in quote.

6. **Macro bodies are VM-evaluated.** Macro arguments are quoted and passed
   to the macro body, which is compiled and executed in the real VM via
   `pipeline::eval_syntax()`. The result Value is converted back to Syntax
   via `from_value()`. Macros must use quasiquote to return code templates.

7. **Qualified symbols pass through expansion unchanged.** `module:name`
   is recognized by the lexer as a single token. The Expander does not
   transform it. The Analyzer desugars it to nested `get` calls.

8. **Or-patterns use `(or pat1 pat2 ...)` syntax.** The `or` symbol in
   pattern position is recognized by the match analyzer in `special.rs`.
   `|` inside lists always starts a set literal (no special marker node).

9. **Set literals are desugared during analysis.** `SyntaxKind::Set` and
   `SyntaxKind::SetMut` pass through expansion unchanged. The Analyzer
   desugars them to `(set ;elems)` and `(mutable-set ;elems)` calls,
   respectively. This keeps the Expander simple and defers collection
   construction to the analysis phase.

10. **A subtree is shared, so a scope walk copies.** `Syntax` is `Copy` and
    a child slice is a region handle, so handing a subtree to two holders
    costs nothing — and writing a scope through one of them would be visible
    to the other. `stamp_scope` and `flip_scope_recursive` therefore build a
    new tree in the working arena. In-place mutation is legal only on a
    uniquely owned tree, through `Syntax::children_mut`.

## Hygiene

Each macro expansion creates a fresh `ScopeId`. Identifiers introduced by
the macro carry this scope. Identifiers from the call site don't. The
Analyzer uses scope-set subset matching to prevent accidental capture:

```lisp
(defmacro swap (a b)
  `(let [tmp ,a] (set ,a ,b) (set ,b tmp)))

(let [tmp 10 x 1 y 2]
  (swap x y)
  tmp)  ; Still 10, not affected by macro's tmp
```

### Syntax objects in the Value system

`SyntaxKind::SyntaxLiteral(SynRef)` is an internal-only variant that carries a
hygiene-bearing template symbol as plain compile-time data. Quasiquote creates
it so a template symbol's scope set survives the Value round-trip during macro
expansion; the Analyzer materializes it as an ordinary allocation per
execution via `ConstTemplate::SyntaxSymbol`.

**Hybrid argument wrapping:** Atoms (nil, bool, int, float, string,
keyword) are wrapped via `Quote` to preserve runtime semantics (e.g.,
`false` stays falsy). Symbols and compound forms are wrapped via
`SyntaxLiteral(Value::syntax(arg))` to preserve scope sets.

### Hygiene escape hatch: `datum->syntax`

`(datum->syntax context datum)` creates a syntax object with the
context's scope set and `scope_exempt: true`. This prevents
`add_scope_recursive` from adding the intro scope, so the datum
resolves at the call site. Used for anaphoric macros:

```lisp
(defmacro aif (test then else)
  `(let [,(datum->syntax test 'it) ,test]
     (if ,(datum->syntax test 'it) ,then ,else)))
```

`(syntax->datum stx)` strips scope information, returning the plain value.
