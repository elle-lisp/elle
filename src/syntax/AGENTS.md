# syntax

<!-- audited: 2026-10-13 -->

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
| `ScopeId` | A hygiene scope: the tag of the unit that minted it, the intro bit, and a count within the unit |
| `UnitTag` | The tag of a unit of expansion: a hash of its source name and its forms as read ([unit.rs](unit.rs)) |
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

## Expansion

[expand/AGENTS.md](expand/AGENTS.md) owns the expander: its data flow, its
dispatch order, how a call hands its arguments to the transformer, quasiquote,
`datum->syntax`, and the expander's invariants.
[docs/macros.md](../../docs/macros.md) owns the hygiene model and its
examples, and [docs/impl/syntax.md](../../docs/impl/syntax.md) the layout of a
`ScopeId`.

## Dependents

- [pipeline](../pipeline/AGENTS.md) calls `Expander::expand()` and provides
  `eval_syntax()` for macro bodies
- [hir/analyze](../hir/analyze/AGENTS.md) consumes the expanded tree

## Invariants

1. **Only the hygiene flip removes a scope.** `add_scope()` never removes
   one. The flip of an expansion's intro scope removes it from the nodes that
   came from the arguments. Both tree walks skip a node with
   `scope_exempt: true`, which `datum->syntax` sets so the node resolves at
   the call site. Two identifiers match only if their scope sets are
   compatible.

2. **Set literals are desugared during analysis.** `SyntaxKind::Set` and
   `SyntaxKind::SetMut` pass through expansion unchanged. The analyzer
   desugars them to `(set ;elems)` and `(@set ;elems)` calls.

3. **`|` inside a list always starts a set literal.** An or-pattern is
   written `(or pat1 pat2 ...)`, which the match analyzer recognizes; the
   reader builds no marker node for it.

4. **A subtree is shared, so a scope walk copies.** `Syntax` is `Copy` and
   a child slice is a region handle, so handing a subtree to two holders
   costs nothing — and writing a scope through one of them would be visible
   to the other. `stamp_scope` and `flip_scope_recursive` therefore build a
   new tree in the working arena. In-place mutation is legal only on a
   uniquely owned tree, through `Syntax::children_mut`.
