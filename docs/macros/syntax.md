# Macros: syntax as data

<!-- audited: 2026-11-24 -->

Every macro argument is a syntax object, and a transformer reads a node's kind, children, span and binding.

[overview.md](overview.md) shows what a transformer cannot read today, and
[plan.md](plan.md) the steps this part lands in.

## Every argument is a syntax object

A transformer receives each argument as a syntax object, an atom included. So
every argument carries a span and a scope set, a compile error can point at any
of them, and `syntax/<-datum` takes its context from any of them.

A syntax object is truthy. A body that tests a literal argument unwraps it
first with `syntax/e`, because the literal `false` arrives as an object. An
`&opt` parameter that the call leaves out is plain `nil`, so a body still
tests for a missing argument directly.

## Reading a node

| Operation | Answers |
|-----------|---------|
| `(syntax/kind stx)` | A keyword for the node's kind: `:symbol`, `:list`, `:array`, `:struct`, `:quote` and the rest of the kinds the reader builds |
| `(syntax/children stx)` | An array of the node's children, each a syntax object; empty for an atom |
| `(syntax/rebuild stx children)` | A node of the same kind, span and scopes over new children |
| `(syntax/span stx)` | `{:file :line :col}`, as `meta/location` answers for a form |
| `(syntax/error stx message)` | No value: the compile fails with the message, at the node's span |

The kind and the children are enough to walk any tree without a case for each
kind, and `rebuild` puts a walked tree back. `match` takes a syntax object and
matches a pattern against the node's kind and children, so a transformer
destructures an argument as it destructures data.

## A datum keeps its kind

`syntax/->datum` strips the scopes and keeps the shape. A struct literal comes
back as a struct, a set literal as a set, and `syntax/<-datum` over the result
builds the node it started from. A quoted list that happens to start with
`struct` therefore stays a list.

## The names

Every syntax operation is named under `syntax/`, as the string operations are
named under `string/`. A name keeps the stem Racket gives the operation, so
`syntax->datum` is `syntax/->datum` and `datum->syntax` is `syntax/<-datum`. No
operation has a second name.

## Rejected alternatives

**Atoms stay plain values.** No macro body would change. An atom would carry
no span and no scopes, and every test of a kind would need a branch for a value
and a branch for an object.

**A predicate for each kind.** Five exist today. A walk over them needs an arm
for each, and a kind added later reaches no walker.

**The `meta/` prefix.** It already holds `meta/location` and `meta/gensym`,
which read no syntax object.
