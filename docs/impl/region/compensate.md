# Per-arm compensation

<!-- audited: 2026-09-05 -->

The releases a branch adds one per arm, each funded by a retain on its own node.
The head route takes an arm that never names the region, the tail route one that
does.

## The return frontier is per-path

The mint above is what makes a returned region "the caller's to free", and that is
why branch compensation excludes a return-escaping region: compensating one would
release a reference the caller now holds.

The exclusion is a property of a **path**, not of the region. Escape answers *can
this value reach a return* — true of the whole region the moment **one** path
returns it. Take the path that does not: the sibling arm of the branch whose other
arm returns the value. No mint fired there, so the caller holds nothing, and the
callee's own reference is the only reference in existence. Nothing releases it —
the region's single `decref_point` sits in the returning arm, and the return
frontier is covering a hand-over that did not happen. The region is held to fiber
teardown, and with it every member its free cascade would have reclaimed, so the
per-call cost is the whole subtree.

A return-escaping region is therefore admitted to **head** compensation
(`regions/compensate.rs`) on a sibling arm that has no use of it. The premises
ordinary compensation already establishes carry the soundness whole:

- the region's `decref_point` is inside another arm, so its last use is inside the
  branch — nothing uses it afterwards, hence no mint for it fires after the branch
  either;
- this sibling arm contains no use of it, so no mint fires on this path;
- arms are mutually exclusive, so the head release and the `decref_point` release
  can never both run.

Every one of those premises is stated over **one arm and its siblings** — none of
them mentions how many arms the branch has, or whether it is an `If` or a `Match`.
So a dead `Match` arm is admitted by the identical argument, and the dominant
polymorphic shape is exactly a `Match`: `(match (type-of x) …)` reaches an arm that
never touches a value the solver's single `decref_point` left in a sibling. Keying the
admission on the branch kind instead held that whole family — every dispatch whose
taken arm ignores a live local — to fiber teardown. A `Match` that matches *no* arm
runs no body, so nothing fires: the leak-preserving direction, never an over-free.

The dual case is an arm that carries the value out while the `decref_point` sits in
a *sibling* arm — `(if c xs (go … xs))`, where the recursive arm's later use wins
the `decref_point` max and the base case is left with a mint and no release. That
arm is a **used** sibling arm, so it takes the `tail` route, admitted by the same
same-node retain guard the store / `-mut`-container compensations carry: its release
node is the `Return` itself, and `lower_return`'s mint (emitted before the node's
releases) is what guarantees the per-arm decref drops the callee's own reference and
never the caller's. This is the shape every base case of a `letrec` walk over a heap
argument has — `(letrec [go (fn [i xs] (if (= i 0) xs (go (- i 1) (rest xs))))] …)`
strands its whole input list per call without it.

Nested branches inside such an arm are covered only where the `decref_point` arm is
a sibling of the arm holding the return: an inner branch whose own arms straddle the
hand-over keeps the conservative baseline. That residual is a leak, never an
over-free.

Both routes are what a branch falls back to. Where the **window** below admits
the region instead — a returned one included ([the branch-arm
window](window.md)) — the single anchored release covers every path and neither
route fires, since neither finds a `decref_point` inside an arm any more.

Pinned by `tests/elle/region-return-arm-escape-leak.lisp` (both faces: the
non-returning arm is bounded, and the returned value survives its caller's use), and
for the `Match` arm by `tests/elle/region-match-dead-arm-leak.lisp` (both faces
again, plus the return-escaping value whose dead `Match` arm hands the caller
nothing).

The **used** sibling arm is the residual, and its guard is not negotiable. A release
there is admitted only where a retain on the same node funds it (the store, the
`-mut` container, the return mint above), or where the release names an env cell box,
whose holders are known without one (§ "A compensating release of an env cell
names the box, not the holder's slot"). The tempting generalization — "the arm's
last-use node is decref-safe by symmetry with the global `decref_point`, so release
there unconditionally" — is a placement argument masquerading as a count argument.
It says the release lands after this arm's last *named* use; it does not say the
callee holds the only reference. An arm that used the region may have handed out one
the solver does not name, and the reachable one is an uncounted borrow in a
suspended frame's activation region map: a release that reaches zero frees a region
a parked fiber still resolves through its slot, and the generation stamp detonates
it at the resume ([generations.md](generations.md)). So an unfunded used
sibling arm keeps the conservative baseline — an over-keep, gauged by the
`match-used-arm` probe in `tests/elle/oracle.lisp`.

### A compensating release of an env cell names the box, not the holder's slot

An **env cell**'s release is placed like any other, and relocated like any
other: a frame-replacing tail call in a branch arm carries the box's one
`DecrefCellRegion` ahead of its `TailCall` ([the relocation](relocate.md)). The
arm that *falls through* to the merge then finds nothing there — the release
went into the sibling — and strands one box per call. The everyday shape is a
captured local read through a closure the branch calls in one arm only: `(fn (n
t) (def @c n) (let [g (fn () c)] (if t (g) 0)))`.

The branch-arm release window below cannot carry it. Anchoring at the merge takes the
box back out of the arm the relocation moved it into, and the merge's replica
placement needs a **self-cancelling** run — load, release by value, nil-stamp — which
`LoadCaptureRaw` + `DecrefCellRegion` is not: it leaves the holder as it was, so a
second copy on a native fall-through would count twice.

Compensation's per-arm routes need no such run, because they rest on arm structure
rather than on a stamp: the compensating release and the sibling's relocated one are
mutually exclusive, so exactly one runs per path and no merge point is involved. The
**head** route takes the arm that falls through naming the cell's binding nowhere —
a dead sibling arm.

Two of compensation's refusals would otherwise decline it, and both are claims about a
release **route** rather than about the region:

- a **mutated** holder repoints its slot, so a slot-routed release frees whatever the
  slot holds then. This release names the box, which `populate_env` mints once per
  activation and an `assign` never repoints — it writes the cell's *content*
  ([the branch-arm window](window.md)).
- a **captured** holder is reachable through a closure's environment, which is why a
  slot-routed release of the captured *value* is refused. A capturer reaches the box
  through a counted `closure ⊇ cell` edge the funnel took when the env was built, never
  through the frame's slot ([the branch-arm window](window.md)).

So the refusals are read per region rather than per holder, exactly as the frame-exit
admission reads them, and a `cell_release_regions` member keeps its holder's mutation
and its holder's capture. What supplies the count is the head route's own premise,
unchanged: the arm creates no reference to the cell, so the release drops the frame's
env-slot reference and every other holder's is a counted edge — or, where the ownership
forest claimed the cell instead, an owning one under which the decref is a structural
no-op.

The **`tail`** route carries the box too, on the arm that *reads* the cell's binding
while a sibling holds the `decref_point`. Two of that route's refusals stand in the
way, and neither is about the box:

- its **count argument** is a retain on the release's own node — a store's, a `-mut`
  container's, a return mint's. That retain buys the knowledge that the arm's use of
  the region left no reference the solver cannot name. A cell release has none and
  needs none: the box's holders are known without one. They are the frame's env slot
  and one counted `closure ⊇ cell` edge per capturer, because no use of the binding
  ever yields the box. `DerefCell` reads the cell's *content*, `assign` writes that
  content, and a capture takes the funnel's edge. So the release drops the frame's own
  reference and nothing else, which is the head route's argument read at a later point
  in the arm.
- the **return frontier** withholds a region the caller now holds a reference to.
  What a return hands over is again the content, which lives in a different region.
  The caller never receives the box, and reaches it only through a closure that counts
  it, so the frontier has nothing to withhold here.

What the route still owes is placement, and the box's per-arm release is a max over
the same pins the global `decref_point` is, restricted to this arm: each in-arm use's
consuming node, and — because the box's cascade drops the cell's one reference to its
*content* — the reader of each in-arm uncounted opcode read that borrows out of the
cell (`uncounted_read_sites`; [rules.md](rules.md) Rule 4's borrowing node). A
candidate that lands outside the arm is a point this arm cannot
host, since ANF may float a consumer past its own arm; the arm is then declined by
*both* routes rather than approximated, a head release there preceding the very use
that candidate came from. Otherwise the arms stay mutually exclusive, so exactly one
release runs per path; no merge point and no nil-stamp is involved, which is what a
cell release cannot supply.

Pinned by `tests/elle/region-tail-frame-exit.lisp` (the `arm-cell` / `arm-cell-ro` /
`arm-cell-read` rows, both arms of each), the `env-cell-read-arm` probe in
`tests/elle/oracle.lisp` (the per-op rate), the analysis pins in
`regions::tests::compensate`
(`a_falling_through_arm_compensates_the_env_cell_its_sibling_relocated`,
`a_reassigned_holder_does_not_withdraw_its_env_cell_compensation`,
`an_env_cell_takes_the_tail_route_on_the_arm_that_reads_it`, and the counterfactual
`an_unfunded_used_sibling_arm_takes_no_tail_route` that keeps the retain requirement
on every other region), the placement pins in `lir::lower::tests::release`
(`a_falling_through_arm_head_releases_the_env_cell_its_sibling_relocated` and
`a_reading_arm_tail_releases_the_env_cell_its_sibling_relocated`, beside the decline
`escaping_holder_env_cell_release_stays_after_the_tail_call`), and
`tests/elle/region-tail-frame-exit-uaf.lisp` (the soundness complement — a closure
handed out through the compensated arm must still rewrite and read its cell, the
content a reading arm returns must outlive the box, and the box must outlive the
reading arm through a capturer that escaped with it).

