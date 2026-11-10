# Last use

<!-- audited: 2026-11-09 -->

Where the solver puts the last read of each value, and why a `let` or `letrec` init starts its solve at the init itself.

A region is released after the last node that reads its value, and
[anchors.md](anchors.md) says where each binder pins that release.
`compute_last_use` ([lastuse.rs](../../../src/hir/liveness/lastuse.rs)) finds
that node for every node that makes a value, in two steps: a walk, then a solve
over the bindings.

## The walk

The walk gives each node the node that consumes its value. A call, a `def`, a
`return` and every other consumer takes its operands, so an operand's last use
is its consumer. A node that hands its value on unchanged, such as an `if` arm,
the last form of a `begin` or the body of a `let`, takes its parent's consumer.
A read inside a loop of a binding bound outside that loop takes the loop node
([anchors.md](anchors.md)).

A `let` or a `letrec` stores its init in the binding and reads it no more. So
the walk gives the init the init itself, and the forms inside the init that
hand its value on inherit that: the arms of an `if`, the body of a `let`. A
`loop` rebinds its parameters on every pass, and a release inside the body
would run on each of them. So the walk gives a `loop` init the loop node.

## Binding chains

A bound value is read through its name, so the value dies after the last read
of its binding. Each read has a last use of its own, and a read can be the init
of another binder: a closure that captures the binding, or a `let` that names
it again. So the last use of one init depends on the last use of another, and
the dependencies form chains.

`compute_last_use` solves the chains to a fixpoint. It starts each init where
the walk put it, raises the init to the latest last use of any read of its
binding, and repeats until nothing grows. Every step takes a maximum, so the
order in which the solver visits the bindings does not change the answer. An
init no name reads takes its floor: the init itself, or for a `def` the point
the walk gave the `def` ([anchors.md](anchors.md) says why).

No step makes a last use smaller, so the walk's answer for an init stays a
lower bound on every read that depends on it. For a `let` or a `letrec` that
answer is the init itself, the least it can be. The binder node would be too
much: a binding captured by a closure that the `let` or `letrec` binds would
then live until that form ends, whatever the closure does. At the top level
the form is the file, and the value would live until the program ends.

The chain through a closure is pinned by `liveness::tests::chains`, and the
release it moves by
[region-toplevel-capture-release.lisp](../../../tests/impl/region-toplevel-capture-release.lisp).
