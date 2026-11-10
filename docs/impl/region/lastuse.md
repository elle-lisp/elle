# Last use

<!-- audited: 2026-11-09 -->

Where the solver puts the last read of each value, and why it solves the chains between bindings from below.

A region is released after the last node that reads its value, and
[anchors.md](anchors.md) says where each binder pins that release.
`compute_last_use` ([lastuse.rs](../../../src/hir/liveness/lastuse.rs)) finds
that node for every node that makes a value, in two steps: a walk, then a solve
over the bindings.

## The walk

The walk gives each node the node that consumes its value. A call, a binder, a
`return` and every other consumer takes its operands, so an operand's last use
is its consumer. A node that hands its value on unchanged, such as an `if` arm,
the last form of a `begin` or the body of a `let`, takes its parent's consumer.
A binder consumes its init, so the walk gives an init the binder node. A read
inside a loop of a binding bound outside that loop takes the loop node
([anchors.md](anchors.md)).

## Binding chains

A bound value is read through its name, so the binder node is not where the
value dies. The value dies after the last read of its binding. Each read has a
last use of its own, and a read can be the init of another binder: a closure
that captures the binding, or a `let` that names it again. So the last use of
one init depends on the last use of another, and the dependencies form chains.

`compute_last_use` solves the chains to their least fixpoint. Each init starts
at its floor, which is the init itself, or for a `def` the point the walk gave
the `def` ([anchors.md](anchors.md) says why). The init then grows to the
latest last use of any read of its binding, and the solver repeats until
nothing grows. Every step takes a maximum, so the order in which the solver
visits the bindings does not change the answer.

The binder node is an upper bound, never a floor. No step makes a last use
smaller, so a solve that starts an init at its binder keeps it there. A binding
captured by a closure that a `let` or a `letrec` binds would then live until
that form ends, whatever the closure does. At the top level the form is the
file, and the value would live until the program ends.

The chain through a closure is pinned by `liveness::tests::lastuse`.
