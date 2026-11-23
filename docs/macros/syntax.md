# Macros: syntax as data

<!-- audited: 2026-11-23 -->

Every macro argument is a syntax object, and a transformer reads a node's kind, children, span and binding.

[overview.md](overview.md) shows what a transformer cannot read today. The
operation names below are working names ([plan.md](plan.md)).

## Every argument is a syntax object

A transformer receives each argument as a syntax object, an atom included. So
every argument carries a span and a scope set, a compile error can point at any
of them, and `datum->syntax` takes its context from any of them.

A syntax object is truthy. A body that tests a literal argument unwraps it
first with `syntax-e`, because the literal `false` arrives as an object. An
`&opt` parameter that the call leaves out is plain `nil`, so a body still
tests for a missing argument directly.

## Reading a node

| Operation | Answers |
|-----------|---------|
| `(meta/syntax-kind stx)` | A keyword for the node's kind: `:symbol`, `:list`, `:array`, `:struct`, `:quote` and the rest of the kinds the reader builds |
| `(meta/syntax-children stx)` | An array of the node's children, each a syntax object; empty for an atom |
| `(meta/syntax-rebuild stx children)` | A node of the same kind, span and scopes over new children |
| `(meta/syntax-span stx)` | `{:file :line :col}`, as `meta/location` answers for a form |
| `(meta/syntax-error stx message)` | No value: the compile fails with the message, at the node's span |

The kind and the children are enough to walk any tree without a case for each
kind, and `rebuild` puts a walked tree back. `match` takes a syntax object and
matches a pattern against the node's kind and children, so a transformer
destructures an argument as it destructures data.

## A datum keeps its kind

`syntax->datum` strips the scopes and keeps the shape. A struct literal comes
back as a struct, a set literal as a set, and `datum->syntax` over the result
builds the node it started from. A quoted list that happens to start with
`struct` therefore stays a list.

## Identifiers compare by binding

`(meta/same-binding? a b)` answers whether two identifiers resolve to one
binding where the expansion stands. Two identifiers that resolve to no binding
are the same when their spellings match, which is how a macro recognizes a
marker word such as `in`.

A macro that treats one name specially asks this question. An `assert` that
reports both sides of `=` recognizes the `=` of the standard library, and no
longer a local function of that spelling. The `(literal sym)` pattern of
`syntax-case` asks it too.

`(meta/static-value id)` answers the value of the binding an identifier
resolves to, when that binding is static ([static.md](static.md)). A second
argument is what it answers otherwise, and without one it fails the compile at
the identifier.

## Rejected alternatives

**Atoms stay plain values.** No macro body would change. An atom would carry
no span and no scopes, and every test of a kind would need a branch for a value
and a branch for an object.

**A predicate for each kind.** Five exist today. A walk over them needs an arm
for each, and a kind added later reaches no walker.
