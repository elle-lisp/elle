# The memory model

<!-- audited: 2026-11-21 -->

What Elle promises a program about memory: the fiber is the programmer's tool, each value has one owner, and release comes at last use.

[The specification](../spec.md) places this model among its commitments.
[The ownership rules](ownership.md) and [signatures](signature.md) state it
form by form, and [the judgment](judgment.md) says how it decides which
programs the language accepts.

## The fiber is the tool

The programmer's tool for memory management is the fiber, not scope and not
closures. Any implementation may erase scope and closures.

Release is 100% prompt, at last use, inside a fiber. An implementation chooses
when and where it allocates and releases a value inside a fiber, and it still
releases each value at that value's last use. Allocating a fiber's locals at
the fiber's birth, and freeing its regions at its death, does not conform.

## Owners

An owner is a fiber or a container. A binding is not an owner, and neither is
a function or a caller.

"A fiber" and "a container" each mean a region here. A container's region owns
its elements' regions, which die with it. A fiber owns many regions, and each
is released at the last use of what it holds. An owner and what it owns die as
one unit.

At each point, each heap value is in one of three states:

| State | Meaning |
|-------|---------|
| Owned | one named owner holds the value |
| Borrowed | a reader uses the value while a named owner holds it |
| Moved | the value has passed to a named owner |

## Regions

- A region's class is fixed at compile time, at its allocation site. No region
  changes class at run time, in either direction.
- An immutable value is not counted. It has one owner, named at birth. Other
  access is a borrow, another fiber's included.
- "Shared" and "single-owner" are different facts. A shared container has more
  than one reference. A single-owner container has one owner, however many
  references reach it. Counting follows owners.
- A single-owner mutable container is not counted. It and its contents die as
  one unit with the owner, internal cycles included.

## No class is counted yet

The discipline starts uncounted. A program it cannot handle is refused, and
each counted class added later makes more programs valid. So a container owns
its elements, and a read borrows.

Two classes are candidates, and neither is in the specification:

- A mutable container with more than one owner is counted.
- An immutable element of a mutable container may be counted where a reader
  can hold it across a point where another holder overwrites it.

A counted class enters the specification with three things. It brings a hand
proof that it keeps the two invariants of [leek](judgment.md), a clean run of
leek over generated programs of the class, and the list of programs refused
without it.
Each class is a language change that only accepts more.

## This implementation

[The region memory model](../impl/memory.md) says what this implementation
does, and [conformance](../impl/conformance.md) says where it differs from
this model.
