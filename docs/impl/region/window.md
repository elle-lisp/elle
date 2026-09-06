# The branch-arm release window

<!-- audited: 2026-09-05 -->

Where a branch puts the ONE release of a region several arms use. The anchor is
the merge every path reaches, not the arm that happens to name it last.

## A release inside one arm is not a release on the other arms

Compensation above *adds* a release per arm, and each addition needs a count
argument. There is a weaker question the same structure answers with a
**placement** argument alone: where should the region's ONE release live?

A region's `decref_point` is the structurally-latest of its uses. When several
arms of a branch use it, "latest" resolves to a node inside **one** arm — and
arms are mutually exclusive, so on every execution that takes a different arm
the release is not early or late, it is *not emitted on that path at all*. The
region is held to fiber teardown, and with it every member its free cascade
would reclaim. "Structurally latest across the arms" is simply not a program
point any single execution passes through.

The point every execution does pass through is the branch's own consuming node —
`last_use[branch]`, the node that consumes the branch's value, or the branch
itself when nothing does — whose decrefs the lowerer emits after the merge
label. So a `decref_point` that lands inside an arm is **re-anchored** there.
One release per execution, on every path; the only thing that changed is that
the region now lives to the end of the branch instead of to the end of one arm.
This is the break window's argument — a release moved *later* can only over-keep
— and it neither relaxes nor replaces the per-arm guard above: there is still
exactly one release, now sitting after every arm's last use instead of after one
arm's. What it does need, and the break window does not, is a reason to believe
the release still has only this frame's reference to drop; the next section is
that reason and it is the window's real gate.

The shape this closes is the dominant polymorphic stdlib entry point: a
`(match (type-of a) …)` whose owned parameter is handed to a different callee in
each arm. `a`'s single `decref_point` lands in the textually-last arm that names
it, so every earlier arm strands `a`'s whole region — a per-call cost equal to
the argument's entire object graph. Where a call site proves the argument's type
the dispatch prunes to a single arm (`typeinfer/prune.rs`) and never reaches this
at all; the cost is what every unproven call site pays.

Once a region's `decref_point` leaves the arms, `regions::compensate` no longer
finds it inside one, so neither the head nor the tail route fires for it: the
single anchored release is exactly what those compensating releases were
approximating, and the arm-structure premises they rest on are unchanged for
every region the window declines.

### An arm is a conditional position, not a syntactic arm body

An arm is a program region at most one of which runs per execution. For an `If`
and a `Match` that coincides with the syntactic arm body. For the
short-circuiting forms — `cond`, `and`, `or` — it does not, and the syntactic
reading is blind to exactly the position those forms put a release in.

A `cond`'s clause **tests** are conditional positions as much as its bodies are:
test *k* runs only where tests 0..*k*-1 all failed. So a region live-in to the
form whose last use is a later clause's test has its `decref_point` where no
earlier body's path passes, and no arm holds it. That is where the polymorphic
entry point puts it. `distinct` dispatches with
`(cond (or (pair? coll) (empty? coll)) … (array? coll) … (array? coll) …)`,
naming `coll` in every test, so the argument's one release lands in the LAST test
and every call that takes an earlier body strands the argument's whole object
graph. `and`/`or` do the same with one position: `(or (array? v) (string? v))`
never evaluates the second test when the first is true.

The arms are read off the nested-`If` each form is equivalent to:

```
(cond t0 b0 t1 b1 … e)  ≡  (if t0 b0 (if t1 b1 … e))
(and e0 e1 … en)        ≡  (if e0 (and e1 … en) false)
(or  e0 e1 … en)        ≡  (if e0 true (or e1 … en))
```

So each clause boundary of a `cond` contributes a two-armed branch — the clause
**body**, and **the rest of the chain** from the next test through the `else` —
while `and`/`or` each contribute a single arm, their tail, the short-circuit path
evaluating no node at all. Every one of those spans is contiguous in post-order,
the walk visiting a form's parts in source order, so each is one interval and
neither consumer learns a new shape. All levels of one form share its own node,
hence one whole-node interval and one anchor: every level falls through to the
same merge, the form's own consuming node.

The last clause's sibling arm is the `else` branch, or **nothing** where the form
has none. A `cond` that matches no clause evaluates to `nil` having run no body,
so that path offers no node to host a compensating release and the pass does not
fire on it — the leak-preserving direction a `Match` with no matching arm already
takes.

Overlapping levels cost nothing, because each `decref_point` lands in the arms of
exactly the levels that need a release for it. One inside body *k* is in an arm
of level *k* and in no arm of any other level. One inside test *k* is in the
"rest" arm of every level below *k*, which is precisely the set of clause bodies
whose paths skip that test.

The rows are `cond-later-test`, `cond-else-path`, `cond-dispatch`, `or-short` and
`and-short` in `tests/elle/region-branch-arm-window.lisp`, beside the
`ctl-cond-last-test` / `ctl-or-full` controls that drive the path which does
evaluate the position holding the release; the `w-cond`, `w-cond-store` and
`w-or-short` soundness rows in `tests/elle/region-branch-arm-window-uaf.lisp`;
the unit pins
`regions::tests::compensate::a_cond_clause_test_is_a_conditional_position`,
`a_cond_body_is_an_arm_like_any_other`, `a_short_circuit_tail_is_an_arm` and
`an_and_tail_is_an_arm_too`; and the `distinct`, `pipeline`, `wrap-map` and
`push-accum` probes in `tests/elle/oracle.lisp` as the production gauges.

### The admission: this frame must be the region's only holder

The placement argument is enough *only* where this frame holds the region's one
reference. On the arms the window newly covers, a release fires where none did
before, so another holder it drops to zero is an over-free — and the reachable
other holder is an uncounted borrow in a frame that is **parked** when the release
runs, which the resume's uncounted-borrow check detonates on
([generations.md](generations.md)). No premise about arm structure discharges
that: it is a count question wearing a placement question's clothes, and it is the
same wall the per-arm route hits.

Escape answers exactly it, and is the sole authority for it
([escape.md](../escape.md)). What the admission needs from it is narrower than
"escapes", though: an **uncounted** second holder. So the facets split. The
**containment** facets — store, and capture by a closure that itself escapes —
hand the value to a holder the frame cannot see and, for a declared native store,
cannot count, and they refuse. The **return** and **fiber** facets each create a
holder that is counted at the crossing, and each rides along instead (below). So
the window is admitted for a region whose every holder binding is free of the
containment facets, whose own release route is unmutated, and which is absent from
the fiber frontier's atomless site half (which no binding names). A region with no
holder binding at all offers nothing to judge and is refused too. Everything else
keeps its in-arm release and the per-arm compensation routes above, which carry a
count argument instead — so the two mechanisms partition the obligation rather
than overlapping on it.

The **mutated** refusal is the one compensation makes about a release *route*: a
slot repointed between the arm and the anchor frees whatever it holds then. It is
therefore asked of the one binding that owns that route, below.

#### The return facet costs the merge nothing

A region on the **return** frontier is read after the frame is gone — by the
caller, through the reference a `Return` mints. The merge is a point *in this
frame*, so every path that reaches it has already run whatever mint it was going
to run. An arm that hands the region over minted the caller's reference before
jumping here, so the anchored release drops the frame's own and leaves the
caller's standing. An arm that hands nothing over leaves the frame's reference the
only one in existence, which is the same per-path reading the head route makes
([the return frontier](compensate.md)).

The arms that never arrive take a **replica** ahead of their own `TailCall` (§
"An arm that leaves through a callee takes a replica, not the anchor"), which
*is* a release before the callee's mint. That gap needs no edge at the point
either, and the reason is the enumeration the exemption already rests on: a
callee reaches a value this frame owns as an **operand** or through its
**captured environment**, and both ends of that enumeration are safe ([the
relocation](relocate.md)). So the return facet is admitted for the whole class,
at the anchor and at every replica alike, and what the point still decides is
only the exemption — a region an arm's own call names keeps its copy in the dead
block as the ownership move.

The leak this closes is the polymorphic helper that returns what it was handed —
`push-all`'s bulk arm, and with it every `append`/`concat` over a byte-family
argument, whose one release sits in the index-walk arm the call never takes — and,
through the replica, the index-walk fold driver behind `fold`/`reduce`/`concat`,
whose base arm returns the accumulator its recursive arm's callee cannot reach.

#### Lexical capture is not a second holder to fear

Reachability through a closure's environment looks like the counter-example to
every one of these admissions, and it is not, because the funnel already paid
for it. A by-value capture becomes scannable content of the `Closure` env, so
the allocation funnel's cross-region scan increfs the captured region when the
closure is built and the closure region's free-time cascade decrefs it again; a
capture materialized through a cell takes the same count at the cell store.
Where the ownership forest admits the containment instead, the capture lowers to
`AdoptRegion`/`AdoptCellRegion` and the member's RC is *frozen* — a decref
against an `Owned` region is a structural no-op ([ownership.md](ownership.md)).
Either way the closure's hold is a counted or an owning edge, never the
uncounted borrow this admission exists to protect, so a release of the frame's
own reference cannot be the one that reaches zero.

What *is* refused is capture by a closure that escapes **beyond the return facet**:
there the closure reaches a holder the compiler did not place, and escape's capture
facet already marks every binding such a closure captures. A closure the frame
merely hands back carries its captures on the same counted edge, so it refuses
nothing ([the relocation](relocate.md)). That is
a flow fact. The structural capture-graph
(`regions::escape::captured_bindings`) marks every captured binding whether or not
its closure ever leaves — the right conservatism for the **merge** gate, which
asks where a value may *live* and so needs raw reachability, and the wrong one
here, where the question is who holds a count.

#### A fiber crossing is a counted holder too

The fiber facet reads exactly as capture does, and for the same reason: every seam
that hands a value to another fiber counts a reference of its own before this frame
runs on. Each direction of each seam:

- **out, at a park.** The emit's `EmitEscape` retain is the delivery reference,
  consumed by the resumer's release of the resume result. Where the emitting body
  holds no reference to give up — a capture, an enclosing frame's parameter, a
  module-level binding — the compiler supplies one, so a park's payload carries
  exactly one body reference beside the delivery's
  ([owner.md](owner.md)).
- **in, at a resume.** The resumer pushes the value onto the parked frame's stack
  and takes nothing for it, so the `Emit` mints the reference the resumed body holds
  it by — released at that node's own `decref_point`, as a call result is
  ([owner.md](owner.md)).
- **send.** `chan/send`'s seam retains the message's region at the enqueue
  (`EscapeSite::ChanSend`) and holds it in the buffer until the receive lowers
  the count ([effects.md](effects.md)).

So a fiber crossing leaves a *counted* second holder, not the uncounted borrow this
admission exists to protect, and the frame's own release still drops the only
reference it owns. That is why the admission reads the containment facets
(`EscapeInfo::binding_escapes_by_containment`) rather than everything beyond
return. The two halves stand or fall together: withdraw the resume value's mint and
a body that parks again holding it reads the resumer's freed reference, which is
what `region-fiber-frontier-window-uaf.lisp` drives. The fiber frontier's **atomless
site half** still refuses — a value emitted or sent with no binding to name it is
judged by no holder here at all, so it keeps the conservative baseline the same way
a region with no holder binding does.

The leak this closes is the owned parameter a frame receives, hands to another
fiber on one path, and reaches the end of on every other. `wake-select-waiters`
takes the completed fiber by tail-call move from `complete-fiber` and resumes a
select waiter with it, so its release sat inside the arm that finds a waiter — and
a program with no select outstanding never runs that arm. Every `ev/spawn` /
`ev/join` pair stranded the fiber, the closure it was made from, and the
`[ok? value]` pair the join delivered.

#### A mutated holder poisons its value route, not its cell box

The mutated refusal is a claim about a *route*, so it reaches exactly as far as
the route does. A value-routed release loads the holder's slot and frees whatever
region the value it finds there lives in — which is why a slot the program
repoints cannot carry one, and why the release is skipped for such a slot
entirely ([bindings.md](bindings.md), "a mutated slot is not a release route").

**One binding owns that route.** `region_to_slot` is keyed on a region's
**allocation** site (`record_region_slot`), so the slot a value-routed release
loads belongs to the binding whose init allocated the region — or, where no site
in this body allocates it, to the parameter the lambda prologue recorded. Every
other holder names the same value through a slot no release ever reads. So the
mutated question is asked of the route's binding, and a second name bound *from*
the value refuses nothing: a cursor an arm walks with repoints its own slot and
leaves the allocating binding's alone. That is the everyday `each` over a list —
the type dispatch receives the cons chain as `seq`, the `:list` arm opens with
`(def @cur seq)`, and reading the mutation off `cur` held `seq`'s whole chain
for the life of the frame while `seq`'s own slot stayed untouched.

**Four sites record a route, and no others.** `Define`, `Let` and `Letrec` are the
three the mirror carries (`binder_init_sites`, recorded at the same three walk arms
the lowerer records the slot at); the fourth is the **lambda prologue**, which
records a parameter's slot for the call-result regions its value may name and for
no others. So a mutated binding the mirror cannot name is read by *what introduced
it* rather than by a blanket refusal:

- a **parameter** poisons exactly the prologue's own set. That set is empty in
  practice, and by construction: `needs_capture` at parameter scope IS `is_mutated`,
  so a reassigned parameter is celled, the one region it names is that cell's, and a
  cell region is exempt throughout (below). Stating the filter rather than relying on
  that keeps the refusal tracking the route if the walk ever gives such a parameter a
  second region.
- a **`Loop` parameter** and a **pattern name** poison nothing at all. No site
  records a slot for either, so no value-routed release can load the slot their
  `assign` repoints. That is what a `def` with a mutable destructuring pattern —
  `(def (@a @b) …)` — was refusing: reassigning one name held the whole scrutinee,
  whose release routes through the temp that produced it.
- a binding **two different binders** introduce keeps the whole-holder reading. Both
  binders record a route, and nothing says which one the release loads; that is a
  genuine ambiguity rather than a gap in the mirror.

The emitter states the same refusal at its two mutated-slot backstops, and it is the
emitter that decides what runs — so an admission the emitter then declines over-keeps
rather than over-frees. The references are the tests:
`a_reassigned_destructured_name_refuses_nothing` for the pattern name,
`a_reassigned_parameter_has_no_route_but_its_box` for the parameter,
`a_reassigned_allocating_binder_refuses_its_own_release` for the refusal the reading
keeps, and `tests/elle/region-destructured-cursor.lisp` for the measured shape.

An **env cell**'s release is a different instruction against a different object.
`LoadCaptureRaw` + `DecrefCellRegion` names the cell **box**, and the box is
minted once per activation by `populate_env` and never repointed: an `assign`
writes the cell's *content* (`StoreCapture`, which increfs the new content's
region and drops the displaced prior), leaving the box exactly where it was. A
reassignment therefore cannot make this release name a value the solver did not
mean, and a `cell_release_regions` member is admitted with its holder mutated —
the same exclusion the emitter already states at both of its mutated-slot
backstops (`emit_decref_for_region`), read one step earlier at the admission
those backstops build on.

What the cell region still owes is the count argument, unchanged. The frame's
reference is the box's allocation reference; a capturing closure's is the
funnel's counted edge (above); and a closure that *escapes* carries escape's
capture facet onto the binding, which refuses the holder as it refuses any other
escaping one. The leak this closes is the env cell of a reassigned capture whose
frame ends in a closure tail call, where the release would otherwise sit in the
dead fall-through and strand one box per activation.

One more separation makes the placement fact honest. The ownership and merge cuts
admit a subtree when the root's drop **post-dominates** a member's last use — a
*lifetime* question — and a release re-anchored onto a branch post-dominates
everything inside it. Reading the moved anchor there would admit cuts the region's
real lifetime does not support, and the subtree drop then frees a member under a
live borrow. So `RegionData` carries both: `decref_point`, where the lowerer emits
the release, and `lifetime_point`, the structural last use the cuts read. Only the
window ever separates them.

### The boundaries

Two bound the window, the same two the break window carries and for the same
reasons. Both are about *how many times* a release runs:

- **An iterative scope nested in the branch** (`While`/`Loop`) holding the
  `decref_point`. A release inside it runs per iteration; hoisting it past the
  loop would leave one release covering N executions.
- **A `Lambda` nested in the branch** holding it. Its body's releases run in a
  different activation against a different frame's slots, which never reach this
  branch's merge label.

Each boundary is the scope's **body**, not the scope's own node. The lowerer emits
a node's releases after it finishes lowering that node, so a `decref_point` equal
to the `While`/`Loop` node is emitted *after* the loop and runs once per execution
of the loop — the same count with which the merge label is reached. A `Lambda`
node reads the same way: the enclosing frame emits its releases, and only its body
runs elsewhere. So the containment test is half-open on the high end — strictly
inside the scope is a boundary, the scope's own node is not.

That distinction is what admits an ordinary class rather than a corner: a
live-in region a loop nested in one arm READS. The loop-node extension ([the
binder's scope](anchors.md)) anchors every such read at the loop node, so the
closed interval would place the branch's only release under the arm holding that
loop. The rows are `arm-loop-read` and `arm-loop-read-local` in
`tests/elle/region-branch-arm-window.lisp`, beside the `bound-loop` boundary
whose value is born in the loop body and whose release must stay there.

The region must also be **live-in** to the branch, so a value born inside an arm
keeps its in-arm release and the window moves only what the branch received.
"Born" is the **allocation**, and the release's route follows it:
`record_region_slot` keys `region_to_slot` on a region's allocation site, so the
slot a value-routed release loads belongs to the binding whose init allocated the
region — never to an alias, whose init merely names another binding and records no
slot. An allocation inside the branch is therefore the shape the premise exists to
keep out: its slot holds garbage on every path that skips the arm. A holder the
arm merely introduces is not a second birth and decides nothing.

So a region with an allocation site is live-in exactly when every one of those
sites is outside the branch. A region with none — an owned parameter's
placeholder, whose slot the lambda prologue records — has only its holder
definitions to offer, and every one of their sites must be outside. The rows that
separate the two are `arm-alias-inside` and `bound-loop` in
`tests/elle/region-branch-arm-window.lisp`; the born-in-an-arm soundness face is
`w-born-in-arm` in `region-branch-arm-window-uaf.lisp`.

Regions whose release belongs to another mechanism are excluded as in
compensation: merge children, co-owned-group members, the mutated-slot 1-slot
containers, and anything already suppressed. **Capture cells** are excluded here
and only here — a cell release leaves no nil-stamp for a replica to no-op
against, so it takes compensation's per-arm routes instead ([the env cell box
release](compensate.md)).

### An arm that leaves through a callee takes a replica, not the anchor

One arm shape does not reach the merge label at all: a tail call to a *closure*
replaces the frame, so that arm leaves through the callee. Read as "the anchor
must be a point every arm reaches", that shape would make the branch decline
whole — and it would take the dominant polymorphic stdlib entry point with it.
`append` and `concat` hand a list argument to `append-list` / `concat-seq` in one
arm, so on **every other** arm the owned parameter's whole object graph is
stranded, once per call.

The window needs a weaker premise than that reading states: the release must
**run once on every path**, which one point covering every path is only one way
to achieve. The frame-exit relocation supplies the other ([the relocation
point](replicate.md)): a merge starts life owning the points its arms sealed, so
a release emitted at the anchor is also **replicated** ahead of each arm's
`TailCall`. An arm that leaves through its callee runs its own copy and never
reaches the anchor; an arm that falls through reaches the anchor and no-ops
against the `nil` stamp if it already ran a copy. So the window anchors whatever
the arms end in, and the exemption already reads per point: an arm whose call
**names** the region keeps its copy in the dead block, because that release is
the ownership move the callee's owned-parameter release consumes.

The exemption's two halves do not read alike here, and the window has to tell them
apart. For an **argument**, the copy left in the dead block is exactly the
ownership move — the callee's owned-parameter release runs in its place (rules.md
Rule 5) — so nothing is owed on that path and the anchor is free to take the
release away. For the **callee's own** region there is no such release: what stands
in for it is the deferred callee channel, and that channel is keyed on where the
release SITS ([the relocation](relocate.md)). Anchoring it
at the merge takes it out of the channel's reach and leaves the exiting arm with
nothing at all. So the closure region an exiting arm's call reaches its callee
through keeps its in-arm release. That boundary is the `bound-callee` row, and the
leak it prevents is one closure region per call, compounding with the depth of a
tower of stdlib HOF compositions.

Neither mechanism owes a new count argument for the composition. Both make a
release fire on a path where none fired before, and both discharge exactly that
with `frame_held_regions` — the anchor at the analysis, each replica at its own
point. The **return**-facet class rides along on the same answer, at the anchor
and at every replica alike (§ "The return facet costs the merge nothing").

The composition does need a release the relocation can replicate, and only a
**value-routed** one qualifies: it loads the holder slot, releases that value's
region, and stamps the slot `nil`, so a second copy on one path no-ops. So the
frame-exit relaxation is asked per region, and the question it asks is the
emitter's own: **can a value route NAME this region**
(`RegionInfo::value_routed_regions`). That is not the region's class. Releasing by
id is the lowerer's default, taken wherever a single point covers every path, and a
region a `Define`/`Let`/`Letrec` binder allocated has a slot naming its value from
the binder to the release — so it takes the value route as soon as some point
admits it ([the relocation point](replicate.md)).
Reading the class instead admits `call_result_regions` and declines every ordinary
binder-owned allocation, which is the everyday live-in local a dispatch arm
tail-calls past.

The mirror is deliberately conservative, because a region admitted here that the
emitter then releases by id gets no replica *and* has lost the per-arm
compensation the window displaced — one leak traded for another. So it carries the
emitter's refusals: a captured binder's slot holds an env box or a compiled cell
rather than the value, and a reassigned binder's slot is repointed. Everything it
declines keeps the whole-branch decline, and with it compensation's head and tail
routes. Declining *inside* the arm instead would leave the anchored release
covering only the falling-through arms while the tail-calling arm, which per-arm
compensation used to reach at its head, got nothing. This is `self_cancelling_run`'s
restriction read one step earlier, at the admission it builds on, and it is the
same value-route line compensation's `tail` route already draws.

Pinned by `tests/elle/region-branch-arm-window.lisp` (the reclamation, with all
three boundaries, the `If` face, the captured-holder face, the frame-replacing-arm
faces and the returned-parameter faces driven as rows), the `param-used-arm` /
`param-used-arm-if` / `branch-arm-tailcall-sibling` / `branch-arm-return-captured`
probes in `tests/elle/oracle.lisp` (the per-op
rates), the placement pins in `lir::lower::tests::release`
(`fallthrough_arm_releases_though_a_sibling_tail_call_exits`,
`tail_call_argument_release_stays_the_ownership_move`,
`moved_argument_takes_no_replica_in_the_arm_that_moves_it`), the value-route
narrowing pins
(`regions::tests::compensate::a_frame_replacing_arm_anchors_a_value_routed_release`,
`a_frame_replacing_arm_anchors_a_binder_routed_release`,
`a_callee_the_arm_tail_calls_keeps_its_in_arm_release`, and the mirror's own
`a_binders_allocation_is_value_routed` /
`a_celled_binders_allocation_is_not_value_routed`),
the return-facet admission
(`regions::tests::compensate::a_capturing_frame_exit_anchors_a_returned_param`,
`a_returned_param_anchors_where_no_arm_leaves_the_frame`,
`a_frame_exit_the_callee_cannot_reach_anchors_a_returned_param`),
and `tests/elle/region-branch-arm-window-uaf.lisp` (the
soundness complement — a value read, stored, returned, carried across a yield,
reached through a closure's environment, or moved into a sibling arm's tail callee
after the branch must survive the moved release).

