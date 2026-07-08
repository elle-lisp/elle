# Region semantics — the model you write against

This is the consumer's view of *what* the memory system guarantees, so you can
write Elle that is sympathetic to it. For the implementor's correctness
obligations see [docs/impl/region/rules.md](../impl/region/rules.md).

Elle frees memory at compile-time-known program points: no tracing collector, no
liveness scan, no `Rc<Value>` (a `Value` is a `Copy` 16-byte tag+pointer). Every
value is born in a *region*; the region is freed at a point the compiler named,
and the regions that value referenced are decremented in turn. You never call
`free`, and there is no GC pause.

## Tofte–Talpin for immutable values, reference counting for mutation

The model is the Tofte–Talpin region calculus completed with reference counting
for exactly the case TT cannot express. Knowing which half you are in tells you
where the guarantees come from.

If you're familiar with Project Verona's region model, we're on our way there
but the first step is correct region sharing; the ownership model that replaces
our use of RC will come only after we have correct and leak-free sharing.

**Immutable values are pure TT.** In TT, `letregion ρ in e` binds a region whose
lifetime encloses `e`; an effect/escape analysis proves every value's lifetime is
bounded by some region, and regions are freed when their `letregion` exits. For
immutable data this is statically sound and complete: the compiler sees every
cross-region reference because immutable contents never change after construction.

**Mutation is what TT omits, so mutation is what reference counting covers.** TT
has no mutable reference. A mutable cell can be made to point at a value created
later or elsewhere *after the cell exists* — there is no static effect for "a
store that happens at runtime," so no static analysis can bound the pointed-to
value's lifetime. Elle closes exactly this gap with per-region RC: a store into a
mutable container increments the stored value's region at runtime; a removal
decrements it. The dividing line is precisely mutability. RC is not a competing
design — it is the dynamic completion of TT for the one construct TT leaves out.

## What escapes stays alive; what doesn't is freed at its last use

The practical consequence: a value is kept alive exactly as long as something
references it. If a value **escapes** — into a container, a closure, a yielded
signal, a returned result — the escape is counted, so the value outlives the
scope that created it. If it does **not** escape, it is freed at its last use.
You do not arrange this; writing ordinary code gets it for free. The mental model
to write against is simply: *a value lives as long as it is reachable, and not a
step longer.*

## The one thing that leaks: mutable cross-region cycles

RC is *safe* (it never frees a region with live references) but *incomplete* in
one way: it cannot reclaim a cycle of cross-region references built through
mutation. `(push a b)(push b a)` with `a`, `b` in distinct regions makes the two
regions mutually reference each other, so neither RC reaches zero. This **leaks**;
it does not crash. Purely immutable code cannot construct such a cycle, so the
incompleteness is confined to mutable back-edges. This is the single known gap,
named here so no one rediscovers it as a "bug" — if you build mutable cyclic
structures across regions, break the cycle yourself before dropping them.

A related, narrower edge (true of every mutable container): a read consumed
*within the same expression* that also removes or overwrites the value
(`(list x (begin (assign x nil) 1))`) can observe the removal mid-expression. The
analysis does not order intra-expression reads against runtime removals; don't
mutate a value in the same expression that reads it.

## Regions are not fibers, not scopes, and are flat

- **Not fibers.** A region is not owned by a fiber. A value a child allocates and
  yields lives in its own region with RC > 0 and outlives the child with no copy;
  the parent holds a 16-byte `Value` into those pages. A fiber's "own"
  allocations are simply the regions that happen to reach RC 0 when it dies. This
  is why long-running fiber schedulers don't accumulate garbage.
- **Not scopes.** A scope may hold values from many regions, and a value's region
  is set by where it dies, not by the scope it is written in. A scope exit can be
  the demise point for a region, but the scope does not own the region.
- **Flat.** Region ids are flat and opaque — no nesting, no parent/child, no tree
  at runtime. The solver may use trees to *compute* assignments; the output is a
  flat set of ids related only by the cross-region references RC tracks.
