# Region semantics — the model you write against

<!-- audited: 2026-09-22 -->

This is the consumer's view of *what* the memory system guarantees, so you can
write Elle that is sympathetic to it. For the implementor's correctness
obligations see [docs/impl/region/rules.md](../impl/region/rules.md).

Elle frees memory at compile-time-known program points: no tracing collector, no
liveness scan, no `Rc<Value>` (a `Value` is a `Copy` 16-byte tag+pointer). Every
value is born in a *region*; the region is freed at a point the compiler named,
and the regions that value referenced are decremented in turn. You never call
`free`, and there is no GC pause.

## Three mechanisms: Tofte–Talpin, reference counts, ownership

Knowing which mechanism frees a value tells you where its guarantees come from.

**Immutable values are pure TT.** In TT, `letregion ρ in e` binds a region whose
lifetime encloses `e`; an effect/escape analysis proves every value's lifetime is
bounded by some region, and regions are freed when their `letregion` exits. For
immutable data this is statically sound and complete: the compiler sees every
cross-region reference because immutable contents never change after construction.

**Mutation is what TT omits, so reference counting covers it.** TT has no
mutable reference. A mutable cell can be made to point at a value created later
or elsewhere *after the cell exists* — there is no static effect for "a store
that happens at runtime," so no static analysis can bound the pointed-to value's
lifetime. Elle closes this gap with per-region counts: a store into a mutable
container counts a reference to the stored value's region at runtime, and a
removal releases it.

**Ownership frees whole groups.** Where the compiler proves that a group of
regions is reachable only through one owner — a parent region, an activation,
or a fiber — it links them into an ownership forest
([impl/region/ownership.md](../impl/region/ownership.md)). An owned region
carries no count. It frees when its owner frees, with everything beneath it.
This is the direction of Project Verona's regions, reached by inference instead
of annotation. A region the compiler cannot prove owned keeps a count; that is
always legal, and never frees a value early.

## What escapes stays alive; what doesn't is freed at its last use

The practical consequence: a value is kept alive exactly as long as something
references it. If a value **escapes** — into a container, a closure, a yielded
signal, a returned result — the escape is counted or owned, so the value
outlives the scope that created it. If it does **not** escape, it is freed at
its last use. You do not arrange this; writing ordinary code gets it for free.
The mental model to write against is simply: *a value lives as long as it is
reachable, and not a step longer.*

## Cycles

A count alone never frees a cycle. `(push a b) (push b a)` leaves each of the two
regions counted by the other, so neither count reaches zero.

The ownership forest frees a cycle whose regions all sit in one owned subtree,
mutable or immutable. A subtree drop walks owners, not references, so the cycle
frees with its owner:

```lisp
(defn knot []
  (def a @[])
  (def b @[])
  (push a b)
  (push b a)
  (length a))

(def before (arena/region-count))
(var i 0)
(while (< i 200) (knot) (assign i (+ i 1)))
(assert (<= (- (arena/region-count) before) 1) "200 knots strand no regions")
```

A cycle leaks when the compiler cannot place all of its regions under one owner,
for example when part of the cycle is also referenced from outside the subtree.
Those regions keep their counts; the leak does not crash. Widening what the
forest can own is optimization work, tracked as leak class F4 in
[impl/memory.md](../impl/memory.md). A container stored into itself also leaks
today (#1228).

A related, narrower edge (true of every mutable container): a read consumed
*within the same expression* that also removes or overwrites the value
(`(list x (begin (assign x nil) 1))`) can observe the removal mid-expression. The
analysis does not order intra-expression reads against runtime removals; don't
mutate a value in the same expression that reads it.

## Regions are not scopes, fibers do not own them by birth, and ids are flat

- **Not born to a fiber.** A value a child fiber allocates and yields lives in
  its own region and outlives the child with no copy; the parent holds a 16-byte
  `Value` into those pages. A fiber can *become* an owner: the forest roots owned
  regions at activation and fiber owner nodes
  ([impl/region/owner.md](../impl/region/owner.md)), and a region that must
  outlive its fiber is counted or handed to a new owner. This is why long-running
  fiber schedulers don't accumulate garbage.
- **Not scopes.** A scope may hold values from many regions, and a value's region
  is set by where it dies, not by the scope it is written in. A scope exit can be
  the demise point for a region, but the scope does not own the region.
- **Flat ids, one forest.** Region ids are flat and opaque; they do not nest.
  The runtime relates them in two ways only: the ownership forest's parent-child
  links, and the recorded references from one region's contents into another.
