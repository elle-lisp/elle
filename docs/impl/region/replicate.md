# The relocation point and its replicas

<!-- audited: 2026-09-05 -->

How a relocation point outlives its own block, so one release covers a merge and
every path that leaves the frame before it.

## The relocation point outlives the block, and a branch merge inherits it

Inside the tail call's own block the relocation is a **move**: the instruction is
lifted from after the `TailCall` to before it, so it runs once on the closure path
and once on the native fall-through, and nothing is left behind.

A release the lowerer emits once that block has closed cannot be moved that way. A
branch arm's tail call is the shape that matters — the arm leaves through the
callee, so the enclosing scope's releases, emitted after the merge label, are
reached on every path except the ones that most need them. Moving such a release
into the arm would delete it on the sibling arms; leaving it alone strands it.

What resolves this is not a stronger placement claim but a property of the release
itself. A value-routed release is **self-cancelling**: it loads the holder slot,
releases that value's region, and stamps the slot `nil` — the same discipline that
lets a branch's per-arm compensations coexist with its `decref_point`
(`emit_branch_compensation`). Two copies of a self-cancelling run on one path
therefore act exactly once: whichever the path reaches first does the work, and
any later copy loads `nil`, whose release is a no-op. So the release is emitted at
the merge **and** replicated ahead of each arm's `TailCall`:

- an arm that leaves through the callee runs its own copy and never reaches the
  merge;
- an arm that falls through — natively, or because it makes no tail call at all —
  reaches the merge, where its copy either does the work or no-ops against the
  stamp the arm already left.

Every path releases exactly once, and no arm needs to be proven to tail-call for
the accounting to hold. The obligations are unchanged and are read **per
point**: a region an arm's own call names keeps its place there (that arm's copy
is the ownership move), and escape's admissions gate the whole thing, because
each replica still fires on a closure path where none fired before. Reading per
point is also what lets one arm's callee fund a returned region ([the
relocation](relocate.md)) while a sibling arm's callee, capturing nothing,
declines it.

Self-cancelling is a real restriction, not a formality. A release by region id
(`DecrefRegion`), a capture cell's `DecrefCellRegion`, and the transfer adopt
leave no stamp behind and would count twice on a native fall-through, so a run
that is not exactly load / release-by-value / nil-stamp keeps the baseline — and
for the id release that is a reason to change the ROUTE rather than to give up the
replica (§ below). Scope regions need nothing here anyway — `lower_call` already
frees them before every `TailCall`.

### Self-cancelling is a property of the ROUTE, not of the region's class

Of those three the id release differs in kind: leaving no stamp is not a fact
about the region. It is the lowerer's **default** route, taken because one
instruction does the work of four wherever a single point covers every path.
`region_to_slot` is keyed on a region's ALLOCATION site (`record_region_slot`), so
a region a `Define`/`Let`/`Letrec` binder allocated has a slot that names its
value from the binder to the release, and releasing what that slot holds frees the
same runtime region the id resolves to. The two routes are therefore
interchangeable at such a region, and only one of them replicates. So a release
the relocation has to replicate takes the value route, and every release it does
not keeps the id route.

Which regions have that slot is `RegionInfo::value_routed_regions`, the analysis's
mirror of `region_to_slot`, read by the branch-arm window so the two mechanisms ask
one question rather than two ([the branch-arm window](window.md)).

The reroute is asked only where some inherited point ADMITS the region, which
keeps it disjoint from the channel that answers the same strand a different way: a
release every point exempts — the merged arena riding the deferred slot
([the relocation](relocate.md)) — never changes route.
The route's own refusals travel with it, each naming a reason this slot is not
what the release reads: a **mutated** binder repoints its slot, an env cell's
release names the BOX rather than the slot, and a transfer consumer's release is
an adopt rather than a decref. Each of those keeps the id route, and with it the
whole-branch decline.

What the reading reaches is the `letrec` binding scope's own drop. A cell-free
self-recursive helper's closure region is no call result, so the id route is its
default, and its demise is the `Letrec` node — the binder is the scope
([selfrec.md](../selfrec.md)). A
body whose tail is a branch every arm of which leaves through a frame-replacing
callee arrives at that drop on no path at all, and the replica is what runs the
release on each arm instead. That is the shape a polymorphic entry point takes
when a `letrec` walker serves a dispatch whose arms tail-call out.

**Which merges inherit the points.** `if`, `cond` and `match` merges are reached
only through arms the lowerer closes one at a time, so each arm's points are
sealed onto its finished block and the merge starts life owning the union. Every
other block boundary clears them: a block that closes for any other reason is
followed by one the tail call's path may not be a predecessor of at all, and a
release replicated into an unreachable point is a release added on a path that
never owed it.

**A merge inherits what covered the branch's ENTRY as well.** The arms are one of
the two sources, not the whole of it. A branch is entered from one position, and
whatever points covered that position cover the merge too — the merge is reached
only through the branch, so the paths that arrive at it are exactly the paths that
arrived at the entry, minus the ones an arm's own tail call took away. Reading the
arms alone loses a point at the boundary the arms never touch: the condition
block, which closes like any other and clears what it was carrying, so a branch
that follows an earlier branch starts life covering nothing. Every release after
the second branch is then emitted at a merge the first branch's tail-calling arm
does not reach, with no replica to run it there — one region per call, plus its
cascade.

That is not a corner shape. **Functionalization inserts a branch of its own** for
every mutable a branch arm reassigns: the two versions of the name have to meet,
and they meet in an `If` on the same condition, emitted after the arm that already
carries the tail call. So a body whose tail is `(if p (begin … (assign i …) …
(f x)) …)` compiles to two branches, and the enclosing scope's releases land past
the second one. A loop is the everyday spelling of it, `while` and `each` alike
carrying an `assign` over their induction variable — which is why an `each` over an
empty sequence, whose body never runs, still strands what the scope around it
holds.

The two sources are collected separately because they are sealed differently. An
arm's own points name a block the arm just closed and are sealed AT that close; the
inherited ones are already sealed when the branch begins, and are read there —
`Finished` points only, a point still naming the open block dying with it exactly as
before. Neither source can double-count the other: a point handed to the merge from
the entry was never in an arm's `tail_exit_hoist` to be sealed, the block boundary
between the two having cleared it.

The residual is unchanged in kind: a holder escape marks by a facet no edge at
the point replaces.

Pinned by `tests/elle/region-tail-frame-exit.lisp` (the reclamation, with the
argument-move and callee exemptions, the per-arm faces, the captured-holder faces,
the non-self-cancelling boundary, the env-cell faces, the
handed-back-through-the-callee faces, the forward-cell faces, the
id-routed letrec closure whose body's tail is a branch, and the merge that
inherits its points from the branch's entry — the `if`, `when`, `cond` and loop
faces of the branch functionalization inserts for a reassigned local — driven as
rows),
the `tail-frame-exit-unused` /
`tail-frame-exit-moved` / `tail-frame-exit-arms` / `tail-frame-exit-captured` /
`tail-frame-exit-handback` / `tail-frame-exit-fold-driver` /
`tail-frame-exit-fwd-cell` / `tail-frame-exit-fwd-cell-ret` / `fresh-env-cell`
probes in `tests/elle/oracle.lisp` (the per-op rates), the analysis-level
projection pins in `regions::tests::cells`
(`frame_held_names_a_sibling_captured_forward_cell`,
`frame_held_names_a_returned_capturers_forward_cell`, and their
escaping-holder counterfactual), the placement pins in
`lir::lower::tests::release`, and
`tests/elle/region-tail-frame-exit-uaf.lisp` (the soundness complement — a value
moved into the tail callee, reached through its captured environment, filled in
place by it, handed back out through it, handed back when the frame holds the only
other reference, held in an env cell the callee rewrites, held in a sibling's
forward cell the callee reads on every recursion, captured by a closure
that escapes, or read after the call must survive the moved release).

## A `break` opens a relocation point too

A frame-replacing tail call is one way a path leaves before a release. A `break`
is the other. It jumps to its block's exit label, so a release the lowerer emits
while that block is still open is one the jump passed over, and for a region the
break does not carry there is no consumer to hand it to.

[The break window](anchors.md) re-anchors what it can onto the block's exit
label, and refuses on its loop barrier. A region the loop body ALLOCATES is
minted once per iteration, so one release at the exit would cover whichever
iteration's value the slot held last. That refusal is right, and it leaves one
path unserved: every iteration that falls through runs its own release, while the
iteration that BREAKS is the last. Nothing displaces its value, and no later
release reaches it.

So the break opens a relocation point of its own, at the end of the block it
leaves. A release emitted afterwards is emitted where the solver placed it AND
replicated there. The breaking path runs the replica; every other path runs the
placed release. The everyday shape is a drain loop whose clause body breaks:

```lisp
(forever
  (let [msg (s:data-queue:take)]
    (cond
      (= msg:type :data) (begin (push body-parts msg:data)
                                (when msg:end-stream (break nil)))
      (= msg:type :error) (error msg:error)
      true (break nil))))
```

`msg`'s last use is the second clause's TEST, so [the branch-arm
window](window.md) anchors its release at the `cond`'s own merge, and both
breaking bodies jump past it. Written with `if` the same loop strands nothing,
because two arms leave the release inside an arm rather than at a merge the
breaks skip. That is a placement fact about the branch, not about the op.

**The point lives exactly as long as the block is being lowered.** That is what
makes the count exact rather than approximate. Every position the lowerer fills
while the block is open is a position the jump passed over, and the exit label is
the first position the break path reaches. The point is gone by then, so no path
runs both copies; the nil stamp is a second net rather than the argument.

A replica is a release firing where none fired before, so it owes what the
relocation's replicas owe. Escape supplies the count argument
(`frame_held_regions`) and the run must be self-cancelling. The value the
**break carries** is exempt, read off the same operand slots a tail call's point
reads: releasing it here would free what the block is about to hand its consumer.
That value's own release stays where the transfer pin put it, at the point the
block's value is consumed.

A frame-replacing tail call in the block clears the points it dominates, the
break's among them, so a release emitted after one keeps the conservative
baseline. Dropping a licence to replicate can only over-keep.

What this closes is the h2 server's remaining per-request residue: a request with
a body read 2 objects and 2 regions where the same request as a `GET` read 0, and
the difference was the DATA-frame drain above.

Pinned by `tests/elle/region-break-loop-replica.lisp` (the reclamation — the
`cond` clause body, the release past the branch's merge, the `if` and bare-break
controls, and the three boundaries driven as rows), the per-request ceilings in
`tests/elle/h2-stress-scoped.lisp`, the placement pins in
`lir::lower::tests::release::breakexit`, and
`tests/elle/region-break-loop-replica-uaf.lisp` (the soundness complement — a
value the break carries out, one it carries a borrow out of, one a container
outside the loop still holds, and one a closure captured must all survive the
replica).
