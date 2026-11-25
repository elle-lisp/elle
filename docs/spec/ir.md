# The region IR

<!-- audited: 2026-11-24 -->

A design, not yet built: the region IR is LIR with a release instruction and block parameters, and the verifier checks it.

The region IR is the code layer of [the specification](../spec.md).
[LIR](../impl/lir.md) is what this implementation lowers to today, and
[conformance](../impl/conformance.md) says how the one becomes the other.

## What it is

The region IR is LIR with a release instruction and block parameters added;
the verifier refuses each count instruction and each slot instruction, and the
flip deletes them.

So one IR type holds both lowerings until the flip. The present lowering emits
what it emits today. The new lowering emits no count instruction and no slot
instruction, and the specification's region IR is the part
[the verifier](judgment.md) accepts. The two disciplines never mix in one
program.

The instruction set is LIR's, with two instructions added. Each instruction
kind has one rule, in a match with no wildcard, and that rule states the facts
the instruction gives ([the facts](facts.md)).

## What it adds

| Addition | Meaning |
|----------|---------|
| Release | `release v` frees the region that holds the value `v`. A release names a value, never a region. |
| Param | `param i` writes the function's parameter `i` into a register. |
| Block parameters | A jump and a branch hand one argument to each parameter of the block they enter. An emit's resume block takes one parameter, the resume value. The region IR has no local slot. |

A value therefore has one definition. The Join, Recur and Break rules apply to
a jump as they are written ([the ownership rules](ownership.md)).

## What the verifier refuses

| Instructions | Why |
|--------------|-----|
| `IncrefRegion`, `DecrefRegion`, `IncrefValueRegion`, `DecrefValueRegion`, `DecrefCellRegion`, `DecrefPostponed`, `AdoptRegion`, `AdoptCellRegion`, `AdoptIntoActivation`, `FreeRegionGroup`, `AssertRegionMatches` | each belongs to the count protocol |
| `LoadLocal`, `StoreLocal` | each reads or writes a local slot |
| `Put`, `Del`, `IntrPush`, `IntrStringPush`, `IntrBytesPush` | each stores or allocates by a run-time tag |

The third row is a target ([the judgment](judgment.md)): the verifier refuses
these five as it refuses a count instruction. The new lowering reaches those
operations only through a call to a native with a declared signature.
[The facts](facts.md) names the other kinds the verifier refuses, each a kind
that has no rule yet.

A call instruction also carries six fields of the count protocol: `moves`,
`defer_callee_release`, `deferred_release_slots`, `borrowed_arg_slots`,
`late_mints` and `args_region`. The new lowering leaves each empty.

## Instances

[The Sink rule](signature.md) lets a call site choose a parameter's mode, so
one function runs as more than one instance.

The lowering emits one body per function, in which each release and each
onward call that depends on a parameter's mode is marked with that parameter.
An instance is that body stamped for one choice of modes, built when a call
site asks for it, and a Call names the instance it calls.

- A choice of modes says which parameters are sunk, and which parameters
  borrow from each sunk one.
- An instance may take an argument the function does not declare, which
  exists to be released.
- An instance holds plain releases and no mark. The verifier checks each
  instance as an ordinary function.
- No run-time mask chooses a release.
- A later unit stamps an instance from the body the callee's code object
  stores.

## A data form

The verifier and leek are written in Elle and read the IR as data: one struct
per function of a unit, children included. A test of either is hand-built IR
in that form. [The data form](form.md) gives its layout.

## Not written yet

- How a closure value reaches an instance stamped after the closure was built.
