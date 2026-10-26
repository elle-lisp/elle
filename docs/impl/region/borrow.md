# A borrowing read's result

<!-- audited: 2026-10-21 -->

How long a value read out of a container without a count keeps the container, and why a name bound to that value is no holder.

[rules.md](rules.md) states the obligation. An uncounted element read,
the `%first`, `%rest` or `%get` opcode, raises no count on the value it hands
back. The container's lifetime is the value's only protection, so the
container's release must follow every use of the value. This document owns how
the solver finds those uses once the value leaves the read.

## The read's consumer is not the last use of a bound result

`last_use` at the read names where the value goes next. In operand position
that is the consumer, and the container's release extends to it
(`uncounted_read_sites`). A binder's override moves `last_use` only on the
binder's own init node. A read nested inside that init keeps the inner node as
its consumer, for example the tail of a `begin`:

```lisp
(defn tail-length [i]
  (let [xs (%pair i (%pair (string "s" i) ()))]
    (let [t (let [p (begin nil (%rest xs))] p)]
      (length (first t)))))

(assert (= (tail-length 7) 2) "the borrowed tail is read after the binders")
```

Read off the consumer alone, `xs` is released at the inner `let`, before
`first` reads `t`. A second read out of a bound result is worse off. Its
container is a name that holds no region, so the read records no container at
all. [region-borrow-chain-uaf.lisp](../../../tests/impl/region-borrow-chain-uaf.lisp)
faults on each shape. [region-borrow-chain-graft-uaf.lisp](../../../tests/impl/region-borrow-chain-graft-uaf.lisp)
faults on the grafted list walk the language suite reached.

## A borrow follows the value to every name it reaches

The result of a read is a **borrow** of its container's regions. A pass after
the walk carries each borrow to every name the value reaches
(`RegionInfo::binding_borrow_regions`). The container's release then extends
over the uses of each such name, as it does over a holder's uses.

A borrow passes unchanged through each form that hands its value on:

- the init of a `let`, a `letrec` or a `def`;
- the tail of a `begin` or a `block`, and the value of each `break` to that
  block;
- each arm of an `if`, a `cond`, an `and`, an `or` or a `match`;
- each name a pattern binds out of a borrowed scrutinee or a borrowed
  destructured value;
- a `loop` parameter, from its init and from each `recur`;
- a second read, whose container is a borrowed name. The borrow it hands on is
  the first container's, joined to whatever the name itself holds;
- an `assign` whose store takes no count. A store into a captured cell takes
  one, and so does a counted store into a 1-slot container, so the borrow stops
  at each ([bindings.md](bindings.md)).

A call, a `fn`, an `emit` and an `eval` end the flow. Each of their results
carries a reference of its own: the return mint of a closure, the pass-through
retain of a native ([rules.md](rules.md)), or the mint of a resume value.
A closure's env takes the allocation scan's count on each value it captures.

Two consuming points carry a borrow past its names. A `destructure` extracts
its fields after its value's own last read, so the value's borrow extends to
the `destructure` node. A `block` hands its value to the node that consumes
it, so the borrow each `break` carries extends to that node. Each read also
records the borrow of its own container, so a read out of a borrowed name in
operand position extends the first container to the read's consumer.

Where a 1-slot container holds the borrowed value by a counted reference of its
own, the pin defers to that reference as the read's own pin does
([bindings.md](bindings.md)).

## A borrowed name is not a holder

A holder owns the reference a region's release gives back: the binding whose
init allocated the value, or a name for that same reference. A borrowed name
owns nothing. The container's own holders own the region. So the borrow is
kept apart from `binding_source_regions`, and each question that counts
holders reads that map alone:

- the merge seed's sole-holder gate ([merging.md](merging.md));
- the reassign gate's sole-held test ([bindings.md](bindings.md));
- the escape projections, which judge a region by its holders
  ([held.md](held.md)).

Counted as a holder, the name bound to a trailing `(%first x)` is a second
holder of `x`'s value, and the reassign gate refuses the container's donation.
A read out of a nested pair makes the outer pair held twice, and the merge seed
refuses the nest.

Each question about how long a region is read reads the borrow as well:

- the binding-chain extension of `decref_point`, and the tight last use the
  ownership obligation reads (`binding_last_use`, [adopt.md](adopt.md));
- branch compensation's per-region uses, so an arm that reads a borrowed name
  is not a dead arm ([compensate.md](compensate.md));
- the tail-call exemption, so a tail call that passes a borrowed name keeps
  the container's release behind the call ([relocate.md](relocate.md)).

`%get` also hands back its container's regions, so the name it binds is a
holder as well as a borrower. A store of its result then records an edge out
of the container. The ownership cut's external-uniqueness test reads that edge
([ownership.md](ownership.md)).

`region::infer::tests::borrow::chain` pins the placement and the holder split.
