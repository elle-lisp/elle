# The ownership rules

<!-- audited: 2026-11-18 -->

The rules that say who owns each heap value at each point of a program, and which programs the language refuses.

[The memory model](memory.md) defines an owner, a borrow and a move.
[Signatures](signature.md) holds the rules for a call and a return, and
[the judgment](judgment.md) says how the rules decide acceptance. A rule has a
name and no number, and one rule cites another by name.

## Values and containers

| Rule | Statement |
|------|-----------|
| Unheld value | A value that no container holds is owned by its fiber, and it is released at its last use. |
| Read | A read of a name, or of an element out of a container, borrows the value from its owner. |
| Store | A store into a container moves a value its fiber owns to that container, and a name that still reaches the value then borrows it from the container. |
| Store inside a unit | A store into a container of a value its fiber does not own is accepted only when the value's region already dies with that container's region, and any other such store is refused. |
| Removal | A removal or an overwrite takes a value out of its container, and the fiber that does it then owns the value, so the value is released at its last use. |
| Internal reference | A reference that a store makes under the Store inside a unit rule is a borrow that lasts while its container lives, so a value removed from a unit is released no earlier than the death of every container that still refers to it. |
| Container waits | A container is released no earlier than the last use of any value borrowed from it. |
| Borrow before release | A borrow ends before its owner releases the value, and a program where a borrow can outlive that release is refused. |
| Allocation operand | An immutable value borrows each immutable heap operand its fiber does not own from that operand's owner, and that owner's release waits for the value's last use; every other heap operand is stored. |

The Store inside a unit rule is what admits a cycle inside one owner. It
refuses `(push acc item)` where `item` is `(get coll i)` and a heap value: the
element is borrowed from `coll`, so its region does not die with the region of
`acc`. The Allocation operand rule accepts the same work over a list, where
`(pair (first c) acc)` makes an immutable value that borrows.

## Closures and parameters

| Rule | Statement |
|------|-----------|
| Immutable capture | A closure borrows each immutable value it captures from that value's owner, and that owner's release waits for the closure's last use. |
| Mutable capture | A closure borrows a mutable value it captures where that borrow ends before the value's owner releases it, and otherwise the capture is a store under the Store rule and the Store inside a unit rule. |
| Parameter inheritance | A fiber made in a parameterize body captures each value the body binds, under the Immutable capture and Mutable capture rules, and a read of the parameter borrows the value from its owner. |

Where an implementation dissolves a closure, its captures are reparented, not
lost.

Two closures that must each own one mutable value are refused. That is the
closure-as-module pattern over mutable state, and it waits for a counted
class.

## Control flow

| Rule | Statement |
|------|-----------|
| Join | Where paths join, each value is in one state, and a value that one path moves and another keeps is released on the path that keeps it, at its last use there. |
| Recur | A recur hands each new value to the next pass as a call hands an argument to a parameter, and a value the previous pass reached is released at its last use, which may lie inside the new values. |
| Break | A break hands its value to the consumer of the block it leaves, as a return hands its value to the caller. |
| Eval | An eval borrows its datum for the call and returns a fresh value. |

## Crossing a fiber

`try` and `protect` run their body in a new fiber, so an emitted error always
crosses a fiber. No rule of its own covers an emit.

One rule governs a crossing, and it is a target
([the judgment](judgment.md)): a value passes to a holder whose hold is not
nested inside the giver's only by a move, and only its owner can move it. A
resume moves its value.

## Not decided

- What the allocation of a mutable value does with an operand its fiber does
  not own, beyond "stored".
- Whether a borrow that an immutable value holds follows that value into a
  container or a closure.
