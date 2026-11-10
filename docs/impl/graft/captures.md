# Grafting a call: the capture a graft adds

<!-- audited: 2026-11-10 -->

A body grafted inside a lambda reaches the module bindings it names through captures the graft appends to that lambda.

[Grafting a call](../graft.md) owns the graft itself. This document owns what
happens when a grafted body of this unit names a binding the lambda around the
call does not hold.

```lisp
(defn fhir [src]
  "The functionalized HIR of src, as text."
  (get (compile/dumps src "<doc>") :fhir))

(defn occurrences [text part]
  "How many times part occurs in text."
  (- (length (string/split text part)) 1))
```

## The names a body reaches

A lambda reaches a module binding through its captures, and the analyzer fixes
those before the graft runs. A body of this unit names only module bindings
and primitives, because a body that names an enclosing runtime local does not
close ([the HIR design](../hir.md)). A primitive is a constant anywhere. A
module binding the lambda neither captures nor binds would read a slot the
lowerer never allocates, so without a capture the call has to stay a call.

The graft adds the capture. For each such binding it appends a `Local` capture
to the lambda around the call, and to every enclosing lambda that neither
captures nor binds the binding, down to the file letrec. The file letrec holds
every module binding's cell before any initializer runs, so a lambda built
early captures a cell a later initializer fills, exactly as a lambda that
names the binding itself does. The graft marks the binding captured, which is
what tells the letrec to mint that cell.

Below, `h` names `k`, and the lambda inside `f` names neither. The call to `h`
grafts, the graft of `k` inside it grafts too, and the lambda's body is `y`:

```lisp
(def inner (fhir "(defn k [x] x)\n(defn h [x] (k x))\n(defn f [] (fn [y] (h y)))\n(f)"))
(assert (< 0 (occurrences inner "(return y#")) "the lambda's body is k's body over y")
```

A lambda that holds the binding as a self-reference, because it is the
binding's own initializer and calls itself, holds the executing closure and
not the cell. Such a capture becomes a plain one when a lambda inside it needs
the cell, as the analyzer makes it when a nested lambda names the enclosing
binding ([self-reference](../selfrec.md)).

Outside every lambda the rule is the analyzer's: a body may name a module
binding only after its initializer ran, because the graft would read the cell
at once. A call there whose body names a later binding stays a call.

## What it costs

A capture the fold leaves unread stays on the lambda, so a closure it builds
carries one environment slot more per such binding. A grafted wrapper over
proven operands reads none of the bindings its guards named, so most of the
captures a graft adds are of this kind.

The reason to pay it is the standard library's own bodies. The stdlib is one
unit, and every wrapper in it names a sibling: `+` names `number?`, and `>=`
names `check-comparable`. Without the capture no call to a wrapper inside a
stdlib function grafts, and a loop a stdlib function runs pays a wrapper call
per operator per element where the same loop in a program pays one op.

## Pinning tests

- `hir::typeinfer::tests::capture::*` — a same-unit body grafts inside a
  lambda and the lambda captures what the body names; every lambda between the
  graft and the file captures it; a body naming a sibling defined later grafts
  and the program runs; a self-reference the graft reaches through becomes a
  plain capture; a mutable module binding is read live; the stdlib's search
  loop reaches its ops with no wrapper call.
- [the stdlib cost test](../../../tests/impl/stdlib-hof-cost.lisp) — a stdlib
  search and a stdlib map each cost about what the same loop written in the
  program costs.
