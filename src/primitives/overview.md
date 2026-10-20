# Primitives

<!-- audited: 2026-10-20 -->

The built-in functions every instance registers at startup, each a Rust function behind a `PrimitiveDef`.

A primitive is a function a program calls by name, written in Rust because it
needs the runtime's internals or a resource outside the process. A special form
is no primitive: the analyzer recognizes it by name
([hir/analyze](../hir/analyze/AGENTS.md)). The arithmetic, the ordering
comparisons and the output verbs are stdlib closures over the `%`-intrinsics
([intrinsics](../../docs/intrinsics.md)).

[The cookbook](../../docs/cookbook/primitives.md) says how to add one, and
[region effects](../../docs/impl/region/effects.md) says which effect its body
declares. A fact about one primitive belongs in its docstring, where
`(doc name)` finds it.

## The rules a primitive keeps

- It checks every argument the arity check leaves to it, and answers
  `(SIG_ERROR, ctx.error(kind, msg))` for a bad one. It never panics.
- It reaches its VM, its instance's symbol table and its heap through `ctx`.
  What the dispatch loop owns, such as running a fiber, it asks for with a
  signal, and the loop does the work.
- It resolves a symbol's name through its own instance's table, so two
  instances on one thread never read each other's names
  ([symbol](../../docs/impl/symbol.md)).
