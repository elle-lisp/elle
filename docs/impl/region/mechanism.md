# The mechanism

The RC-instruction machinery the [rules](rules.md) constrain: how each
region-RC instruction names its region, when the compiler may resolve it to a
static slot instead of a runtime value, and the two nets that keep a
mis-resolution from silently becoming a use-after-free.

A region owns pages and carries one `u32` reference count. RC starts at **1** —
the compiler's initial reference, i.e. the TT `letregion` owner. Cross-region
references raise it above 1.

- `IncrefRegion` raises RC. The runtime also auto-increfs at two points: scanning
  an immutable object's contents at allocation (`alloc_obj`), and storing into a
  mutable container at runtime.
- `DecrefRegion` lowers RC. At 0 the region's pages return to the pool **and**
  every region its contents reference is decremented (the cascade), recursively.
- `DecrefRegion` is the only demise instruction. There is no `FreeRegion`.

If a value escapes — into a container, a closure, a yielded signal — its region's
RC was already raised at the escape site, so the `DecrefRegion` at its `decref_point`
drops the initial reference without freeing. The region lives exactly as long as
RC says. There is no promotion pass that moves a value to a longer-lived region;
the value is born in the right region (Rule 3) and RC tracks the rest.

## Two resolutions: value-resolved and slot-resolved

Each region-RC instruction names its region in one of two ways:

- **value-resolved** (`IncrefValueRegion`/`DecrefValueRegion`): the operand is a
  *register*; the runtime reads the value and asks `region_of` for its physical
  region. This is the honest encoding whenever the region cannot be named at
  compile time — a passed-through arg, a branch-dependent mix, an opaque call
  result, a runtime mutable store. The prediction-free calling convention is
  built on it: a callee hands its caller one owning reference to the result's
  *runtime* region (`IncrefValueRegion` at every tail) and the caller consumes it
  (`DecrefValueRegion` at the result's `decref_point`), neither side naming the
  other's region statically.
- **slot-resolved** (`IncrefRegion`/`DecrefRegion`): the operand is a
  *`StaticRegion` slot*; the runtime resolves it through the current activation's
  `activation_region_map` to the physical region this execution minted for that
  slot. Usable only where the region is statically known.

## A call's result is named by the call's own region

Nothing of the callee's *interior* naming crosses the call boundary. Every call node
mints one `call_r` — the caller-side name for "whatever region the returned value
turns out to live in" — and its release is the value-resolved route above, so a
static region of the callee is never named in the caller.

That holds however much of the callee this compilation can see. The walk **inlines**
a resolvable lambda callee's body (`regions::walk::inline`) so the intrinsics buried
inside it record their cross-region edges at *this* call site — the whole reason the
inline exists. The regions that walk yields are the callee's, minted against the
callee's own nodes and remapped to fresh physical regions per activation, so they are
discarded: the caller's binding for the result holds `call_r`, exactly as it does for
an opaque callee.

Letting them through instead makes the caller a nominal holder of a region it never
allocates, and the `decref_point` machinery reads that fiction as fact:

- the holder's uses **extend the region's `decref_point`** into the caller. Where the
  caller's use sits in a branch arm mutually exclusive with the arm that does allocate
  the region — the base case of `(if p (mk …) (go …))`, whose recursive call inlines
  the same body and so yields the base arm's own result region — the region's one
  release is emitted on the only path that never mints it, and the allocating path
  emits none at all. The value route loads a slot holding `nil` there, so the release
  is inert as well as misplaced and the region is held to fiber teardown;
- the region gains a **second holder binding**, which disqualifies it from the
  single-holder value route `regions::compensate` needs, so the per-arm compensation
  that would otherwise cover the allocating arm declines as well.

This is the result-side half of one rule. The argument-side half is
`inline_bound_regions`, which keeps a `Return`/`Break` reached *inside* an inline from
extending a **caller** region's `decref_point` onto a callee node. Both say the same
thing: an inline is a device for collecting edges, not a splice, and the two
activations' namings must not mix.

Pinned by `regions::tests::inline::*`, the leak face
`tests/elle/region-inline-result-naming.lisp`, and the soundness complement
`region-inline-result-naming-uaf.lisp` — the caller holds exactly one release for the
result, so everything the callee hands back that is not freshly its own must ride a
counted edge.

## The return mint is emitted exactly once

The callee half of that convention is **one** mint per returned value: a function
hands its caller exactly one owning reference, and the caller's single
`DecrefValueRegion` at the result's `decref_point` consumes it. Two lowering
sites can supply it, and which one applies is decided by whether the result is
*named*:

- **the `Return` mint** (`lower_return`, marked on the HIR by
  `hir/return_incref.rs`) — the named path. ANF binds the tail value to a
  synthetic slot, so the frame holds its own reference; the mint raises RC and
  the binding's `decref_point` — extended past the mint by `return_sites` — drops
  the frame's reference, leaving net one for the caller.
- **the `TailCall` fall-through retain** (`lower_call`'s tail arm) — the
  anonymous path. A *native* tail call pushes no bytecode frame, so on normal
  completion the dispatch loop runs the post-`TailCall` block before the
  enclosing lambda returns. In a **propagating** tail position (a `let`/`lambda`
  body, which ANF deliberately leaves unnamed) there is no binding, hence no
  `decref_point` to balance a `Return` mint — the fall-through retain *is* the
  mint.

They cover the same value whenever ANF *does* name a tail call's result — the
canonical wrap `(let [t (f …)] (return t))`, which ANF builds for a tail call
nested in a `begin`/`if`/`cond`/`match` arm. Emitting both retains the result
twice against one release: an over-keep of one region per call, growing per
loop iteration. So the fall-through retain **stands down** whenever a `Return`
mint covers the same result (`return_minted_calls`), and the named path's
mint-then-release accounting carries the convention alone. A frame-replacing
*closure* tail call reaches neither instruction (the callee emits its own
`Return` mint), so the rule is uniform over callee kinds.

Two narrower sites already suppress the fall-through retain for the same
"exactly one reference" reason, and are unaffected: a `-mut` pass-through
store/remove funnel whose dispatch wrapper released the container owned-param
reference here (`container_release_sites`), and a moves-out ∩ `PassThrough`
native whose in-body escape retain is already the caller's reference
(`moves_out_release_sites`).

The pinning tests are `tests/elle/region-native-tail-compound-leak.lisp` (the
per-shape region-count deltas: bare, `let`-body, `begin`-nested, `if`-nested,
over Fresh / Funnel / pass-through natives) and `region-native-tail-return-uaf.lisp`
/ `region-hof-tail-return-uaf.lisp` (the soundness complement — the anonymous
path must keep its retain).

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

Both routes are what a branch falls back to. Where the **window** below admits the
region instead — a returned one included (§ "The return facet costs the merge
nothing") — the single anchored release covers every path and neither route fires,
since neither finds a `decref_point` inside an arm any more.

Pinned by `tests/elle/region-return-arm-escape-leak.lisp` (both faces: the
non-returning arm is bounded, and the returned value survives its caller's use), and
for the `Match` arm by `tests/elle/region-match-dead-arm-leak.lisp` (both faces
again, plus the return-escaping value whose dead `Match` arm hands the caller
nothing).

The **used** sibling arm is the residual, and its guard is not negotiable. A release
there is admitted only where a retain on the same node funds it (the store, the
`-mut` container, the return mint above), or where the release names an env cell box,
whose holders are known without one (§ "A compensating release of an env cell names
the box, not the holder's slot"). The tempting generalization — "the arm's
last-use node is decref-safe by symmetry with the global `decref_point`, so release
there unconditionally" — is a placement argument masquerading as a count argument.
It says the release lands after this arm's last *named* use; it does not say the
callee holds the only reference. An arm that used the region may have handed out one
the solver does not name, and the reachable one is an uncounted borrow in a
suspended frame's activation region map: a release that reaches zero frees a region
a parked fiber still resolves through its slot, and the generation stamp detonates
it at the resume (`generations.md` § "Uncounted-borrow check"). So an unfunded used
sibling arm keeps the conservative baseline — an over-keep, gauged by the
`match-used-arm` probe in `tests/elle/oracle.lisp`.

### A compensating release of an env cell names the box, not the holder's slot

An **env cell**'s release is placed like any other, and relocated like any other: a
frame-replacing tail call in a branch arm carries the box's one `DecrefCellRegion`
ahead of its `TailCall` (§ "A release past a frame-replacing tail call is not a
release"). The arm that *falls through* to the merge then finds nothing there — the
release went into the sibling — and strands one box per call. The everyday shape is a
captured local read through a closure the branch calls in one arm only:
`(fn (n t) (def @c n) (let [g (fn () c)] (if t (g) 0)))`.

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
  (§ "A mutated holder poisons its value route, not its cell box").
- a **captured** holder is reachable through a closure's environment, which is why a
  slot-routed release of the captured *value* is refused. A capturer reaches the box
  through a counted `closure ⊇ cell` edge the funnel took when the env was built, never
  through the frame's slot (§ "Lexical capture is not a second holder to fear").

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
(§ "The return frontier is per-path").

The arms that never arrive take a **replica** ahead of their own `TailCall`
(§ "An arm that leaves through a callee takes a replica, not the anchor"), which
*is* a release before the callee's mint. That gap needs no edge at the point
either, and the reason is the enumeration the exemption already rests on: a callee
reaches a value this frame owns as an **operand** or through its **captured
environment**, and both ends of that enumeration are safe (§ "The callee's return
mint, and why the point owes it nothing"). So the return facet is admitted for the
whole class, at the anchor and at every replica alike, and what the point still
decides is only the exemption — a region an arm's own call names keeps its copy in
the dead block as the ownership move.

The leak this closes is the polymorphic helper that returns what it was handed —
`push-all`'s bulk arm, and with it every `append`/`concat` over a byte-family
argument, whose one release sits in the index-walk arm the call never takes — and,
through the replica, the index-walk fold driver behind `fold`/`reduce`/`concat`,
whose base arm returns the accumulator its recursive arm's callee cannot reach.

#### Lexical capture is not a second holder to fear

Reachability through a closure's environment looks like the counter-example to
every one of these admissions, and it is not, because the funnel already paid for
it. A by-value capture becomes scannable content of the `Closure` env, so the
allocation funnel's cross-region scan increfs the captured region when the closure
is built and the closure region's free-time cascade decrefs it again; a capture
materialized through a cell takes the same count at the cell store. Where the
ownership forest admits the containment instead, the capture lowers to
`AdoptRegion`/`AdoptCellRegion` and the member's RC is *frozen* — a decref against
an `Owned` region is a structural no-op ([ownership.md](ownership.md) § "The
runtime: a reclamation typestate"). Either way the closure's hold is a counted or
an owning edge, never the uncounted borrow this admission exists to protect, so a
release of the frame's own reference cannot be the one that reaches zero.

What *is* refused is capture by a closure that escapes **beyond the return facet**:
there the closure reaches a holder the compiler did not place, and escape's capture
facet already marks every binding such a closure captures. A closure the frame
merely hands back carries its captures on the same counted edge, so it refuses
nothing (§ "The callee's return mint, and why the point owes it nothing"). That is
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
  ([owner.md](owner.md) § "Park/unpark symmetry").
- **in, at a resume.** The resumer pushes the value onto the parked frame's stack
  and takes nothing for it, so the `Emit` mints the reference the resumed body holds
  it by — released at that node's own `decref_point`, as a call result is
  ([owner.md](owner.md) § "A resume value crosses counted, or not at all").
- **send.** `chan/send`'s seam retains the message's region at the enqueue
  (`EscapeSite::ChanSend`) and holds it in the buffer until the receive lowers
  the count ([effects.md](effects.md) § `Sends`).

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

That distinction is what admits an ordinary class rather than a corner: a live-in
region a loop nested in one arm READS. The loop-node extension (§ "Every binder
records its scope") anchors every such read at the loop node, so the closed
interval would place the branch's only release under the arm holding that loop.
The rows are `arm-loop-read` and `arm-loop-read-local` in
`tests/elle/region-branch-arm-window.lisp`, beside the `bound-loop` boundary whose
value is born in the loop body and whose release must stay there.

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
against, so it takes compensation's per-arm routes instead (§ "A compensating release
of an env cell names the box, not the holder's slot").

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
to achieve. The frame-exit relocation supplies the other (§ "The relocation point
outlives the block, and a branch merge inherits it"): a merge starts life owning
the points its arms sealed, so a release emitted at the anchor is also
**replicated** ahead of each arm's `TailCall`. An arm that leaves through its
callee runs its own copy and never reaches the anchor; an arm that falls through
reaches the anchor and no-ops against the `nil` stamp if it already ran a copy.
So the window anchors whatever the arms end in, and the exemption already
reads per point: an arm whose call **names** the region keeps its copy in the
dead block, because that release is the ownership move the callee's
owned-parameter release consumes.

The exemption's two halves do not read alike here, and the window has to tell them
apart. For an **argument**, the copy left in the dead block is exactly the
ownership move — the callee's owned-parameter release runs in its place (rules.md
Rule 5) — so nothing is owed on that path and the anchor is free to take the
release away. For the **callee's own** region there is no such release: what stands
in for it is the deferred callee channel, and that channel is keyed on where the
release SITS (§ "What the exemption keeps, a channel must still run"). Anchoring it
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
admits it (§ "Self-cancelling is a property of the ROUTE, not of the region's
class").
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

## A binder's init release lands after the slot store

A binding's initializer is an ordinary expression, so `lower_expr` emits its
releases where it emits every node's: immediately after the node. That position is
**before** the binder has stored the value into its slot, and a release landing
there does the wrong thing on either route (§ "Two resolutions"): a **value-routed**
one reloads the holder slot, reads the `nil` the binder pre-stamped and releases
nothing at all, while a **slot-resolved** one names the region directly and frees a
value the binder is about to store — leaving the slot pointing into freed pages.

A release lands there only when the initializer is *itself* the region's
`decref_point`, which is the unused-binding narrowing: nothing reads the bound name,
so the value's last use is pulled back to where the value was made. `Let` and
`Letrec` therefore route the init node through `deferred_decref_points` and emit its
releases themselves, after the store (`tests/elle/region-unused-let-binding.lisp` is
the pin).

`Define` is the binder that must not be narrowed there in the first place, because
**a `def` evaluates to what it bound**. Every other binding form's value is its
*body*, so an init no name reads really is dead at the init; a `def`'s value IS the
init, so it is live wherever the `def` is — handed to a callee, returned, bound to a
second name, propagated out of a `begin` or a branch arm. The narrowing's floor for a
`Define` is therefore the point the walk gave the `def` itself
(`propagated_inits`, `hir/liveness/lastuse`): the enclosing consumer when there is
one, and the `Define` node when the `def`'s value is discarded — whose releases
`lower_expr` emits after `lower_define` has stored. Narrowing below that frees the
value under the expression it was handed to
(`tests/elle/region-define-init-release-uaf.lisp`); leaving it at the init frees
nothing (`tests/elle/region-define-init-release.lisp`).

So a `def`'s initializer region is released by the ordinary last-use mechanism,
whatever it holds. This is what a cell-free self-recursive `def` rides — its closure
region needs no suppression, because the binding's last use as a **callee** resolves
to the node that consumes it and the release lands where the recursion has already
completed ([selfrec.md](../selfrec.md) § the placement table).

## Every binder records its scope

A `Var` read inside a `While`/`Loop` is extended to the loop node when the binding
it names is bound **outside** that loop: the body re-reads it on every iteration,
so its region has to outlive the loop (`hir/liveness/lastuse`). The premise is a
containment test — is the binding's **scope node** a descendant of the loop? — and
it is only as good as the scope map is complete. A binder the walk does not record
has no scope node at all, and an absent scope is read as *bound outside*.

Both answers to that question are consequential, in opposite directions. Read as
bound **inside** when it is not, the release fires per iteration and the next
iteration reads a freed region — a use-after-free. Read as bound **outside** when
it is not, the release is hoisted past a loop whose body re-allocates the value
every iteration, so one release covers N allocations and N−1 regions are held to
fiber teardown — an unbounded leak. Neither direction is a safe default, which is
why the answer must come from a recorded fact rather than from absence.

So every binding form records its scope: `Define`, `Let`, `Letrec`, `Loop`,
`Destructure`, and a `Match` arm's **pattern**. The pattern is the one whose names
carry a region they did not allocate: a projection out of the scrutinee is an
uncounted read ([rules.md](rules.md) Rule 4's borrowing node), so it resolves to
the *scrutinee's* region, and the binding-chain extension carries the scrutinee's
release out to wherever the projection is last used. Unrecorded, an arm that reads
a name its pattern bound hoists the whole scrutinee's release past the enclosing
loop — every object the scrutinee holds, stranded once per iteration, on the arm
that runs and equally on one that never does (the extension is structural, so a
read in an arm no execution takes strands the scrutinee just the same).

A `Match` pattern records only its scope, not the init registration `Destructure`
also makes, and the difference is where the bound names are readable. A
`Destructure`'s names are read by *later siblings*, so the destructured value's own
last use must be extended to cover them. A `Match` arm's names are readable
strictly inside the `Match` node's subtree, and the scrutinee's last use is the
`Match` node itself — the branch consumes it — which already post-dates every read
of a projection in any arm. Registering an init would also expose the scrutinee to
the unused-binding narrowing (`compute_last_use`'s first phase pulls an init's last
use back to the init itself when no bound name is read), shrinking a lifetime the
`Match` node already states correctly.

Pinned by `tests/elle/region-match-bind-loop.lisp` (the reclamation, with the
arm-taken, arm-not-taken, nested-loop and guard faces driven as rows) and the
`struct-match` probe in `tests/elle/oracle.lisp` (the per-op rate), with
`tests/elle/region-match-bind-loop-uaf.lisp` as the soundness complement — a
pattern-bound projection stored, returned, broken out of the loop, captured, or
carried across a yield must survive the per-iteration release.

## `break` transfers its value; it does not consume it

A `Return` hands a value across a *function* frontier. A `break` is the
intra-function dual: it hands a value to the enclosing `block`, whose value is
its fall-through value **or** the value of any `break` targeting it. While the
block is *interior* to the function no reference changes hands — the value stays
in the same activation — so there is no mint; when the block is the function's
**tail** the break's value is also the function's result and takes the ordinary
return mint (below). What a `break` does change is *where the value dies*, and by
two compounding facts, neither of which the ordinary consuming-node treatment
(Rule 4) covers:

- `break` lowers to a store into the block's result slot plus a **jump to the
  block's exit label**. Control leaves the body there, so a release the lowerer
  placed at a `decref_point` inside the body is emitted into the break's
  unreachable fall-through and never executes at all. Treating `Break` as a
  consumer of its operand anchors exactly there, and the value is then held to
  fiber teardown — one region per break.
- The block's own exit label is not late enough either: the block's value may
  flow straight into a consumer (`(f (block … (break v) …))`), and releasing at
  the exit frees it under that consumer.

So the transfer is stated as two facts, both over structures the solver already
holds:

- **Region flow** (`hir/region/infer/walk`, the `Block`/`Break` arms): a `Block`'s
  result region set is the union of its fall-through value's regions and every
  targeting `break`'s value regions. A binding that names the block's value
  therefore names those regions, and the ordinary binding-chain `decref_point`
  extension carries the release past the binding's own last use — which is what
  keeps `(let [r (block … (break v) …)] (use r))` from freeing `v` under `use`.
- **The break pin** (`regions/analyze/decref.rs`, the dual of `return_sites`):
  each broken region's `decref_point` is extended to `last_use[block]` — the
  node that consumes the block's value, or the `Block` itself when nothing does.
  The lowerer emits a node's decrefs *after* it, and for the `Block` that is
  after the exit label, so the one release fires on the break path and the
  fall-through path alike. Every `decref_point` rule is a max, so a later
  binding-chain or return extension still wins.

The lowerer needs no new instruction and no compensating release at the break
site. On a path that did not run the break, the value-route reloads a slot that
still holds `nil` and the release no-ops — the same nil-stamp discipline the
branch-union release relies on.

Pinned here: `tests/elle/region-break-transfer.lisp` (the reclamation), the
`break-value*` probes in `tests/elle/oracle.lisp` (the rates),
`regions::tests::blocks` (the placement, structurally), and
`region-break-transfer-uaf.lisp` (the soundness complement — a value broken out
and read afterwards, stored, or returned must survive).

### A break out of a TAIL block carries the return mint

The pin above places the release at the block's exit label. When the block is the
function's **tail**, that exit label is the last thing before the frame is handed
back, so the value the break carried is the *returned* value and it must leave
with one owning reference: the release at the exit consumes the callee's, and the
caller's own `DecrefValueRegion` consumes another. Only the return mint balances
that — the same mint any other returned value gets.

Both passes that decide "tail position" must therefore agree that a `break`
targeting a tail block is in it. `mark_tail_calls` and `wrap_tail_returns`
(`hir/return_incref.rs`) each thread a `tail_blocks` set: a `Block` in tail
position adds its own id, and a `Break` whose target is in that set walks its
value as a tail value — marking a call there `is_tail` (whose callee-side retain
propagates) or, for anything else, wrapping it in `Return` (which mints).

The two flags answer different questions, and the invariant is that only a
**function boundary** invalidates the second: `in_tail` is severed by any node
whose child is not its result, but `tail_blocks` survives every node except a
`Lambda`, because a `break` reaches its target's exit label by a *jump* and no
enclosing construct can intercept it. The shape that makes this load-bearing is
the pervasive `(fn … (forever … (break v)))`: the loop between the tail block and
the break is not itself a tail position — the loop's fall-through value is the
loop's, not the function's — yet `v` is the function's result. Sever the set
there and `v` is returned with no mint while the exit-label release still fires:
the caller reads a freed value. Pinned structurally (`return_incref::tests` — the
mint count per break, with the interior-block control) and behaviourally
(`region-break-transfer-uaf.lisp`'s tail-loop witnesses, whose faulting shape is
`lib/tls.lisp`'s `tls/read`).

### A release the break jumps over is not a release

The transfer covers the value the break *carries*. Every **other** region whose
release sits in the same window — inside the block's body, at or after the break
site, before the exit label — is jumped over by the identical edge, and for a
region the break does not carry there is no consumer to hand it to: the release
is emitted into unreachable code and the region is held to fiber teardown.
`(block (let [x (mk)] (when c (break 1)) (use x)))` strands `x` on every
execution that breaks.

The close is the same pin, not a release at the break site. A per-path release
at the break would need a site-list of what to free there *and* a count argument
for each entry; the placement argument alone suffices, because a release moved
**later** can only over-keep. So a region whose `decref_point` falls in the
skipped window is re-anchored to `last_use[block]` — the first point both the
break path and the fall-through path reach, and the same anchor the broken value
takes. Carried and skipped regions then leave the block through one release
each, and the lowerer still needs no new instruction and no new site-list.

"Skipped" is read off the structural order (`compute_order`, the same index
every `decref_point` comparison uses): a node's releases are passed over by a
break exactly when its post-order index is **at or above** the break's — which
covers the break node itself (its own decrefs land in the dead block after the
jump) and every enclosing `let`/`begin` whose releases the lowerer emits after
the body.

Three boundaries bound the window. Two are about *how many times* a release runs
rather than where:

- **An iterative scope nested in the block** (`While`/`Loop`). A value allocated
  in a loop body is re-allocated per iteration, so its release must stay
  per-iteration: hoisting it to the block's exit would leave one release for N
  allocations — a worse leak than the one being closed, and the same
  re-allocation argument the `capture_loop_ext` "bound outside" guard makes. A
  break out of a loop therefore still strands the *breaking iteration's* regions,
  an over-keep bounded by one iteration.
- **A `Lambda` nested in the block.** Its body's releases run in a different
  activation, against a different frame's slots; the enclosing block's exit label
  is not a point that activation ever reaches.

The third guards the anchor itself — the hoist's premise is that the exit label
is a point every path **reaches**:

- **A frame-replacing exit in the body** (a `Return`, or a `Call` in tail
  position, lowered as `TailCall`). That path leaves through the callee instead
  of arriving at the exit label, so a release moved to the anchor would be dead
  on exactly the path that used to run it — one leak traded for another. Such a
  block declines the window whole. This is the `(fn … (forever … (break v)))`
  tail-block idiom, where the broken value's own pin still applies (it is the
  *returned* value, and its release is the one the return mint funds) but the
  window's does not.

All three leave the conservative baseline (the release stays where it is,
skipped on the break path), never a mis-free.

Pinned by `tests/elle/region-break-skip.lisp` (the reclamation, with all three
boundaries driven as rows that must stay bounded on their own releases),
`regions::tests::blocks` (the placement and the boundaries, structurally), and
`tests/elle/region-break-skip-uaf.lisp` (the soundness complement — a value in
the window that is read, stored, or returned after the block must survive the
moved release).

## A release past a frame-replacing tail call is not a release

A tail call whose callee turns out to be a *closure* replaces the frame. Every
instruction the lowerer emits after the `TailCall` therefore belongs to the
**native fall-through**: a native pushes no bytecode frame, so on normal
completion the dispatch loop continues into that block (`tail_call_inner`,
src/vm/call/inner/tail.rs) and runs it. A closure callee never arrives there.

For a region the call's own **arguments** name, that is precisely the intent, and
it is the ownership transfer the calling convention rests on (rules.md Rule 5,
move-on-tail-call): the caller does not incref a moved argument, and the release
it never runs *is* the reference the callee's owned-param release consumes. The
callee's own region has the same story through a different channel — the new
activation takes over its release (`defer_callee_release`, `deferred_release_slot`),
which holds only where that channel reaches the release (§ "What the exemption
keeps, a channel must still run").

Every **other** release in that block has no such story. A parameter whose only
use is inside a closure the body builds, a parameter used nowhere at all, a scope
region the body allocated — each has its release emitted where control provably
never arrives, and the frame's own reference is stranded. The cost is one region
per call plus everything its free cascade would have reclaimed, so the dominant
witness is the stdlib helper whose body ends in a call to a local walker:
`(fn [dst src] (let [n (length src)] (letrec [go (fn [i] …)] (go 0))))` strands
both `dst` and `src` on every call, once per heap parameter the walker captures.

The close is the one case where a release moves *earlier*: the same single release
the solver placed is emitted immediately **before** the `TailCall` instead of
after it. The scope-region half of this is already unconditional — `lower_call`
emits the pending `DecrefRegion`s before every `TailCall` for the same reason.

**Relocating an instruction is not by itself free of obligation.** It is tempting
to argue that nothing is added and nothing duplicated, so no count argument is
owed. That is false, and it is the same category error the per-arm route makes
(§ "A release inside one arm…"): on the closure path the release did not run
before and now does, so at runtime this *is* a new release, and it owes exactly
what any new release owes — a reason to believe the frame holds the region's one
reference.

Two readings are therefore both required.

**What the call can reach** — the exemption, read off the call itself: every
region the callee, an operand's own value, the call's own result, or its deferred
channels (`deferred_release_slot`) name keeps its place in the dead fall-through,
where the ownership move and the deferred callee release own it. Read over
`alloc_region` and `binding_source_regions`, and again over the emitted
instructions, because ANF is free to rewrite an operand into a synthetic binding
the syntax walk does not connect back to the call.

**What an operand names is its VALUE, not its syntax.** The reading descends the
value-transparent wrappers — a `Let`/`Letrec` body, a `Begin`/`Block` tail, a
branch arm, an `And`/`Or`, a `DerefCell`, a `Return` — and stops where the value is
produced, recording that node's own region because that region *is* the value
handed over. It does **not** descend a `Call`'s callee or arguments, nor a
`Lambda`'s captures: a region reached only in there is one the operand's own
evaluation used and finished with before the tail call was made, and exempting it
leaves a release the frame still owes emitted where control never arrives.
`(f (g x))` hands the callee `g`'s **result**; `g`'s own closure region is not
reachable from the call at all. What the produced value does still hold, it holds by
a **counted** (or owning) edge in each case: a call's result carries exactly one
minted reference (§ "The return mint is emitted exactly once"), and a closure's env
took the funnel's count when it was built (§ "Lexical capture is not a second holder
to fear") — so the frame's own release remains the only reference it drops. An
inline `%`-opcode is not such a node: it mints no region and its heap result
(`%first`/`%rest`/`%get`) is an uncounted borrow living *in* its operand's region,
so the operand is the value-producing leaf and the descent continues through it.
This is the same reading the closure-cycle merge's by-move boundary makes of the
same question (letrec.md § "What the non-member tail still refuses"), for the same
reason.

Producing a value is not the same as producing a *fresh* one — a callee may hand
back an argument itself or a value it read out of one (adopt.md § "The lifetime
obligation the root carries") — and that costs the reading nothing, because the
mint is per *value*, not per freshness: whichever region the result turns out to
live in, the callee raised **that** region's count by exactly one on the way out (§
"The return mint is emitted exactly once"). So the frame's own release still drops
only the frame's reference, and the moved value survives it. The one node with no
such count is the inline `%`-opcode above, which is why the descent passes through
it to the operand that owns the page.

**Whether the frame holds the region alone** — the admission, and escape is its
sole authority. The exemption above is a statement about *arguments*, and
arguments are not the only path into a callee: a tail callee reaches its
**captured environment** too, which no argument names and no callee region
describes. `push-all`'s walker is exactly that shape —
`(letrec [go (fn [i] … dst)] (go 0))` names `dst` only through `go`'s env. That
path needs no enumeration and no refusal of its own, because the env's hold is a
counted (or owning) edge the funnel took when the closure was built (§ "Lexical
capture is not a second holder to fear"): a release of the frame's reference
leaves the callee's standing. The predicate is one and the same for both
mechanisms (`RegionInfo::frame_held_regions`): every holder binding escaping by
the **return** facet at most, the region's own release route unmutated — or naming
a cell box rather than a slot (§ "A mutated holder poisons its value route, not its
cell box") — and the region absent from the fiber frontier's atomless site half.

So this close covers a parameter or local the frame alone owns — captured by a
locally-called closure or not — whose release lands at the body's scope exit, and
with it the **env cell** of a captured local, whose `DecrefCellRegion` lands in
the same dead block. Why the return facet rides along rather than refusing is the
next section.

### The callee's return mint, and why the point owes it nothing

The shape that makes the return facet look like a refusal is the same walker one
parameter over: `push-all` returns `dst` through `go`. A relocated release is safe
when the reference it drops is not the region's last *live* one. For a value the
callee merely **reads** — the walker's `src` — the frame's release is the last one
and nothing reads the region after the frame is gone. For a value the callee
**returns**, the caller does read it afterwards, through a reference the callee's
own `Return` mints — and that mint fires *after* the relocated release. Between
the two the count must not reach zero.

Nothing can put it there, and the reason is the enumeration the exemption already
rests on. A callee reaches a value this frame owns by exactly two routes: as an
**operand**, where the release stays in the dead block and is the ownership move;
or through its **captured environment**, where the funnel took a counted (or
owning) edge when the closure was built. Both ends of that enumeration are safe:

- a callee neither route reaches cannot name the region at all, so its `Return`
  mints nothing against it and the frame's release is the region's last;
- a callee the second route reaches holds a count that the closure region's
  free-time cascade drops only at the callee's *completion*, after its `Return`. So
  the order over one call is: env edge taken, frame release, callee mint, env edge
  falls away — and the reference left standing is the caller's.

The admission is therefore a fact about the **region**, not about the point: a
region whose only escape facet is the return one is relocated wherever its release
lands, and what the point still decides is the exemption alone. That the callee's
captures are usually unknowable is what makes reading both routes the right
reading rather than a weaker one — this compilation resolves a `Var` callee to a
lambda in this unit and no further, so an imported or parameter callee's captures
are invisible, and a capture is counted however little of it the compiler can see.

Every other facet still refuses, and each for the reason it always did: a holder
that crosses the **fiber** frontier may be borrowed uncounted by a parked frame; a
region whose own **route** binding is mutated has a release that frees whatever the
slot holds then, except where the release names the cell box the mutation leaves
alone (§ "A mutated holder poisons its value route, not its cell box"); a
holder captured by a closure that **escapes** leaves with it. What is dropped is
the return facet's refusal and nothing else — which is why escape must be able to
say "*this* facet and no other" (`EscapeInfo::binding_escapes_beyond_return`, the
complement of `binding_escapes_via_return`).

The everyday shape this reaches is the index-walk fold driver — `fold`, `reduce`
and `concat` all walk with it:

```
(fn [f n i acc] (if (%lt i n) (recur f n (%add i 1) (f acc i)) acc))
```

The base arm returns `acc`, so the region is on the return frontier. The recursive
arm hands the callee the *combiner's* result rather than `acc` itself, so neither
route reaches the accumulator at that point and the frame's release is its last —
which is what frees each displaced accumulator per step rather than per call.

### A compiled capture cell is frame-held exactly as its binding is

The admission reads the frame's holders through `binding_source_regions`, so a
region **no binding names** offers nothing to judge and is refused. A compiled
**capture cell** (`begin_cell_regions`) is exactly such a region: it is minted at
the scope that prebinds it — the `Letrec` of a binding some *sibling* closure
captures (letrec.md § the static-slot cell requirement) — and the binding names the
closure region the cell points *at*, never the cell's own. So the cell's
`DecrefRegion`, which the solver places at that binding scope, is stranded whenever
the scope's body ends in a frame-replacing tail call, and it takes the closure down
with it: the cell's reference is what keeps that closure's region off zero, so the
closure leaks *behind* the cell even where its own release relocated cleanly. The
everyday shape is a pair of local helpers where one calls the other and the body
tail-calls the caller —
`(letrec [helper (fn [x] …) go (fn [m] (helper m))] (go n))`. Where that caller is
also **self-recursive** the projection is not what reclaims the cell: the ownership
forest's capture adopt claims it into the capturer's closure region and suppresses
its own decref (`capture_adopt_edges`), so the capturer's stranded-self deferral
takes the pair down together. Both are pinned, so neither channel can quietly
become the other's.

The fact that settles it is that the cell's holders are its binding's holders, one
indirection out. The frame holds the cell through its own static slot; every other
holder is a closure that captures the binding, and that hold is the counted (or
owning) edge the funnel takes at the cell store (§ "Lexical capture is not a second
holder to fear"). No route reaches the cell that does not reach the binding — a
`DerefCell` read goes *through* the cell to get at the closure — so whatever escape
says about the binding's regions it says about the cell's, by every facet and by
the mutated-holder reading alike. Projecting each binding's single compiled cell
region (`RegionInfo::single_cell_region_of`) alongside its `binding_source_regions`
therefore asserts no admission the predicate was not already making; it names a
region the predicate could not see. Without the projection the cell is refused for
want of a holder and strands the closure it holds — one region short of the
cascade, so the helper pair leaks whole.

A binding with more than one compiled cell — a file-body/nested-`begin`
double-declare — is refused: `single_cell_region_of` yields `None`, so the
admission agrees with the `AdoptCellRegion` emit to refuse rather than guess which
physical cell a given closure holds.

This is the cell of a **prebound forward reference**, not the env cell of a
reassigned capture: that one is a `cell_release_regions` member whose release names
the box through `LoadCaptureRaw` + `DecrefCellRegion`, and it is already frame-held
because the binding names its own region (§ "A mutated holder poisons its value
route, not its cell box").

### A move that crosses a read through the cell it frees is declined

A captured binding's value and its env cell are addressed by one env index, and
they are two **regions**. The relocation decides per region, so the pair can get
different answers, and moving one of them alone inverts the order between them.
The value release loads the box RAW and lets `result_region_of` unwrap it, so it
READS the page the box's `DecrefCellRegion` frees ([bindings.md](bindings.md) § "A
cell's release lands at or after every release routed through that cell"). Move
the cell's release ahead of the `TailCall` while the value's stays behind, and the
unwrap reads a reclaimed page.

The everyday split is the admission's own holder rule. It judges a region through
the bindings that name it, so a value region **no binding names** is refused for
want of a holder, while the cell region is admitted on its binding's verdict (§ "A
compiled capture cell is frame-held exactly as its binding is"). An
`Immediate`-effect native's result is such a region: the walk records no result
region for the call, and the lowerer still routes the binding's release through the
env index.

Neither reading the relocation already makes can see the inversion: the exemption
asks what the CALL names and the admission asks whether the frame holds the region
alone, and this obligation holds between two regions rather than between a region
and the call. So the relocation asks one more question, of the window the move
crosses rather than of the region: a run spliced from after the `TailCall` to
before it crosses every instruction now between the two positions, and those
instructions say whether the move inverts anything. A `DecrefCellRegion` naming an
env index that some instruction in the window still reads — a `LoadCapture` or
`LoadCaptureRaw` at that index — declines the move and stays where the lowerer put
it.

Reading the window is enough because the clamp already fixed the emission order:
it puts the cell's `decref_point` at or after the value release's, and where both
land on one point the release order sorts the deepest read first
([rules.md](rules.md) Rule 4). So a value release that reads through the cell is
already in the window when the cell release asks to move.

Declining strands the box on the closure path — the bounded, always-legal fallback
the relocation takes for every region it refuses, and one box per activation rather
than a page freed under its own reader. The everyday shape is the closure-as-module
whose last form is a struct literal over the closures its captured defs built:
`(fn [] (def a (ptr/from-int 0)) (defn p [] a) {:p p})`. Pinned by
`a_cell_release_declines_a_move_across_a_read_through_it`, beside the admitted face
`reassigned_env_cell_release_precedes_the_frame_replacing_tail_call`, whose cell
holds an immediate and so has no release routed through it at all.

The order the clamp and this decline together hold is stated once more over the
finished emission, as a debug-only walk of every block
(`lir::lower::assert_cells_outlive_their_readers`). Each mechanism can only see its
own half, so a block that frees a cell before a read through it names a gap in
either.

### What the exemption keeps, a channel must still run

The exemption states its reason positively: the callee's own region keeps its place
in the dead block because the new activation takes the release over
(`defer_callee_release`). That is a claim about a *channel*, and it holds only where
the channel reaches the release in question. The deferral recognises a callee whose
region **demises at the call node** — the per-call local closure a body builds and
immediately calls, whose one use is the call. A letrec **member** the body tail-calls
does not fit that description: a sibling captures it, so its uses span the whole
letrec and the solver places its demise at the letrec's own scope end. The release
lands after the body — the same dead block the exemption is keeping it in — and no
channel runs it. The member's closure region strands once per call, and its
environment and captures strand behind it. The everyday shape is the mirror of the
forward-cell pair above:
`(letrec [helper (fn [x] …) go (fn [m] (helper m))] (helper (go n)))`, where the body
tail-calls the **captured sibling** rather than the capturer.

So the deferral reads the release's **placement**, not the call node alone: a tail
callee whose release the enclosing letrec emits at its scope end rides the same
channel, run once at the callee's normal completion.

The count argument is the ordering one, and it has nothing to bridge. The deferral is
a decref, not a free, and it runs *after* the callee's `Return` mint — the same
argument the cell-free self-recursive deferral makes for its own return admission
(selfrec.md § "The deferral needs no escape gate"), where the
frame-exit relocation has to move a release *ahead* of the call and fund the gap. The
return facet is therefore funded, and only the **fiber** facet refuses, a parked frame
being free to hold an uncounted borrow the compiler never placed.

What the placement reading must still exclude is a release the frame does not own. A
**suppressed** decref belongs to the store or capture-adopt path that claimed the
region — deferring it decrements a count the frame never raised — which is the same
exclusion the demise reading makes through `suppressed_decref_regions`. A **closure-
cycle member** is released by the merge's own channel, which already covers every
stranding tail path of an admitted cycle (letrec.md § "The frontier gate"). And the
marking is honoured only through a **non-upvalue** reference, for the reason the arena
channel is: a nested closure that captures the member completes its own activation
before the enclosing letrec's later uses, so deferring there frees the region early.

The sibling's forward **cell** is not the callee's own region, so it relocates like
any other holder and its cascade drops the `cell ⊇ closure` edge ahead of the call.
What the deferral drops afterwards is the frame's own slot reference, the last one
standing.

### The relocation point outlives the block, and a branch merge inherits it

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
the accounting to hold. The obligations are unchanged and are read **per point**:
a region an arm's own call names keeps its place there (that arm's copy is the
ownership move), and escape's admissions gate the whole thing, because each
replica still fires on a closure path where none fired before. Reading per point
is also what lets one arm's callee fund a returned region (§ "The callee's return
mint") while a sibling arm's callee, capturing nothing, declines it.

Self-cancelling is a real restriction, not a formality. A release by region id
(`DecrefRegion`), a capture cell's `DecrefCellRegion`, and the transfer adopt
leave no stamp behind and would count twice on a native fall-through, so a run
that is not exactly load / release-by-value / nil-stamp keeps the baseline — and
for the id release that is a reason to change the ROUTE rather than to give up the
replica (§ below). Scope regions need nothing here anyway — `lower_call` already
frees them before every `TailCall`.

#### Self-cancelling is a property of the ROUTE, not of the region's class

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
one question rather than two (§ "An arm that leaves through a callee takes a
replica, not the anchor").

The reroute is asked only where some inherited point ADMITS the region, which
keeps it disjoint from the channel that answers the same strand a different way: a
release every point exempts — the merged arena riding the deferred slot
(§ "What the exemption keeps, a channel must still run") — never changes route.
The route's own refusals travel with it, each naming a reason this slot is not
what the release reads: a **mutated** binder repoints its slot, an env cell's
release names the BOX rather than the slot, and a transfer consumer's release is
an adopt rather than a decref. Each of those keeps the id route, and with it the
whole-branch decline.

What the reading reaches is the `letrec` binding scope's own drop. A cell-free
self-recursive helper's closure region is no call result, so the id route is its
default, and its demise is the `Letrec` node — the binder is the scope
([selfrec.md](../selfrec.md) § "The closure region is per-call and stranded"). A
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

The residual is unchanged in kind: a holder escape marks by a facet no edge at
the point replaces.

Pinned by `tests/elle/region-tail-frame-exit.lisp` (the reclamation, with the
argument-move and callee exemptions, the per-arm faces, the captured-holder faces,
the non-self-cancelling boundary, the env-cell faces, the
handed-back-through-the-callee faces, the forward-cell faces, and the
id-routed letrec closure whose body's tail is a branch, driven as rows),
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

### What the fall-through owes, a signal exit owes too

The relocation above decides which releases *leave* the post-`TailCall` block. What it
cannot decide is whether the block ever runs. The block belongs to the native
fall-through, and a native reaches it on exactly one outcome: **normal completion**
(`bits.is_empty()`, the `SignalAction::Ok` classification). Every other outcome — an
error, a suspend, a fiber carrier (`fiber/resume`/`fiber/abort`/`fiber/propagate`), a
capability denial — leaves through the signal machinery, which does not run the block
before returning to the dispatch loop.

One release in that block is the frame's own **extra** reference, and it is the one the
signal exit runs. A tail call hands its callee one fresh owning reference per **borrowed**
argument, because a captured upvalue is owned by the closure env rather than by this
activation: pure-moving it would hand the callee a reference the caller never had (§ "A
release past a frame-replacing tail call is not a release"). That retain has exactly one
consumer per path — a frame-replacing closure callee's owned-param release, or, the frame
not being replaced by a native, the fall-through block's own `DecrefValueRegion`. A signal
exit reaches neither, so it consumes the retain itself. The count argument is the
borrowedness: the value has a holder that is not this frame, by the definition of the
class, so dropping the extra reference frees nothing.

**Except the retain that names the parked payload.** A SUSPEND does not abandon the
continuation: the driver the tail suspend unwinds to parks it at the post-`TailCall` ip and
the resume replays the block (owner.md § "A suspending native tail call parks its
continuation"). So on that exit the fall-through's `DecrefValueRegion` is still a consumer,
and the retain it consumes is exactly what the park owes — a fiber body owns one reference
of every value it yields, and a borrowed payload has no other (owner.md § "A fiber body owns
one reference of every value it yields"). The suspending exit therefore leaves standing each
retain whose region is the payload's, and consumes the rest as before: a park delivers ONE
payload and the discharge of an abandoned fiber releases ONE reference of it, so a retain on
any other region has no such stand-in and would be stranded once per abandoned park. The
test is the payload's region against the stash's, the same reading the abandoned-frame walk
makes of its own tables (§ "An abandoned frame runs the releases it still owes"). A
capability denial parks too, but its payload is a struct the denial built, which names no
argument — so its retains keep the ordinary consume.

That exemption is what carries a **dynamic** `emit` in tail position, whose non-literal
first argument makes it an ordinary native call rather than the `Emit` terminator: the
borrowed-argument retain is the body reference the park owes, and there is no second mint.

**A terminal `:error` mints instead of exempting.** The same operation raising a terminal
signal hands its payload to a **catcher**, whose read of the signal consumes one reference —
the payload's **delivery** (§ "An abandoned frame runs the releases it still owes"). The
suspend exemption cannot supply that one. An `:error` fiber is resumable, so a restart replays
this block too, and the stash release then reaches a retain the catcher has already consumed.
So the exit consumes its retains like any other — the nil stamp makes the replay a no-op — and
mints the delivery itself, exactly as `handle_emit` mints it on the literal path, recording it
in the same `Fiber::emit_delivery`.

The mint is taken only where the payload is one of the call's **own arguments**. A payload the
native BUILT funds the delivery with its birth reference, so an ordinary native raise — a fresh
error struct — must not receive one. The identity test is what tells the two apart, and the
`emit` primitive handing back its second argument is the shape that needs it.

**The record travels with the mint**, as it does on the literal path. Once the mint funds the
delivery, every reference the frame holds is owed to the frame's own routes, so the walk and
the discharge must stop exempting the payload's region. Two references are reclaimed that way:
a payload the body ALLOCATED, whose owned-argument release sits in the abandoned block, and the
second name of a payload that reaches the call twice — the first occurrence moves the frame's
reference and the second takes a retain of its own (rules.md Rule 5), as `(emit s s)` does.
Where the fiber is restarted rather than discharged, the replayed block runs that same release,
so the accounting is the same either way.

A **halt** takes neither mint nor record, for the reason `handle_emit` withholds its own retain
there: the fiber is promoted to `:dead` and never resumed, so that delivery has no consumer and
a reference taken for it is stranded (owner.md § "Park/unpark symmetry").

**Every OTHER release in that block stays.** They divide in two and neither half has that
argument. An **argument's** own release is the ownership move, and a signal exit is exactly
where the payload may BE that argument — a fiber carrier returns its fiber argument, a
yielding io op hands the scheduler a request embedding its port — so the signal machinery
accounts for it on the path it takes. Everything else the block still holds is a release the
relocation declined to move, which it declines only when escape refuses `frame_held_regions`
— the same count argument, refused. Running either would be a new release on a path that
owed none, with nothing to fund it.

**A frame that leaves by a signal can still be replayed.** A suspending signal parks
the continuation at the post-`TailCall` ip and the resume re-enters it (owner.md
§ "Park/unpark symmetry"); an `:error` fiber is resumable too, so a restart replays
the same block. Running the release at the signal exit and again at the replay would
be a double release, so the exit **stamps the stash local `nil`** as it takes the value:
the replayed `DecrefValueRegion` then loads an immediate and no-ops — the same
self-cancelling discipline a replicated release relies on (§ "The relocation point
outlives the block"). The stash is the block's alone, written once before the call and read
once after it, for this release and nothing else.

Resolving the region and releasing it are **two steps**, with the signal handler between
them: the handler installs the payload (which may be this very value) and may swap the live
fiber out from under the frame whose locals name it, so the names are taken first and the
references dropped after.

What this closes is one member of a **dead continuation's** pending value releases. A fiber
abandoned while parked at such an exit — an aborted fiber the restarts system keeps
resumable, a capability-denied fiber nobody restarts — stranded the retain per call, and its
first stranded reference is often the fiber value itself, which then pins the body closure,
its captures and its parked payload behind it. What remains stranded is the denied call's own
argument scratch (owner.md § "The bounded residual").

The **JIT** tier keeps today's behaviour: `elle_jit_tail_call` carries neither this
channel nor the callee-adoption channels (`defer_callee_release`,
`deferred_release_slot`), so a compiled frame that leaves by a signal strands the
retain as before — a bounded over-keep, never an over-free.

Pinned by `tests/elle/region-tail-signal-exit.lisp` (the reclamation, with the
fiber-carrier exit, a heap payload beside it, and a restarted `:error` fiber driven as rows),
the `abort-discard` probe in `tests/elle/oracle.lisp` (the per-op rate),
`lir::lower::tests::release::frameexit::{a_borrowed_tail_argument_is_named_on_the_call,
an_owned_tail_argument_is_not_named_on_the_call}` (the naming pins, both faces), and
`tests/elle/region-tail-signal-exit-uaf.lisp` (the soundness complement — a value the signal
payload carries, a restarted `:error` fiber that replays the block, a suspending handoff, and
a caught error whose handler reads the released value's holder must all survive the exit's
release). The suspend half of the payload exemption is pinned by
`tests/elle/region-dynamic-emit-borrow-uaf.lisp` (a tail dynamic `emit` of a borrowed value,
driven past an abandoned park) and gauged by the `emit-dyn-tail` probe in
`tests/elle/oracle.lisp`; the terminal half by
`tests/elle/region-dynamic-emit-terminal-uaf.lisp` (a tail dynamic `(emit sig v)` raise of a
borrowed value, read back through every holder that outlives the fiber) and gauged by the
`emit-dyn-*-error*` probes there, whose `emit-dyn-error-fresh` and `emit-dyn-error-repeat`
faces are the ones that read the RECORD — a payload the body allocated, and one region named
through both arguments, each holding a frame reference the walk must stop exempting once the
mint funds the delivery.

### A carrier that comes back with a result never left the frame

The section above divides a native tail call's outcomes into normal completion, which
runs the post-`TailCall` block, and a signal exit, which abandons it. A **fiber
carrier** — `fiber/resume`, `fiber/abort`, `fiber/propagate`, `fiber/refuse` —
belongs to neither
half until the VM has driven the child. It leaves the primitive as a signal because it
is a *request*: the VM is asked to run another fiber and report what happened. Where
this fiber's own mask **absorbs** the child's outcome, the request is answered here,
the value is the call's result, and nothing has left. So the carrier takes the
fall-through — push the result, continue into the post-`TailCall` block — exactly as a
native that returned `SIG_OK` does.

Reading the absorbed outcome as an exit instead is what strands the block. The block
holds every release the frame still owes for this call: one `DecrefValueRegion` per
**owned argument** (the ownership move a native callee never runs in the caller's
place), the return mint, and the result's own release. Handing the value out through
`fiber.signal` and returning `SIG_OK` reaches none of them, and no other path runs them
either — an absorbed outcome is not an error, so the abandoned-frame walk does not
fire, and it is not a suspend, so no replay arrives.

The count argument the exemptions in the section above make is *why the fall-through
is the answer rather than a walk*. That section keeps an argument's release in the
dead block because a signal exit is where the payload may BE that argument, and keeps
the result's release because it names a local the fall-through would have stored.
Absorption removes both premises at once: the frame runs on, so the block stores its
own result before releasing anything, and it runs the compiler's exact per-argument
ownership rather than a runtime guess about which argument the signal took.

The **other two positions already read it this way**, so this is one rule stated in
three places rather than a new one. In Call position `handle_fiber_abort_signal` pushes
the absorbed result and returns `None`, and the compiler's post-call code runs. On the
JIT tier `handle_fiber_abort_signal_jit` hands it back in the return register and the
compiled caller's post-`TailCall` block runs. The interpreter's tail position was the
one that treated the answer as an exit.

What funds the result's release is unchanged by the position. `dispatch_native_call`
withholds the pass-through retain from a value a native returns as a signal payload
(effects.md § "The dispatch pass-through retain"), and for an absorbed carrier the seam that
produced the value has already counted one: the injection's `AbortDelivery` where an
abort's mask catches (effects.md § `Delivers`), and for a resume the reference the
crossing itself counted — the park's `EmitEscape` retain for a yielded value, the
child's `Return` mint for a terminal one. That is the same reference the Call position
consumes, so the two positions cannot drift apart.

Where the payload is *also* an owned argument — a literal materialized straight into
`(fiber/abort f "boom")` whose caller reads the result back — the frame owes **two**
releases on one region, and holds two references to fund them: the one its allocation
minted and the one the delivery did. Running one is what a skipped block looks like
from the outside: a rate of one region per abort, flat in the payload's size.

Pinned by the `abort-tail-result`, `abort-mask-caught-literal` and
`refuse-tail-result` probes in `tests/elle/oracle.lisp` (the per-op rates, each beside
the control that removes the tail position), and by
`tests/elle/region-fiber-abort-delivery-uaf.lisp` (the soundness complement — the block
frees the fiber and the payload at the call the carrier returned through, so every
reader that outlives it must still find them).

## An abandoned frame runs the releases it still owes

The section above places one release in the block a signal exit skips. That block is
not the only thing skipped. An **error** leaves through the signal machinery, so
*none* of the frame's remaining instructions run — and every release the frame still
owed is among them. The frame that called the raising native holds the arguments it
materialized for that call, and every binding whose last use lies past it; each of
those is one region nobody releases. The rate is per unwound frame and per pending
value, so a `try`/`protect` in a loop grows without bound — the shape a retry loop
and a server request loop both are.

The frame is gone, so the release cannot be reached by resuming it — the runtime runs
it at the exit instead. What that needs is the set the frame still owes, and the
emitter already names it. A **value-routed** release is three instructions,
`LoadLocal s; DecrefValueRegion; StoreLocal s nil` (§ "Two resolutions"), and the
slot `s` is its whole identity: the instruction releases whatever `s` holds at the
moment it runs, and the nil stamp is what records that it ran.
`Code::frame_release_slots` carries those slots per function; an error exit walks
them, and a slot still holding a heap value is a release that did not run.

**The slot is the release, not the value.** Three facts make the walk *that* release
rather than a new one:

- **A slot belongs to one binding for the whole body.** `allocate_slot` counts
  `num_locals` up and never reuses, and stamps each new slot nil where the binding is
  introduced, so nothing but that binding's own value is ever what the walk finds.
- **The nil stamp is the receipt.** Every value route clears its slot as it releases,
  so a non-nil slot is an unrun release and a run one is invisible to the walk. An arm
  whose release the taken path would not have reached reads nil for the same reason a
  replicated release does (§ "The relocation point outlives the block").
- **A release the emitter declined is not in the table.** A slot is recorded where the
  plain value route is *emitted*, so a mutated route, a reassigned binding's slot, a
  cell release naming the box, and a transfer adopt each record nothing. The walk can
  only run a release the frame genuinely had.

**Two routes, two receipts.** The slot-resolved release (`DecrefRegion`) is named the
same way, by the static region slot it carries, and its receipt is the activation map
itself: the alloc mints the mapping and the release TAKES it
(`take_runtime_region_for_drop_slot`), so a slot still mapped is a release that did not
run. `Code::frame_release_regions` carries those. Naming only the slots the *executing*
function releases for is what keeps a caller's leftovers out: the map survives a
frame-replacing tail call, and the references still in it are the callee's own machinery
to answer for.

**What the signal carries is not abandoned — unless the raise minted its delivery.**
The error's payload leaves with the signal, and the catcher's read of it is funded by
exactly one **delivery** reference. Where that reference comes from decides what the
walk may release, and the two raise paths differ:

- A **native** raise installs the payload with no retain. A fresh error struct funds
  the delivery with its own birth reference; a payload the native read out of an
  argument funds it with the frame's reference to that argument — the very release the
  walk would otherwise run. So the walk skips a slot whose value lives in the
  payload's region: the skipped release *is* the delivery.
- An **`Emit`** raise (`(error v)`) mints the delivery itself — the `EmitEscape`
  retain `handle_emit` takes, consumed by the resumer's release of the resume result.
  The frame's own reference funds nothing, so the skip has nothing to stand in for and
  would strand one region per raised-and-caught error whose payload the raise chain
  owns. The raise records the mint (`Fiber::emit_delivery`), and a walk whose live
  signal payload matches it skips nothing.
- A **dynamic `emit` in tail position** reads like the `Emit` case with the mint moved to
  the exit: the raise is an ordinary native call, so the signal exit mints the delivery of
  a payload the call received as an argument and records it there (§ "What the fall-through
  owes, a signal exit owes too"). What the walk then reclaims is the frame's own reference,
  wherever the payload is one the body allocated.
- An **injected** `fiber/abort` / `fiber/refuse` payload reads like the `Emit` case,
  for the same reason: the injection mints the delivery
  ([effects.md](effects.md) § `Delivers`) and records it on every fiber whose frames
  the payload then travels through — the aborted one and, where the error escapes it,
  the aborting one. A frame holding the payload owes its release like any other.

The same reading governs the parked frame's discharge: `Fiber::take_parked_state`
withholds its payload protection exactly where the mint is recorded, so the free-path
discharge runs the parked body frame's owed release for an emitted payload and leaves a
native payload's standing. Nothing else survives the frame either way: a value the
frame stored elsewhere is held by a counted edge the store funnel recorded, which this
release cannot take below that holder's count.

**A frame the restarts system can replay is not abandoned.** A fiber body's first run
parks its own frame on an error exit (`do_fiber_first_resume`), so a restart replays
those instructions and the releases among them; running them here as well would release
twice. The parking caller says so with a one-shot (`VM::pending_error_park`), taken at
the activation's entry so the frames that body *calls* — which nothing parks — still
walk. What the parked frame owes runs where no resume can reach it either: the fiber's
own discharge reads the same two tables off each parked `BytecodeFrame`, its saved
locals and its saved activation map standing in for the live ones
([owner.md](owner.md) § "The bounded residual").

**The compiled tier runs the same walk.** A compiled frame leaves by an error at two
points, and each runs the walk before its activation's region-map pop: the check after
a call, which finds the callee's raise, and an `Emit` of `SIG_ERROR`, which parks no
frame to resume. The tables are compile-time constants, so the prologue materializes
them once — each in its own stack slot, at its own width — and every exit hands the
runtime both, with the frame's locals spilled in slot order. The value route resolves
`s` to the spilled `LoadLocal s`; the slot route reads the very activation map the
compiled prologue pushed. The two tiers share the walk itself; only where the slots
are read differs.

Nothing is spilled back. The compiled frame returns as the walk completes, so the nil
stamp that is the interpreter's receipt has no compiled counterpart and needs none: a
table names each slot once, and one error exit runs per unwind.

A compiled frame is never one a restart replays, so `VM::pending_error_park` has no
compiled reader. A fiber body's first run enters through
`execute_bytecode_saving_stack`, and compiled code is reached only from a call site
inside it, so the parked frame is always an interpreter frame.

Every exit — compiled or not — pops the map it pushed, and the walk depends on it:
`last()` must be the abandoned activation's own frame, not a callee's leftover.
`execute_bytecode_saving_stack` asserts the balance in debug builds, so an exit path
that returns without popping detonates at the first activation to return through it
rather than resolving some later release against the wrong frame.

The rule reaches the exits an error never takes. A compiled frame whose CALLEE
suspends parks itself at the post-call yield check and returns the yield sentinel,
and that exit pops too: the park reads the map first, so what the pop discards is a
frame nothing needs again. Left behind, it is what `last()` names for the interpreter
activation above — which then parks a map that was never its own — and the remap stack
never shrinks back, one frame per suspend through a compiled callee. That the exit is a
suspend rather than an unwind changes only what runs before the pop: nothing, because
the frame resumes and still owes its releases to the resumed body.
`jit::compiler::tests::every_compiled_exit_pops_the_region_map` pins it on the emitted
code, where a missing pop is visible without an activation having to return first.

Pinned by `tests/elle/region-error-unwind.lisp` (the leak gauge — the pending release
of a raising call's argument, of two of them, of a binding live across the raising
call, and of an enclosing frame, each bounded beside a control that raises holding
nothing), the `error-payload*` closed controls in `tests/elle/oracle.lisp` (the
emitted payload's own region, bounded per face — raised in the parked body frame,
in a walked non-tail callee, handed down as an owned parameter, and as a
two-region struct — beside the native-raise control whose gap isolates the
recorded mint from the walk and discharge) with
`tests/elle/region-error-payload-uaf.lisp` as their guardfree complement (the
payload a catcher stores outward, a borrowed module payload raised repeatedly, a
native raise's unrecorded install, and a restarted `:error` fiber's replay), the
`denied-discard` probe in `tests/elle/oracle.lisp` (the per-op rate of what the
tables cannot name), `tests/elle/region-jit-error-unwind.lisp` with
`tests/elle/region-jit-error-unwind-uaf.lisp` as its guardfree complement (the
compiled face — one subject per compiled error exit, and, on the soundness side,
the caller's binding live across a compiled callee's exit),
`vm::core::region::tests::a_compiled_frames_*` (the spilled locals stand in for the
frame stack, and the payload exemption reads the same) and
`jit::dispatch::tests::release_abandoned_frame_runs_both_routes_off_the_compiled_exits_buffers`
(the two tables reach the runtime as separate buffers of different widths),
`lir::lower::tests::release::emission::{frame_release_tables_name_exactly_the_routes_emitted,
a_reassigned_binding_records_no_value_route}` (the tables are the emit sites, so a route
the emitter declined has no entry), and `tests/elle/region-error-unwind-uaf.lisp` (the
soundness complement — the payload the raising native builds while the frame holds its
argument, a value the frame stored into a container that outlives it, a parked frame the
restarts system replays, and a catching frame's own values, all under
`--trace=guardfree`).

## Compile-time region selection (coalescing)

Where the compiler can prove a value is a **fresh local allocation whose region
is a known slot** — the value was allocated in this function (`alloc_region` has
an entry, or for a returned binding `binding_source_regions` resolves to one such
region), that region is `live`, and it is none of the dynamic classes below — it
substitutes the slot-resolved `IncrefRegion` for the value-resolved
`IncrefValueRegion` (and likewise on the decref side). This is **instruction
selection, not a change of RC unit**: the slot resolves — through the activation
map — to the *same physical region* `region_of(value)` would return, because the
allocation stamped that slot to that region and a value never moves regions. So
every region's RC trajectory is bit-identical and leak counts and teardown
residue are unchanged *by construction*. The win is one fewer runtime deref per
coalesced site, and the slot-resolved form touches no operand stack (the value
register stays on top as the return value — stack-neutral).

The substitution is **purely callee-mint-side** for the return convention: the
caller still cannot name the callee's region, so the caller's balancing
`DecrefValueRegion` stays value-resolved. The pervasive coalescible site is the
prediction-free return mint at every function tail. The two narrower sites are
both reassigned-binding traffic over a value the lowerer just allocated locally:
the **reassign incref-on-store** (`lower_assign`'s drop-on-overwrite — pinning a
1-slot container's new content; coalesces only for a *fn-local* container, since a
*module-scope* container's value is in `mutated_binding_value_regions` and stays
value-resolved), and the decref-side **captured-reassign init-drop**
(`store_captured_cell_init` — dropping the producer's reference to a captured
binding's fresh init value, `DecrefValueRegion` → `DecrefRegion`). Both reach the
same `coalescible_region` predicate, so the runtime-population guard (the region's
slot must be stamped by an allocation emitted in this function) refuses any
captured/cross-thread value at all three sites alike.

The reduction this buys — coalesced (slot-resolved) versus value-resolved mints at
the candidate sites, plus the self-edges eliminated below — is *measured, not
asserted*: the lowerer records each decision in the thread-local instrument
`lir::lower::rcstats` (the choice is not recoverable from the final LIR — a
coalesced mint's `IncrefRegion` is indistinguishable from a store-edge's, and an
eliminated self-edge leaves no instruction), and `benches/regionrc.rs` reports the
totals across the stdlib load and the `tests/elle` corpus.

## The dynamic boundary (stays value-resolved)

These sites are genuinely runtime facts and must **never** coalesce — the region
is not knowable at compile time:

| Site / class | Why it stays value-resolved |
|---|---|
| caller-side `DecrefValueRegion` of a call result | the caller cannot name the callee's region — prediction-free by design |
| tail borrowed-arg incref (`tail_arg_is_borrowed`) | a captured upvalue / env **cell** region — dynamic |
| tail native-result retain | pass-through native result; region named only at runtime |
| reassign drop-old (`DecrefValueRegion{old_reg}`) | the displaced 1-slot-container content — the runtime fact the container tracks |
| `Mixed`/`Unknown` `RegionEffect` results | region unknown; the clique is a may-store over-keep |
| pass-through natives (`first`/`rest`/`get`) | result lives in an arg's region, named only at runtime |
| capture cells (`cell_release_regions`, `DecrefCellRegion`) | release frees the *cell's* own region, not the inner value's |
| phantom param regions | no `alloc_here`, filtered from `live_regions` — runtime-counted |
| suspended frames | `activation_region_map` captured/restored across resume; the slot is per-activation |
| terminal fiber signals | set-once park-retain, no compile-time edge |
| runtime mutable-store traffic | the `push_with_incref` funnel counts at the store site (the TT gap is dynamic) |
| ownership-forest ops (`AdoptRegion`, `FreeRegionGroup`, `AdoptIntoActivation`) | a forest member is a runtime fact (a call-result / cross-activation region); `AdoptIntoActivation`'s parent — the activation's pages-less owner node — has no slot at all (owner.md § "Owner nodes") |

## Self-edge elimination

`emit_increfs_for` emits one `IncrefRegion(source)` per cross-region store edge
`(site, source, target)` — a value in `source` stored into a structure in
`target` — balanced by `target`'s free-time cascade at `DecrefRegion(target)`.
The cascade **skips self-references** (`regionpool/introspect.rs` decrefs a
referenced region only when `rid != own_id`). So a `source == target` self-edge
`R→R` has no balancing decref: keeping its `IncrefRegion(R)` **leaks** `R`.
Eliminating a self-edge is therefore the sound transform — the compiler-side
mirror of the cascade's own `own_id` self-skip. It is the *only* redundant case:

- **alias edges** — `(%pair x x)` and repeated-arg shapes emit N edges into a
  *distinct* target; the cascade finds N references and decrefs N times, so all N
  increfs are required. Collapsing them is an over-collapse UAF.
- **may-store clique edges** — over-approximations whose balancing decref is the
  target's runtime content scan (per *actual* store, not per emitted edge).
  Eliminating them trades a known leak for a possible UAF.

A self-edge appears only when a region **merge** collapses a store edge's source
and target into one region (a value merged into the aggregate it is stored into);
see [merging.md](merging.md) § Merging. The compiler detects one with
`RegionInfo::is_merge_self_edge` — `merged_root(source) == merged_root(target)`
over the merge forest — which is exactly the slot coincidence the merged
allocation resolves to (`static_slot` canonicalizes through that forest). When the
predicate fires, `emit_increfs_for` **drops** the `IncrefRegion` rather than
emitting it; the detection isolates the redundant self-edge from the two must-keep
classes above by construction, because the merge seed never collapses an escaping
alias (it is not sole-held) nor a clique edge (it is not a `%pair` immutable
store), so their endpoints keep distinct merge roots.

This elimination is half of one mechanism with the merge's allocation
canonicalization and child-decref suppression (merging.md § "Emission: one
slot per merge tree, one demise at the root"): a self-edge dropped without the
merge frees early, and a merge without the drop leaks, so neither side is emitted
without the other. Its correctness net is not a per-edge runtime assert — once both
endpoints share a slot, a slot-vs-slot check is a tautology — but the compile-time
decref-dominance assertion (exactly one `DecrefRegion` per merged slot,
`record_merged_slots`) together with `--trace=guardfree` over the builder corpus
(an over-collapse surfaces as a UAF; a self-edge left in place grows the live
region count). The pinning test is the canonical reference
(`tests/elle/region-merge-builder-loop.lisp`).

## The equivalence oracle

A mis-coalesce is a use-after-free: a slot resolved to the wrong physical region
makes its cascade free a live region. The net for a coalesced *mint* (the
value→slot substitution, § "Compile-time region selection") is the debug-only
`AssertRegionMatches { region_id, src }`, emitted immediately before every
coalesced `IncrefRegion`. (Self-edge *elimination* carries no coalesced incref to
guard — its net is the decref-dominance assertion and guardfree, § "Self-edge
elimination".) In the bytecode interpreter it panics when
`activation_region_map.resolve(region_id) != region_of(src)` — turning an
inference bug into a deterministic panic at the exact instruction, under the
trustworthy guardfree oracle, instead of a later heap corruption (the mirror of
the native-effect declaration oracle, [effects.md](effects.md)).
Release builds and the JIT/WASM tiers treat it as a no-op (the GPU tiers exclude
any function carrying it via the `is_gpu_instruction` whitelist); their coalesced
sites are covered instead by the runner's cross-tier divergence detection and the
escape golden. The instruction renders into no `[region_instrs]` golden line — it
is scaffolding, not part of the semantic RC stream.

