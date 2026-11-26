# Last use

<!-- audited: 2026-11-25 -->

Where the solver puts the last read of each value, and why a `let` init and a branch's test are each their own last use.

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

An init can itself be a read inside a loop of a binding bound outside that
loop, as `b` is in `(while c (let [s b] …))`. The loop rule outranks the init
rule there, and the read takes the loop node. The value of `b` is read again
on the next pass, so no read of `s` can end it sooner.

A binding that an `assign` writes is the exception. A name bound to a read of
it takes a counted reference of its own ([reads.md](reads.md)), on every pass.
So the init rule holds, and the release of `s` runs on every pass. With the
loop node as its last use, the release would run once past the loop, and every
pass but the last would strand its reference.

A branch reads its test to choose an arm, and the choice reads the value's tag
alone. So the walk gives the test of an `if`, a `cond` clause or a `while` the
test itself, and the forms inside the test that hand its value on inherit it.
The branch's node would be too late for a loop. A `while` node
lies past the loop, so a release there runs once for every pass. A `while`
whose test reads a variable the body assigns becomes a `loop` around an `if`
whose arm `recur`s. That arm jumps back before the `if` ends, so a release at
the `if` runs on the exit path alone.

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
the walk gave the `def` ([anchors.md](anchors.md) says why). An init that is a
read across a loop, of a binding no `assign` writes, has the loop node as its
floor, and a read of its binding that ends inside the loop does not lower it.

No step makes a last use smaller, so the walk's answer for an init stays a
lower bound on every read that depends on it. For a `let` or a `letrec` that
answer is the init itself, the least it can be. The binder node would be too
much: a binding captured by a closure that the `let` or `letrec` binds would
then live until that form ends, whatever the closure does. At the top level
the form is the file, and the value would live until the program ends.

`liveness::tests::chains` pins the chain through a closure, and
[region-toplevel-capture-release.lisp](../../../tests/impl/region-toplevel-capture-release.lisp)
the release it moves. `liveness::tests::inits` pins the forms inside an init,
and [region-toplevel-init-release.lisp](../../../tests/impl/region-toplevel-init-release.lisp)
the release they move. `liveness::tests::inits` also pins the floor of an
init that reads across a loop, and the init of an assigned binding's read that
takes no floor.
[loop-alias-reread.lisp](../../../tests/lang/loop-alias-reread.lisp) pins
the sum that a release inside the loop got wrong, and
[region-loop-counted-alias.lisp](../../../tests/impl/region-loop-counted-alias.lisp)
the reference that a release past the loop stranded on each pass.
`liveness::tests::branches` pins a branch's test, and
[region-loop-test-release.lisp](../../../tests/impl/region-loop-test-release.lisp)
the release a loop's test makes on each pass.
[region-loop-test-release-uaf.lisp](../../../tests/impl/region-loop-test-release-uaf.lisp)
is the soundness complement: a test's value that a name holds must survive
into the arm that reads it.
