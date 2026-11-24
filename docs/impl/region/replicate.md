# The relocation point and its replicas

<!-- audited: 2026-11-24 -->

How a relocation point outlives its own block, so one release covers a merge and
every path that leaves the frame before it.

## The relocation point outlives the block, and a branch merge inherits it

Inside the tail call's own block the relocation is a **move**. The instruction
is lifted from after the `TailCall` to before it. So it runs once on the
closure path and once on the native fall-through, and nothing is left behind.

A release the lowerer emits once that block has closed cannot be moved that way.
A branch arm's tail call is the shape that matters. The arm leaves through the
callee, and the enclosing scope's releases are emitted after the merge label. So
they are reached on every path except the ones that most need them. Moving such
a release into the arm would delete it on the sibling arms, and leaving it
alone strands it.

A property of the release itself resolves this. A value-routed release is
**self-cancelling**: it loads the holder slot, releases that value's region, and
stamps the slot `nil`. The same discipline lets a branch's per-arm compensations
coexist with its `decref_point` (`emit_branch_compensation`). Two copies of a
self-cancelling run on one path therefore act exactly once. Whichever copy the
path reaches first does the work, and any later copy loads `nil`, whose release
is a no-op. So the release is emitted at the merge **and** replicated ahead of
each arm's `TailCall`:

- an arm that leaves through the callee runs its own copy and never reaches the
  merge;
- an arm that falls through, natively or because it makes no tail call at all,
  reaches the merge. There its copy either does the work or no-ops against the
  stamp the arm already left.

Every path releases exactly once, and no arm needs to be proven to tail-call for
the accounting to hold. The obligations are unchanged and are read **per
point**. A region an arm's own call names keeps its place there, because that
arm's copy is the ownership move. Escape's admissions gate the whole thing,
because each replica still fires on a closure path where none fired before.
Reading per point also lets one arm's callee fund a returned region
([the relocation](relocate.md)) while a sibling arm's callee, capturing nothing,
declines it.

Self-cancelling is a real restriction. A release by region id (`DecrefRegion`),
a capture cell's `DecrefCellRegion` and the transfer adopt leave no stamp
behind, so each would count twice on a native fall-through. A run that is not
exactly load, release by value and nil-stamp therefore keeps the baseline. For
the id release that is a reason to change the ROUTE rather than to give up the
replica, as the next section says. Scope regions need nothing here, because
`emit_pending_free_regions` already frees them before every `TailCall`.

### Self-cancelling is a property of the route

Of those three the id release differs in kind, because leaving no stamp is not a
fact about the region. It is the lowerer's **default** route, taken because one
instruction does the work of four wherever a single point covers every path.
`region_to_slot` is keyed on a region's ALLOCATION site (`record_region_slot`).
So a region that a `Define`, `Let` or `Letrec` binder allocated has a slot that
names its value from the binder to the release. Releasing what that slot holds
frees the same runtime region the id resolves to. The two routes are therefore
interchangeable at such a region, and only one of them replicates. So a release
the relocation has to replicate takes the value route, and every other release
keeps the id route.

Which regions have that slot is `RegionInfo::value_routed_regions`, the
analysis's mirror of `region_to_slot`. The branch-arm window reads it too, so the
two mechanisms ask one question rather than two ([the branch-arm
window](window.md)).

The reroute is asked only where some inherited point ADMITS the region. That
keeps it disjoint from the channel that answers the same strand a different way.
A release that every point exempts never changes route, and the merged arena
riding the deferred slot is one ([the relocation](relocate.md)). The route's own
refusals travel with it, because the replica reads the route the region's
`decref_point` release takes ([the region instructions](../../../src/lir/lower/regions.md)).
Each names a reason this slot is not what the release reads:

- a **mutated** binder repoints its slot, unless a counted cell stores the value
  ([bindings.md](bindings.md)), and a reassigned fn-local slot holds a later
  value;
- an env cell's release names the BOX rather than the slot;
- a transfer consumer's release is an adopt rather than a decref.

Each of those keeps the id route, and with it the whole-branch decline.

What the reading reaches is the `letrec` binding scope's own drop. A cell-free
self-recursive helper's closure region is no call result, so the id route is its
default. Its demise is the `Letrec` node, because the binder is the scope
([selfrec.md](../selfrec.md)). Take a body whose tail is a branch, every arm of
which leaves through a frame-replacing callee. That body reaches the drop on no
path at all, and the replica runs the release on each arm instead. A
polymorphic entry point takes that shape when a `letrec` walker serves a
dispatch whose arms tail-call out.

**Which merges inherit the points.** An `if`, a `cond`, a `match` and the
short-circuit pair `and`/`or` reach their merge only through arms the lowerer
closes one at a time. So each arm's points are sealed onto its finished block,
and the merge starts life owning the union. A block's exit label inherits the
points that cover its breaks ([the break's point](breakpoint.md)). Every other
block boundary clears them. The tail call's path may not be a predecessor of the
next block at all, and a replica in an unreachable point is a release added on
a path that never owed it.

**A short-circuit operand is an arm.** `and` and `or` carry no branch in the
source, and the lowerer gives them one. Each operand stores its value into the
result slot, and every operand but the last branches on that value. It branches
to the done block where the answer is settled, and to the next operand
otherwise. So the done block is a merge reached through every operand's block,
and the operands are its arms.

Only the LAST operand inherits tail position, so it is the only arm that can
carry a frame-replacing tail call. That one arm is enough to strand the whole
release set the enclosing scope emits past the merge. A closure callee replaces
the frame, so the path through it reaches the merge no more than any other
arm's path does. The everyday shape is a two-test predicate whose second test
calls out:

```lisp
(defn waiting? [f]
  (let [s (fiber/status f)]
    (or (= s :new) (not (fiber/done? f)))))
```

`not` is a closure, so the `or`'s last operand replaces the frame. Without a
replica, the merge's release of the owned parameter `f` would be skipped on
every call the first test does not settle. The caller's `CallArgument` reference
would strand once per such call, and everything the argument's region holds would
strand behind it. A scheduler that asked this of a fiber would pin that fiber
for the life of the process.

**A merge inherits what covered the branch's ENTRY as well.** The arms are one
of the two sources, not the whole of it. A branch is entered from one position,
and whatever points covered that position cover the merge too. The merge is
reached only through the branch. So the paths that arrive at it are the paths
that arrived at the entry, minus the ones an arm's own tail call took away.

Reading the arms alone loses a point at a boundary the arms never touch. The
condition block closes like any other and clears what it was carrying. So a
branch that follows an earlier branch starts life covering nothing. Every
release after the second branch is then emitted at a merge the first branch's
tail-calling arm does not reach. No replica runs it there, so it strands one
region per call, plus its cascade.

That is not a corner shape. **Functionalization inserts a branch of its own** for
every mutable a branch arm reassigns. The two versions of the name have to meet,
in an `If` on the same condition, emitted after the arm that already carries
the tail call. So a body whose tail is `(if p (begin … (assign i …) … (f x)) …)`
compiles to two branches, and the enclosing scope's releases land past the
second one. A loop is the everyday spelling of it, because `while` and `each`
both carry an `assign` over their induction variable. That is why an `each` over
an empty sequence, whose body never runs, still strands what the scope around it
holds.

The two sources are collected separately because they are sealed differently.
An arm's own points name a block the arm just closed, and are sealed AT that
close. The inherited ones are already sealed when the branch begins, and are
read there. Only `Finished` points are inherited, and a point still naming the
open block dies with it as before. Neither source can double-count the other. A
point handed to the merge from the entry was never in an arm's
`tail_exit_hoist` to be sealed, because the block boundary between the two
cleared it.

The residual is unchanged in kind: a holder escape marks by a facet no edge at
the point replaces.

The pins:

- [region-tail-frame-exit.lisp](../../../tests/impl/region-tail-frame-exit.lisp),
  the reclamation, driven as rows. It holds the argument-move and callee
  exemptions, the per-arm faces and the non-self-cancelling boundary. It holds
  the merge that inherits its points from the branch's entry, in the `if`,
  `when`, `cond` and loop faces of the branch functionalization inserts.
- [region-tail-frame-exit-capture.lisp](../../../tests/impl/region-tail-frame-exit-capture.lisp),
  the captured-holder, env-cell and handed-back-through-the-callee faces.
- [region-tail-frame-exit-letrec.lisp](../../../tests/impl/region-tail-frame-exit-letrec.lisp),
  the forward-cell faces, and the id-routed letrec closure whose body's tail is a
  branch.
- The per-op rates, as probes the [oracle](../../../tests/impl/oracle.lisp)
  dashboard runs: `tail-frame-exit-unused`, `tail-frame-exit-moved`,
  `tail-frame-exit-arms`, `tail-frame-exit-captured`,
  `tail-frame-exit-handback`, `tail-frame-exit-fold-driver`,
  `tail-frame-exit-fwd-cell`, `tail-frame-exit-fwd-cell-ret` and
  `fresh-env-cell`. They are defined in
  [probe/branch.lisp](../../../tests/impl/probe/branch.lisp) and
  [probe/direct.lisp](../../../tests/impl/probe/direct.lisp).
- The analysis-level projection pins in `hir::region::infer::tests::cells`:
  `frame_held_names_a_sibling_captured_forward_cell`,
  `frame_held_names_a_returned_capturers_forward_cell`, and their
  escaping-holder counterfactual.
- The placement pins in `lir::lower::tests::release`.
- [region-tail-frame-exit-uaf.lisp](../../../tests/impl/region-tail-frame-exit-uaf.lisp),
  the soundness complement. A value must survive the moved release when it is
  moved into the tail callee, reached through its captured environment, filled
  in place by it, or handed back out through it. It must survive when it is
  handed back while the frame holds the only other reference. It must survive
  in an env cell the callee rewrites, and in a sibling's forward cell the callee
  reads on every recursion. It must survive when a closure that escapes captures it,
  and when it is read after the call.

The short-circuit face rides the same three instruments. Its per-op rates are
the `tail-frame-exit-or-arm` and `tail-frame-exit-and-arm` rows of the same
dashboard. Each is driven through the arm its own operator's polarity reaches.
Its placement pins are `lir::lower::tests::release::shortcircuit`. They read
WHICH release lands ahead of the arm's `TailCall` and which stays behind as the
ownership move, a position no rate can see. Its soundness rows are the `sc-*`
witnesses of the same UAF file. A value must survive the replica when the arm's
callee moves it, captures it, hands it back, stores it into a longer-lived
container, or lets it escape. A native callee's fall-through must run the
replica and the merge's own copy exactly once between them.

## A `break` opens one too

A `break` opens a point at the end of the block it leaves.
[The relocation point a break opens](breakpoint.md) owns that point, and what a
block's exit label inherits.

## A branch operand's arm opens one too

A tail call's operand that is a branch opens a point at the end of each arm. The
point takes a replica only of a release whose region the call receives on a
sibling arm alone, or postpones that release to the call where a later operand
reads the region. [What a tail-call operand hands the call](operand.md) owns
which regions it admits and postpones.
