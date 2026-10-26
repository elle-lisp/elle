# The branch-arm release window

<!-- audited: 2026-10-26 -->

Where a branch puts the ONE release of a region several arms use. The anchor is
the merge every path reaches, not the arm that happens to name it last.

## A release inside one arm is not a release on the other arms

[Per-arm compensation](compensate.md) *adds* a release per arm, and each
addition needs a count argument. The same structure answers a weaker question
with a **placement** argument alone: where should the region's ONE release
live?

A region's `decref_point` is the structurally-latest of its uses. When several
arms of a branch use it, "latest" resolves to a node inside **one** arm. Arms
are mutually exclusive, so an execution that takes a different arm does not run
the release early or late: that path does not emit it at all. The region is
held to fiber teardown, and with it every member its free cascade would
reclaim. "Structurally latest across the arms" is not a program point that any
single execution passes through.

Every execution does pass through the branch's own consuming node,
`last_use[branch]`. That is the node that consumes the branch's value, or the
branch itself when nothing does, and the lowerer emits its decrefs after the
merge label. So a `decref_point` that lands inside an arm is **re-anchored**
there. One release runs per execution, on every path. The only change is that
the region now lives to the end of the branch instead of to the end of one arm.

This is the break window's argument ([anchors](anchors.md)): a release moved
*later* can only over-keep. It neither relaxes nor replaces the per-arm guard,
since there is still exactly one release, now after every arm's last use. What
the window needs, and the break window does not, is a reason to believe the
release still has only this frame's reference to drop. The next section gives
that reason, and it is the window's real gate.

The shape this closes is the dominant polymorphic stdlib entry point: a
`(match (type-of a) …)` whose owned parameter is handed to a different callee in
each arm. `a`'s single `decref_point` lands in the textually-last arm that names
it. Every earlier arm then strands `a`'s whole region, a per-call cost equal to
the argument's entire object graph. Where a call site proves the argument's
type, the dispatch prunes to a single arm
([src/hir/typeinfer/prune.rs](../../../src/hir/typeinfer/prune.rs)) and never
reaches this. The cost is what every unproven call site pays.

Once a region's `decref_point` leaves the arms, `region::infer::compensate` no
longer finds it inside one, so neither the head nor the tail route fires for it.
The single anchored release is exactly what those compensating releases
approximated. The arm-structure premises they rest on are unchanged for every
region the window declines.

### An arm is a conditional position, not a syntactic arm body

An arm is a program region at most one of which runs per execution. For an `If`
and a `Match` that coincides with the syntactic arm body. For the
short-circuiting forms — `cond`, `and`, `or` — it does not, and the syntactic
reading is blind to exactly the position those forms put a release in.

A `cond`'s clause **tests** are conditional positions as much as its bodies
are: test *k* runs only where tests 0..*k*-1 all failed. Take a region live-in
to the form whose last use is a later clause's test. Its `decref_point` sits
where no earlier body's path passes, and no arm holds it. That is where the
polymorphic entry point puts it. `distinct` dispatches with
`(cond m … (trait/iterable? coll) … (array? coll) … (or (pair? coll) (empty? coll)) …)`,
naming `coll` in every test after the first. The argument's one release lands
in the LAST test, and every call that takes an earlier body strands the
argument's whole object graph. `and`/`or` do the same with one position:
`(or (array? v) (string? v))` never evaluates the second test when the first is
true.

The arms are read off the nested-`If` each form is equivalent to:

| Form | Read as |
|------|---------|
| `(cond t0 b0 t1 b1 … e)` | `(if t0 b0 (if t1 b1 … e))` |
| `(and e0 e1 … en)` | `(if e0 (and e1 … en) false)` |
| `(or e0 e1 … en)` | `(if e0 true (or e1 … en))` |

Each clause boundary of a `cond` contributes a two-armed branch: the clause
**body**, and **the rest of the chain** from the next test through the `else`.
`and`/`or` each contribute a single arm, their tail, since the short-circuit
path evaluates no node at all. The walk visits a form's parts in source order,
so every one of those spans is contiguous in post-order. Each is one interval,
and neither consumer learns a new shape. All levels of one form share its own
node, hence one whole-node interval and one anchor. Every level falls through
to the same merge, the form's own consuming node.

The last clause's sibling arm is the `else` branch, or **nothing** where the
form has none. A `cond` that matches no clause evaluates to `nil` having run no
body. That path offers no node to host a compensating release, so the pass does
not fire on it. This leaks and never over-frees, as a `Match` with no matching
arm already does.

Overlapping levels cost nothing, because each `decref_point` lands in the arms
of exactly the levels that need a release for it. One inside body *k* is in an
arm of level *k* and in no arm of any other level. One inside test *k* is in
the "rest" arm of every level below *k*, which is precisely the set of clause
bodies whose paths skip that test.

[tests/impl/region-branch-arm-window.lisp](../../../tests/impl/region-branch-arm-window.lisp)
drives the rows `cond-later-test`, `cond-else-path`, `cond-dispatch`,
`or-short` and `and-short`. Beside them, the `ctl-cond-last-test` and
`ctl-or-full` controls drive the path that does evaluate the position holding
the release.
[tests/impl/region-branch-arm-window-uaf.lisp](../../../tests/impl/region-branch-arm-window-uaf.lisp)
holds the `w-cond`, `w-cond-store` and `w-or-short` soundness rows.

The unit pins are in
[region::infer::tests::compensate::positions](../../../src/hir/region/infer/tests/compensate/positions.rs):
`a_cond_clause_test_is_a_conditional_position`,
`a_cond_body_is_an_arm_like_any_other`, `a_short_circuit_tail_is_an_arm` and
`an_and_tail_is_an_arm_too`. The production gauges are the `distinct`,
`pipeline`, `wrap-map` and `push-accum` probes of the
[tests/impl/oracle.lisp](../../../tests/impl/oracle.lisp) dashboard, defined in
[tests/impl/probe/direct.lisp](../../../tests/impl/probe/direct.lisp) and
[tests/impl/probe/container.lisp](../../../tests/impl/probe/container.lisp).

### The admission: this frame must be the region's only holder

The placement argument is enough *only* where this frame holds the region's one
reference. [The frame-held admission](held.md) decides that from escape's
facets, and the frame-exit relocation reads the same answer. A region the
admission refuses keeps its in-arm release and the per-arm compensation routes,
which carry a count argument instead.

The **mutated** refusal is the one compensation makes about a release *route*:
a slot repointed between the arm and the anchor frees whatever it holds then. It
is therefore asked of the one binding that owns that route, below.

#### A mutated holder poisons its value route, not its cell box

The mutated refusal is a claim about a *route*, so it reaches exactly as far as
the route does. A value-routed release loads the holder's slot, and frees the
region of whatever value it finds there. So a slot the program repoints cannot
carry one, and the release is skipped for such a slot entirely
([bindings.md](bindings.md)).

**One binding owns that route.** `region_to_slot` is keyed on a region's
**allocation** site (`record_region_slot`). The slot a value-routed release
loads belongs to the binding whose init allocated the region. Where no site in
this body allocates it, the slot belongs to the parameter the lambda prologue
recorded. Every other holder names the same value through a slot no release
ever reads.

So the mutated question is asked of the route's binding. A second name bound
*from* the value refuses nothing: a cursor an arm walks with repoints its own
slot and leaves the allocating binding's alone. That is the everyday `each`
over a list. The type dispatch receives the cons chain as `seq`, and the
`:list` arm opens with `(def @cur seq)`. Reading the mutation off `cur` would
hold `seq`'s whole chain for the life of the frame, while `seq`'s own slot stays
untouched.

**Four sites record a route, and no others.** `Define`, `Let` and `Letrec` are
the three the mirror carries in `binder_init_sites`, recorded at the same three
walk arms the lowerer records the slot at. The fourth is the **lambda
prologue**, which records a parameter's slot for the call-result regions its
value may name and for no others. So a mutated binding the mirror cannot name
is read by *what introduced it* rather than by a blanket refusal:

- a **parameter** poisons exactly the prologue's own set. That set is empty in
  practice, and by construction. `needs_capture` at parameter scope IS
  `is_mutated`, so a reassigned parameter is celled. The one region it names is
  that cell's, and a cell region is exempt throughout (below). The filter is
  stated anyway, so the refusal keeps tracking the route if the walk ever gives
  such a parameter a second region.
- a **`Loop` parameter** and a **pattern name** poison nothing at all. No site
  records a slot for either, so no value-routed release can load the slot their
  `assign` repoints. So a `def` with a mutable destructuring pattern,
  `(def (@a @b) …)`, refuses nothing. A refusal there would hold the whole
  scrutinee, whose release routes through the temp that produced it.
- a binding **two different binders** introduce keeps the whole-holder reading.
  Both binders record a route, and nothing says which one the release loads.
  That is a real ambiguity rather than a gap in the mirror.

The emitter states the same refusal at its two mutated-slot backstops, and the
emitter decides what runs. So an admission the emitter then declines over-keeps
rather than over-frees. The tests in
[region::infer::tests::compensate::mutated](../../../src/hir/region/infer/tests/compensate/mutated.rs)
are the references: `a_reassigned_destructured_name_refuses_nothing` for the
pattern name, `a_reassigned_parameter_has_no_route_but_its_box` for the
parameter, and `a_reassigned_allocating_binder_refuses_its_own_release` for the
refusal the reading keeps.
[tests/impl/region-destructured-cursor.lisp](../../../tests/impl/region-destructured-cursor.lisp)
measures the shape.

An **env cell**'s release is a different instruction against a different
object. `LoadCaptureRaw` + `DecrefCellRegion` names the cell **box**.
`populate_env` mints the box once per activation and never repoints it. An
`assign` writes the cell's *content* (`StoreCapture`, which increfs the new
content's region and drops the displaced prior), and leaves the box exactly
where it was. A reassignment therefore cannot make this release name a value
the solver did not mean. A `cell_release_regions` member is admitted with its
holder mutated. The emitter states the same exclusion at both of its
mutated-slot backstops (`emit_decref_for_region`), and the admission reads it
one step earlier.

What the cell region still owes is the count argument, unchanged. The frame's
reference is the box's allocation reference. A capturing closure's reference is
the funnel's counted edge ([held.md](held.md)). A closure that *escapes* carries escape's
capture facet onto the binding, which refuses the holder as it refuses any
other escaping one. The shape this closes is the env cell of a reassigned
capture whose frame ends in a closure tail call. The release would otherwise
sit in the dead fall-through and strand one box per activation.

One more separation keeps the placement fact honest. The ownership and merge
cuts admit a subtree when the root's drop **post-dominates** a member's last
use, a *lifetime* question. A release re-anchored onto a branch post-dominates
everything inside it. Reading the moved anchor there would admit cuts the
region's real lifetime does not support, and the subtree drop would then free a
member under a live borrow. So `RegionData` carries both: `decref_point`, where
the lowerer emits the release, and `lifetime_point`, the structural last use
the cuts read. Only the window ever separates them.

### The boundaries

Two boundaries bound the window, the same two the break window carries and for
the same reasons. Both are about *how many times* a release runs:

- **An iterative scope nested in the branch** (`While`/`Loop`) holding the
  `decref_point`. A release inside it runs per iteration. Hoisting it past the
  loop would leave one release covering N executions.
- **A `Lambda` nested in the branch** holding it. Its body's releases run in a
  different activation against a different frame's slots, which never reach
  this branch's merge label.

Each boundary is the scope's **body**, not the scope's own node. The lowerer
emits a node's releases after it finishes lowering that node. A
`decref_point` equal to the `While`/`Loop` node is therefore emitted *after* the
loop, and runs once per execution of the loop: the same count with which the
merge label is reached. A `Lambda` node reads the same way. The enclosing frame
emits its releases, and only its body runs elsewhere. So the containment test is
half-open on the high end: strictly inside the scope is a boundary, and the
scope's own node is not.

That distinction admits an ordinary class rather than a corner: a live-in
region that a loop nested in one arm READS. The loop-node extension ([the
binder's scope](anchors.md)) anchors every such read at the loop node. The
closed interval would therefore place the branch's only release under the arm
holding that loop. The rows are `arm-loop-read` and `arm-loop-read-local` in
[tests/impl/region-branch-arm-window.lisp](../../../tests/impl/region-branch-arm-window.lisp).
Beside them is the `bound-loop` boundary, whose value is born in the loop body
and whose release must stay there.

The region must also be **live-in** to the branch. A value born inside an arm
keeps its in-arm release, and the window moves only what the branch received.
The anchors of the premise are the region's allocation sites and the definition
of every holder binding that could be its route. `record_region_slot` keys
`region_to_slot` on a region's allocation site, so the slot a value-routed
release loads belongs to the binding whose init allocated the region. An
allocation inside the branch is exactly the shape the premise keeps out: its
slot holds garbage on every path that skips the arm.

An **alias**, a binding whose init merely names another binding, is no anchor
(`alias_binders`). Its init allocates nothing, so it records no slot and is
never the route. An arm that introduces one has not given birth to the value.
Every other holder's definition must lie outside the branch. A region with no
allocation site, such as an owned parameter's placeholder whose slot the lambda
prologue records, has only those holder definitions to offer. The rows that
separate the cases are `arm-alias-inside` and `bound-loop` in
[tests/impl/region-branch-arm-window.lisp](../../../tests/impl/region-branch-arm-window.lisp).
The born-in-an-arm soundness face is `w-born-in-arm` in the UAF file.

Regions whose release belongs to another mechanism are excluded as in
compensation: merge children, co-owned-group members, the mutated-slot 1-slot
containers, and anything already suppressed. **Capture cells** are excluded
here and only here. A cell release leaves no nil-stamp for a replica to no-op
against, so it takes compensation's per-arm routes instead
([the env cell box release](compensate.md)).

### An arm that leaves through a callee takes a replica, not the anchor

One arm shape does not reach the merge label at all. A tail call to a
*closure* replaces the frame, so that arm leaves through the callee. Suppose
the anchor had to be a point every arm reaches. That shape would then make the
branch decline whole, and take the dominant polymorphic stdlib entry point with
it. `append` hands a list argument to `append-list` in one arm, and `concat`
hands its arguments to `concat-seq` in one arm per family. Declining the branch
would strand the owned parameter's whole object graph on **every other** arm,
once per call.

The window needs a weaker premise: the release must **run once on every
path**. One point covering every path is only one way to achieve that. The
frame-exit relocation supplies the other ([the relocation point](replicate.md)).
A merge starts life owning the points its arms sealed, so a release emitted at
the anchor is also **replicated** ahead of each arm's `TailCall`. An arm that
leaves through its callee runs its own copy and never reaches the anchor. An arm
that falls through reaches the anchor, and no-ops against the `nil` stamp if it
already ran a copy.

So the window anchors whatever the arms end in, and the exemption already reads
per point. An arm whose call **names** the region keeps its copy in the dead
block. That release is the ownership move the callee's owned-parameter release
consumes.

The exemption's two halves do not read alike here, and the window has to tell
them apart. For an **argument**, the copy left in the dead block is exactly the
ownership move. The callee's owned-parameter release runs in its place
([rules.md](rules.md) Rule 5), so nothing is owed on that path, and the anchor
is free to take the release away. For the **callee's own** region there is no
such release. The deferred callee channel stands in for it, and that channel is
keyed on where the release SITS ([the relocation](relocate.md)).

Anchoring the callee's region at the merge takes it out of the channel's reach
and leaves the exiting arm with nothing at all. So the closure region through
which an exiting arm's call reaches its callee keeps its in-arm release. That
boundary is the `bound-callee` row. The leak it prevents is one closure region
per call, compounding with the depth of a tower of stdlib HOF compositions.

Neither mechanism owes a new count argument for the composition. Both make a
release fire on a path where none fired before. Both discharge exactly that
with `frame_held_regions`: the anchor at the analysis, each replica at its own
point. The **return**-facet class rides along on the same answer, at the anchor
and at every replica alike ([held.md](held.md)).

The composition does need a release the relocation can replicate, and only a
**value-routed** one qualifies. It loads the holder slot, releases that value's
region, and stamps the slot `nil`, so a second copy on one path no-ops. So the
frame-exit relaxation is asked per region, and it asks the emitter's own
question: **can a value route NAME this region**
(`RegionInfo::value_routed_regions`)? That is not the region's class.

Releasing by id is the lowerer's default, taken wherever a single point covers
every path. A region that a `Define`/`Let`/`Letrec` binder allocated has a slot
naming its value from the binder to the release. So it takes the value route as
soon as some point admits it ([the relocation point](replicate.md)). Reading the
class instead admits `call_result_regions` and declines every ordinary
binder-owned allocation. That is the everyday live-in local that a dispatch arm
tail-calls past.

The mirror is deliberately conservative. A region admitted here that the
emitter then releases by id gets no replica, *and* it has lost the per-arm
compensation the window displaced: one leak traded for another. So the mirror
carries the emitter's refusals. A captured binder's slot holds an env box or a
compiled cell rather than the value, and a reassigned binder's slot is
repointed. Everything the mirror declines keeps the whole-branch decline, and
with it compensation's head and tail routes.

Declining *inside* the arm instead would leave the anchored release covering
only the falling-through arms. The tail-calling arm, which per-arm compensation
used to reach at its head, would get nothing. This is `self_cancelling_run`'s
restriction read one step earlier, at the admission it builds on. It is the
same value-route line that compensation's `tail` route already draws.

[tests/impl/region-branch-arm-window.lisp](../../../tests/impl/region-branch-arm-window.lisp)
pins the reclamation. Its rows drive the loop and lambda boundaries, the `If`
face and the captured-holder face.
[tests/impl/region-branch-arm-tailcall.lisp](../../../tests/impl/region-branch-arm-tailcall.lisp)
drives the tail-callee boundary, the frame-replacing-arm faces and the
returned-parameter faces. The `param-used-arm`, `param-used-arm-if`, `branch-arm-tailcall-sibling`
and `branch-arm-return-captured` probes in
[tests/impl/probe/branch.lisp](../../../tests/impl/probe/branch.lisp) gauge the
per-op rates, and the oracle dashboard runs them.

The placement pins are
`fallthrough_arm_releases_though_a_sibling_tail_call_exits` and
`tail_call_argument_release_stays_the_ownership_move` in
[lir::lower::tests::release::arms](../../../src/lir/lower/tests/release/arms.rs),
and `moved_argument_takes_no_replica_in_the_arm_that_moves_it` in
[lir::lower::tests::release::armexit](../../../src/lir/lower/tests/release/armexit.rs).

[region::infer::tests::compensate](../../../src/hir/region/infer/tests/compensate.rs)
holds two more groups. The value-route narrowing pins are
`a_frame_replacing_arm_anchors_a_value_routed_release`,
`a_frame_replacing_arm_anchors_a_binder_routed_release`,
`a_callee_the_arm_tail_calls_keeps_its_in_arm_release`, and the mirror's own
`a_binders_allocation_is_value_routed` and
`a_celled_binders_allocation_is_not_value_routed`. The return-facet admission
pins are `a_capturing_frame_exit_anchors_a_returned_param`,
`a_returned_param_anchors_where_no_arm_leaves_the_frame` and
`a_frame_exit_the_callee_cannot_reach_anchors_a_returned_param`.

[tests/impl/region-branch-arm-window-uaf.lisp](../../../tests/impl/region-branch-arm-window-uaf.lisp)
is the soundness complement. After the branch, a value must survive the moved
release whether it is read, stored, returned, carried across a yield, reached
through a closure's environment, or moved into a sibling arm's tail callee.
