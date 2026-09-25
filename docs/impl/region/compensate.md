# Per-arm compensation

<!-- audited: 2026-09-28 -->

The releases a branch adds one per arm, each funded by a retain on its own node.
The head route takes an arm that never names the region, the tail route one that
does.

## The premises are read per route, never per holder

A value-routed release loads ONE slot, the allocating binder's
([the branch-arm window](window.md)). Every refusal and premise about that
release is therefore a claim about that binder. Asked of every holder, they
refuse the ordinary shapes:

- The **mutated taint** poisons the regions `mutated_route_regions` names
  ([the window](window.md) reads the same set), never every region a
  reassigned binding holds. A 1-slot container that merely stores the value
  cannot make the release mistarget — the release never loads its slot.
- The **live-in anchors** are the region's allocation sites plus the route
  binder's def, where the slot is written. An alias's or a functionalization
  version's def says nothing about where the value was born. Anchoring that def
  read a value stored inside an arm as born there, and refused the sibling-arm
  release. An env cell keeps its holder's def as an anchor, because its release
  names the box at the holder's env index, minted where the holder is bound.
- The **loop-invariant guard** asks the allocation sites alone. A cell the loop
  stores into is defined outside the loop, and reading its def as a birth
  refused the per-iteration release exactly where it is correct. A region with
  no recorded allocation falls back to its anchors, the only birth reading
  there is.

The conditional accumulate demanded all three at once. It is a loop that names
each element, then stores it into an outer 1-slot container in one arm of a
`when`. Its element release sat in the storing arm and leaked on every other
path ([region-cell-aliased-store.lisp](../../../tests/impl/region-cell-aliased-store.lisp)).
An aliased stored value takes the counted store of
[the 1-slot container](bindings.md).

## The return frontier is per-path

The return mint ([the mechanism](mechanism.md)) makes a returned region "the
caller's to free". Branch compensation therefore excludes a return-escaping
region, because compensating one would release a reference the caller now holds.

The exclusion is a property of a **path**, not of the region. Escape answers
*can this value reach a return*, which is true of the whole region the moment
**one** path returns it. Take the path that does not: the sibling arm of the
branch whose other arm returns the value. No mint fired there, so the caller
holds nothing, and the callee's own reference is the only one in existence.

Nothing releases that reference. The region's single `decref_point` sits in the
returning arm, and the return frontier covers a hand-over that did not happen.
The region is held to fiber teardown, and with it every member its free cascade
would have reclaimed, so the per-call cost is the whole subtree.

A return-escaping region is therefore admitted to **head** compensation
([compensate.rs](../../../src/hir/region/infer/compensate.rs)) on a sibling arm
that has no use of it. The premises ordinary compensation already establishes
carry the soundness whole:

- the region's `decref_point` is inside another arm, so its last use is inside
  the branch. Nothing uses it afterwards, so no mint for it fires after the
  branch either;
- this sibling arm contains no use of it, so no mint fires on this path;
- arms are mutually exclusive, so the head release and the `decref_point`
  release can never both run.

Every one of those premises is stated over **one arm and its siblings**. None of
them mentions how many arms the branch has, or whether it is an `If` or a
`Match`. So a dead `Match` arm is admitted by the identical argument. The
dominant polymorphic shape is exactly a `Match`: `(match (type-of x) …)` reaches
an arm that never touches a value whose single `decref_point` sits in a sibling.
Keying the admission on the branch kind held that whole family to fiber
teardown. A `Match` that matches *no* arm runs no body, so nothing fires, which
leaks and never over-frees.

The dual case is an arm that carries the value out while the `decref_point`
sits in a *sibling* arm. In `(if c xs (go … xs))`, the recursive arm's later use
wins the `decref_point` max, and the base case is left with a mint and no
release. That arm is a **used** sibling arm, so it takes the `tail` route. The
same-node retain guard admits it, as it admits the store and `-mut` container
compensations. Its release node is the `Return` itself. `lower_return` emits its
mint before the node's releases, so the per-arm decref drops the callee's own
reference and never the caller's.

Every base case of a `letrec` walk over a heap argument has this shape.
`(letrec [go (fn [i xs] (if (= i 0) xs (go (- i 1) (rest xs))))] …)` strands its
whole input list per call without the `tail` route.

Nested branches inside such an arm are covered only where the `decref_point` arm
is a sibling of the arm holding the return. An inner branch whose own arms
straddle the hand-over keeps the conservative baseline. That residual is a leak,
never an over-free.

Both routes are what a branch falls back to. The **window** may admit the region
instead, a returned one included ([the branch-arm window](window.md)). Its
single anchored release then covers every path, and neither route fires, since
neither finds a `decref_point` inside an arm any more.

[region-return-arm-escape-leak.lisp](../../../tests/impl/region-return-arm-escape-leak.lisp)
pins both faces: the non-returning arm is bounded, and the returned value
survives its caller's use.
[region-match-dead-arm-leak.lisp](../../../tests/impl/region-match-dead-arm-leak.lisp)
pins both faces for the `Match` arm, plus the return-escaping value whose dead
`Match` arm hands the caller nothing.

The **used** sibling arm is the residual, and its guard is not negotiable. A
release there is admitted only where a retain on the same node funds it: the
store, the `-mut` container, or the return mint above. The one other admission
is a release that names an env cell box, whose holders are known without a
retain (§ "A compensating release of an env cell names the box, not the
holder's slot").

The tempting generalization says "the arm's last-use node is decref-safe by
symmetry with the global `decref_point`, so release there unconditionally".
That is a placement argument masquerading as a count argument. It says the
release lands after this arm's last *named* use. It does not say the callee
holds the only reference.

An arm that used the region may have handed out a reference the solver does not
name. The reachable one is an uncounted borrow in a suspended frame's activation
region map. A release that reaches zero frees a region a parked fiber still
resolves through its slot, and the generation stamp detonates it at the resume
([generations.md](generations.md)). So an unfunded used sibling arm takes no
per-arm release. [The branch-arm window](window.md) closes that shape instead:
it anchors the region's single release where every arm reaches it. The
`match-used-arm` probe in [the branch probes](../../../tests/impl/probe/branch.lisp)
gauges it at zero.

### A compensating release of an env cell names the box, not the holder's slot

An **env cell**'s release is placed like any other, and relocated like any
other. A frame-replacing tail call in a branch arm carries the box's one
`DecrefCellRegion` ahead of its `TailCall` ([the relocation](relocate.md)). The
arm that *falls through* to the merge then finds nothing there, since the
release went into the sibling, and strands one box per call. The everyday shape
is a captured local read through a closure the branch calls in one arm only:
`(fn (n t) (def @c n) (let [g (fn () c)] (if t (g) 0)))`.

The branch-arm release window cannot carry it. Anchoring at the merge takes the
box back out of the arm the relocation moved it into. The merge's replica
placement needs a **self-cancelling** run: load, release by value, nil-stamp.
`LoadCaptureRaw` + `DecrefCellRegion` is not one. It leaves the holder as it
was, so a second copy on a native fall-through would count twice.

Compensation's per-arm routes need no such run, because they rest on arm
structure rather than on a stamp. The compensating release and the sibling's
relocated one are mutually exclusive, so exactly one runs per path and no merge
point is involved. The **head** route takes the arm that falls through naming
the cell's binding nowhere: a dead sibling arm.

Two of compensation's refusals would otherwise decline it, and both are claims
about a release **route** rather than about the region:

- a **mutated** holder repoints its slot, so a slot-routed release frees
  whatever the slot holds then. This release names the box, which
  `populate_env` mints once per activation. An `assign` never repoints the box;
  it writes the cell's *content* ([the branch-arm window](window.md)).
- a **captured** holder is reachable through a closure's environment, which is
  why a slot-routed release of the captured *value* is refused. A capturer
  reaches the box through a counted `closure ⊇ cell` edge the funnel took when
  the env was built, never through the frame's slot
  ([the frame-held admission](held.md)).

So the refusals are read per region rather than per holder, exactly as the
frame-exit admission reads them. A `cell_release_regions` member keeps its
holder's mutation and its holder's capture. The head route's own premise
supplies the count, unchanged: the arm creates no reference to the cell. The
release drops the frame's env-slot reference, and every other holder's
reference is a counted edge. Where the ownership forest claimed the cell
instead, that edge is an owning one, and the decref is a structural no-op.

The **`tail`** route carries the box too, on the arm that *reads* the cell's
binding while a sibling holds the `decref_point`. Two of that route's refusals
stand in the way, and neither is about the box:

- its **count argument** is a retain on the release's own node: a store's, a
  `-mut` container's, a return mint's. That retain proves that the arm's use of
  the region left no reference the solver cannot name. A cell release has no
  retain and needs none, because the box's holders are known without one. They
  are the frame's env slot and one counted `closure ⊇ cell` edge per capturer,
  because no use of the binding ever yields the box. `DerefCell` reads the
  cell's *content*, `assign` writes that content, and a capture takes the
  funnel's edge. So the release drops the frame's own reference and nothing
  else, which is the head route's argument read at a later point in the arm.
- the **return frontier** withholds a region the caller now holds a reference
  to. What a return hands over is again the content, which lives in a different
  region. The caller never receives the box, and reaches it only through a
  closure that counts it, so the frontier has nothing to withhold here.

What the route still owes is placement. The box's per-arm release is a max over
the same pins the global `decref_point` is, restricted to this arm. The first
pin is each in-arm use's consuming node. The second is the reader of each
in-arm uncounted opcode read that borrows out of the cell
(`uncounted_read_sites`; [rules.md](rules.md) Rule 4's borrowing node), because
the box's cascade drops the cell's one reference to its *content*.

A candidate that lands outside the arm is a point this arm cannot host, since
ANF may float a consumer past its own arm. Both routes then decline the arm
rather than approximate it, since a head release there would precede the very
use that candidate came from. Otherwise the arms stay mutually exclusive, so
exactly one release runs per path. No merge point and no nil-stamp is involved,
which is what a cell release cannot supply.

[region-tail-frame-exit-capture.lisp](../../../tests/impl/region-tail-frame-exit-capture.lisp)
pins the shape in the `arm-cell`, `arm-cell-ro` and `arm-cell-read` rows, both
arms of each. The `env-cell-read-arm` probe in
[the direct probes](../../../tests/impl/probe/direct.lisp) gauges the per-op
rate.

The analysis pins are in
[region::infer::tests::compensate::cells](../../../src/hir/region/infer/tests/compensate/cells.rs):
`a_falling_through_arm_compensates_the_env_cell_its_sibling_relocated`,
`a_reassigned_holder_does_not_withdraw_its_env_cell_compensation` and
`an_env_cell_takes_the_tail_route_on_the_arm_that_reads_it`. The counterfactual
`an_unfunded_used_sibling_arm_takes_no_tail_route` keeps the retain requirement
on every other region.

The placement pins are in
[lir::lower::tests::release::frameexit::cells](../../../src/lir/lower/tests/release/frameexit/cells.rs):
`a_falling_through_arm_head_releases_the_env_cell_its_sibling_relocated` and
`a_reading_arm_tail_releases_the_env_cell_its_sibling_relocated`, beside the
decline `escaping_holder_env_cell_release_stays_after_the_tail_call`.

[region-tail-frame-exit-uaf.lisp](../../../tests/impl/region-tail-frame-exit-uaf.lisp)
is the soundness complement. A closure handed out through the compensated arm
must still rewrite and read its cell. The content a reading arm returns must
outlive the box. The box must outlive the reading arm through a capturer that
escaped with it.
