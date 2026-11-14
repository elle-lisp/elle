# The relocation point and its replicas

<!-- audited: 2026-11-13 -->

How a relocation point outlives its own block, so one release covers a merge and
every path that leaves the frame before it.

## The relocation point outlives the block, and a branch merge inherits it

Inside the tail call's own block the relocation is a **move**. The lowerer lifts
the instruction from after the `TailCall` to before it, so it runs once on the
closure path and once on the native fall-through, and nothing is left behind.

A release the lowerer emits once that block has closed cannot move that way. A
branch arm's tail call is the shape that matters: the arm leaves through the
callee. The enclosing scope's releases, emitted after the merge label, are then
reached on every path except the ones that most need them. Moving such a release
into the arm would delete it on the sibling arms; leaving it alone strands it.

A property of the release itself resolves this. A value-routed release is
**self-cancelling**: it loads the holder slot, releases that value's region, and
stamps the slot `nil`. The same discipline lets a branch's per-arm compensations
coexist with its `decref_point` (`emit_branch_compensation`). Two copies of a
self-cancelling run on one path therefore act exactly once: whichever copy the
path reaches first does the work, and a later copy loads `nil`, whose release is
a no-op. So the release is emitted at the merge **and** replicated ahead of each
arm's `TailCall`:

- an arm that leaves through the callee runs its own copy and never reaches the
  merge;
- an arm that falls through, natively or because it makes no tail call at all,
  reaches the merge. There its copy either does the work or no-ops against the
  stamp the arm already left.

Every path releases exactly once, and no arm needs a proof that it tail-calls for
the accounting to hold. The obligations are unchanged, and they are read **per
point**. A region an arm's own call names keeps its place there, because that
arm's copy is the ownership move. Escape's admissions gate the whole thing,
because each replica still fires on a closure path where none fired before.
Reading per point also lets one arm's callee fund a returned region ([the
relocation](relocate.md)) while a sibling arm's callee, capturing nothing,
declines it.

Self-cancelling restricts which runs replicate. A release by region id
(`DecrefRegion`), a capture cell's `DecrefCellRegion`, and the transfer adopt
leave no stamp behind, so each would count twice on a native fall-through. A run
that is not exactly load, release by value and nil-stamp keeps the baseline. For
the id release the lowerer changes the route instead, and keeps the replica.
Scope regions need nothing here: the tail call's lowering frees them before
every `TailCall`.

### Self-cancelling is a property of the route

The id release differs in kind from the other two, because leaving no stamp is
no fact about the region. It is the lowerer's **default** route, taken because
one instruction does the work of four wherever a single point covers every path.
`region_to_slot` is keyed on a region's ALLOCATION site (`record_region_slot`).
So a region a `Define`, `Let` or `Letrec` binder allocated has a slot that names
its value from the binder to the release. Releasing what that slot holds frees
the same runtime region the id resolves to.

The two routes are therefore interchangeable at such a region, and only one of
them replicates. So a release the relocation has to replicate takes the value
route, and every release it does not keeps the id route.
`RegionInfo::value_routed_regions`, the analysis's mirror of `region_to_slot`,
names the regions that have that slot. The branch-arm window reads it too, so
the two mechanisms ask one question ([the branch-arm window](window.md)).

The lowerer asks for the reroute only where some inherited point ADMITS the
region. That keeps it apart from the channel that answers the same strand
another way: a release every point exempts never changes route. The merged arena
riding the deferred slot is such a release ([the relocation](relocate.md)). The
route's own refusals travel with it, each a reason this slot is not what the
release reads:

| Refusal | Why the slot does not name the value |
|---------|--------------------------------------|
| A **mutated** binder | The binder repoints its slot |
| An env cell's release | The release names the BOX, not the slot |
| A transfer consumer's release | The release is an adopt, not a decref |

Each refusal keeps the id route, and with it the whole-branch decline.

What the reading reaches is the `letrec` binding scope's own drop. A cell-free
self-recursive helper's closure region is no call result, so the id route is its
default. Its demise is the `Letrec` node, because the binder is the scope
([selfrec.md](../selfrec.md)). Take a body whose tail is a branch, and whose
every arm leaves through a frame-replacing callee. That body reaches the drop on
no path at all, and the replica runs the release on each arm instead. A
polymorphic entry point takes that shape when a `letrec` walker serves a
dispatch whose arms tail-call out.

### Which merges inherit the points

An `if`, a `cond`, a `match` and the short-circuit pair `and`/`or` reach their
merge only through arms the lowerer closes one at a time. So each arm's points
are sealed onto its finished block, and the merge starts life owning the union.
Every other block boundary clears them. A block that closes for any other reason
is followed by one the tail call's path may not be a predecessor of at all. A
release replicated into an unreachable point is a release added on a path that
never owed it.

A short-circuit operand is an arm. `and` and `or` carry no branch in the source,
and the lowerer gives them one. Each operand stores its value into the result
slot. Every operand but the last branches on that value: to the done block where
the answer is settled, or to the next operand. So the done block is a merge
reached through every operand's block, and the operands are its arms.

Only the LAST operand inherits tail position, so it is the only arm that can
carry a frame-replacing tail call. That one arm is enough to strand the whole
release set the enclosing scope emits past the merge. A closure callee replaces
the frame, so the path through it reaches the merge no more than any other arm's
does. The common shape is a two-test predicate whose second test calls out:

```lisp
(defn waiting? [f]
  (let [s (fiber/status f)]
    (or (= s :new) (not (fiber/done? f)))))
```

`not` is a closure, so the `or`'s last operand replaces the frame. Without a
replica, the merge's release of the owned parameter `f` would be skipped on
every call the first test does not settle. The caller's `CallArgument` reference
would strand once per such call, and everything the argument's region holds
would strand behind it. A scheduler that asked this of a fiber would pin that
fiber for the life of the process.

A merge also inherits what covered the branch's ENTRY, so the arms are one of
two sources. A branch is entered from one position, and whatever points covered
that position cover the merge too. The merge is reached only through the branch.
So the paths that arrive at it are exactly the paths that arrived at the entry,
less the ones an arm's own tail call took away. Reading the arms alone loses a
point at the condition block, a boundary the arms never touch. That block closes
like any other and clears what it was carrying, so a branch that follows an
earlier branch starts life covering nothing.

Without the entry's points, every release after the second branch lands at a
merge the first branch's tail-calling arm does not reach. No replica runs it
there, so one region strands per call, with its cascade. Functionalization
inserts such a branch for every mutable a branch arm reassigns. The two versions
of the name meet in an `If` on the same condition, emitted after the arm that
already carries the tail call. So a body whose tail is
`(if p (begin … (assign i …) … (f x)) …)` compiles to two branches, and the
enclosing scope's releases land past the second one.

A loop is the everyday spelling of it, because `while` and `each` both carry an
`assign` over their induction variable. So an `each` over an empty sequence
strands what the scope around it holds, though its body never runs.

The two sources are collected separately because they are sealed differently.
An arm's own points name a block the arm just closed, and they are sealed AT
that close. The inherited ones are already sealed when the branch begins, and
the merge reads them there. Only `Finished` points are inherited; a point still
naming the open block dies with it. Neither source can double-count the other: a
point the entry hands the merge was never in an arm's `tail_exit_hoist`, because
the block boundary between the two cleared it.

The residual is unchanged in kind: a holder escape marks by a facet no edge at
the point replaces.

### What pins the replicas

| Pin | What it covers |
|-----|----------------|
| [region-tail-frame-exit.lisp](../../../tests/impl/region-tail-frame-exit.lisp) | The reclamation, the argument-move and callee exemptions, the per-arm faces and the non-self-cancelling boundary, driven as rows. The merge that inherits its entry's points, through the `if`, `when`, `cond` and loop faces of the branch functionalization inserts |
| [region-tail-frame-exit-capture.lisp](../../../tests/impl/region-tail-frame-exit-capture.lisp) | The captured-holder, env-cell and handed-back-through-the-callee faces |
| [region-tail-frame-exit-letrec.lisp](../../../tests/impl/region-tail-frame-exit-letrec.lisp) | The forward-cell faces, and the id-routed letrec closure whose body's tail is a branch |
| [oracle.lisp](../../../tests/impl/oracle.lisp) | The per-op rates of the `tail-frame-exit-unused`, `-moved`, `-arms`, `-captured`, `-handback`, `-fold-driver`, `-fwd-cell` and `-fwd-cell-ret` probes ([branch.lisp](../../../tests/impl/probe/branch.lisp)), and of `fresh-env-cell` ([direct.lisp](../../../tests/impl/probe/direct.lisp)) |
| `hir::region::infer::tests::cells` | The analysis-level projection: `frame_held_names_a_sibling_captured_forward_cell`, `frame_held_names_a_returned_capturers_forward_cell`, and their escaping-holder counterfactual |
| `lir::lower::tests::release` | The placement |
| [region-tail-frame-exit-uaf.lisp](../../../tests/impl/region-tail-frame-exit-uaf.lisp) | Soundness, the complement: each value below must survive the moved release |

The soundness file's values are one moved into the tail callee, and one reached
through its captured environment. Others are filled in place by the callee,
handed back out through it, or handed back when the frame holds the only other
reference. Others are held in an env cell the callee rewrites, held in a
sibling's forward cell the callee reads on every recursion, captured by a
closure that escapes, or read after the call.

The short-circuit face rides the same three instruments. Its per-op rates are
the dashboard's `tail-frame-exit-or-arm` and `tail-frame-exit-and-arm` rows, one
per operator, each driven through the arm its own polarity reaches. Its
placement pins are `lir::lower::tests::release::shortcircuit`. They read WHICH
release lands ahead of the arm's `TailCall` and which stays behind as the
ownership move, a position no rate can see.

Its soundness rows are the `sc-*` witnesses of the same UAF file. A value the
arm's callee moves, captures, hands back, stores into a longer-lived container,
or lets escape must survive the replica. A native callee's fall-through must run
the replica and the merge's own copy exactly once between them.

## A `break` opens a relocation point too

A frame-replacing tail call is one way a path leaves before a release. A `break`
is the other. It jumps to its block's exit label, so a release the lowerer emits
while that block is still open is one the jump passed over. A region the break
does not carry then has no consumer to hand it to.

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
placed release. The everyday shape is a drain loop whose clause body breaks,
the h2 server's DATA-frame drain ([lib/http2/server.lisp](../../../lib/http2/server.lisp)).
The function below compiles with the document and is never called, because it
needs a live stream:

```lisp
(defn drain-body [s body-parts]
  (forever
    (let [msg (s:data-queue:take)]
      (cond
        (= msg:type :data) (begin (push body-parts msg:data)
                                  (when msg:end-stream (break nil)))
        (= msg:type :error) (error msg:error)
        true (break nil)))))
```

`msg`'s last use is the second clause's TEST, so [the branch-arm
window](window.md) anchors its release at the `cond`'s own merge, and both
breaking bodies jump past it. Written with `if` the same loop strands nothing,
because two arms leave the release inside an arm rather than at a merge the
breaks skip. That is a placement fact about the branch alone.

The point lives exactly as long as the block is being lowered, and that makes
the count exact. Every position the lowerer fills while the block is open is a
position the jump passed over, and the exit label is the first position the
break path reaches. The point is gone by then, so no path runs both copies; the
nil stamp is a second net.

A replica is a release firing where none fired before, so it owes what the
relocation's replicas owe. Escape supplies the count argument
(`frame_held_regions`) and the run must be self-cancelling. The value the
**break carries** is exempt, read off the same operand slots a tail call's point
reads: releasing it here would free what the block is about to hand its consumer.
That value's own release stays where the transfer pin put it, at the point the
block's value is consumed.

A frame-replacing tail call in the block clears the points it dominates, the
break's among them, so a release emitted after one keeps the conservative
baseline. Dropping a license to replicate can only over-keep.

| Pin | What it covers |
|-----|----------------|
| [region-break-loop-replica.lisp](../../../tests/impl/region-break-loop-replica.lisp) | The reclamation: the `cond` clause body, the release past the branch's merge, the `if` and bare-break controls, and the three boundaries, driven as rows |
| [h2-stress-scoped.lisp](../../../tests/impl/h2-stress-scoped.lisp) | The per-request pins, for requests that carry a body |
| `lir::lower::tests::release::breakexit` | The placement |
| [region-break-loop-replica-uaf.lisp](../../../tests/impl/region-break-loop-replica-uaf.lisp) | Soundness, the complement: a value the break carries out, one it carries a borrow out of, one a container outside the loop still holds, and one a closure captured must all survive the replica |
