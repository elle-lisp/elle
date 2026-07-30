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

Pinned by `tests/elle/region-return-arm-escape-leak.lisp` (both faces: the
non-returning arm is bounded, and the returned value survives its caller's use), and
for the `Match` arm by `tests/elle/region-match-dead-arm-leak.lisp` (both faces
again, plus the return-escaping value whose dead `Match` arm hands the caller
nothing).

The **used** sibling arm is the residual, and its guard is not negotiable. A release
there is admitted only where a retain on the same node funds it (the store, the
`-mut` container, the return mint above). The tempting generalization — "the arm's
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
([escape.md](../escape.md)): a value that leaves its activation by **no** facet —
return, store, capture, fiber — is reachable only through this frame's slots. So
the window is admitted for a region whose every holder binding is non-escaping and
non-mutated, and which is absent from the return and fiber frontiers' atomless
site halves (which no binding names). A region with no holder binding at all
offers nothing to judge and is refused too. Everything else keeps its in-arm
release and the per-arm compensation routes above, which carry a count argument
instead — so the two mechanisms partition the obligation rather than overlapping
on it.

The **mutated** refusal is the one compensation makes about a release *route*: a
slot repointed between the arm and the anchor frees whatever it holds then.

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

What *is* refused is capture by a closure that **escapes**: there the closure
outlives this activation, and escape's capture facet already marks every binding
such a closure captures. That is a flow fact. The structural capture-graph
(`regions::escape::captured_bindings`) marks every captured binding whether or not
its closure ever leaves — the right conservatism for the **merge** gate, which
asks where a value may *live* and so needs raw reachability, and the wrong one
here, where the question is who holds a count.

#### A mutated holder poisons its value route, not its cell box

The mutated refusal is a claim about a *route*, so it reaches exactly as far as
the route does. A value-routed release loads the holder's slot and frees whatever
region the value it finds there lives in — which is why a slot the program
repoints cannot carry one, and why the release is skipped for such a slot
entirely ([bindings.md](bindings.md), "a mutated slot is not a release route").

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

Three bound the window, the same three the break window carries and
for the same reasons. Two are about *how many times* a release runs:

- **An iterative scope nested in the branch** (`While`/`Loop`) holding the
  `decref_point`. A release inside it runs per iteration; hoisting it past the
  loop would leave one release covering N executions.
- **A `Lambda` nested in the branch** holding it. Its body's releases run in a
  different activation against a different frame's slots, which never reach this
  branch's merge label.

The third guards the anchor itself — the hoist's premise is that the merge label
is a point every arm **reaches**:

- **A frame-replacing tail call in the branch.** A tail call to a *closure*
  replaces the frame, so that arm leaves through the callee and never arrives at
  the merge; a release moved there would be dead on exactly the path that runs
  it. A tail call to a **native** pushes no frame and falls through to the merge,
  which is why the callee kind decides this and not the `is_tail` flag: the
  native-tail dispatch shape is the whole point of the window. The branch
  declines whole when any arm can leave through a callee.

The region must also be **live-in** to the branch (every allocation and
holder-definition site outside the branch's subtree) — the same premise
compensation states — so a value born inside an arm keeps its in-arm release,
and the window only moves releases of values the branch received.

Regions whose release belongs to another mechanism are excluded exactly as in
compensation: merge children, co-owned-group members, capture cells, the
mutated-slot 1-slot containers, and anything already suppressed.

Pinned by `tests/elle/region-branch-arm-window.lisp` (the reclamation, with all
three boundaries, the `If` face and the captured-holder face driven as rows), the
`param-used-arm` / `param-used-arm-if` probes in `tests/elle/oracle.lisp` (the
per-op rates), and `tests/elle/region-branch-arm-window-uaf.lisp` (the soundness
complement — a value read, stored, returned, carried across a yield, or reached
through a closure's environment after the branch must survive the moved release).

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

- **Region flow** (`hir/regions/walk`, the `Block`/`Break` arms): a `Block`'s
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
activation takes over its release (`defer_callee_release`, `deferred_release_slot`).

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

**Whether the frame is the sole holder** — the admission, and escape is its sole
authority. The exemption above is a statement about *arguments*, and arguments are
not the only path into a callee: a tail callee reaches its **captured environment**
too, which no argument names and no callee region describes. `push-all`'s walker
is exactly that shape — `(letrec [go (fn [i] … dst)] (go 0))` names `dst` only
through `go`'s env. That path needs no enumeration and no refusal of its own,
because the env's hold is a counted (or owning) edge the funnel took when the
closure was built (§ "Lexical capture is not a second holder to fear"): a release
of the frame's reference leaves the callee's standing. The predicate is one and
the same for both mechanisms (`RegionInfo::sole_frame_held_regions`): every holder
binding non-escaping, no holder mutated except where the release names a cell box
rather than the mutated slot (§ "A mutated holder poisons its value route, not its
cell box"), and the region absent from the return/fiber frontiers' atomless site
halves.

So this close covers a parameter or local the frame alone owns — captured by a
locally-called closure or not — whose release lands at the body's scope exit, and
with it the **env cell** of a captured local, whose `DecrefCellRegion` lands in
the same dead block. The value the callee hands **back** is a separate question
with a separate funding argument, below.

### The callee's return mint, and the edge that funds the gap

A region on the **return** frontier fails the admission above, and the shape that
fails it is the same walker one parameter over: `push-all` returns `dst` through
`go`. Read as a count question the refusal is right in general and too strong
here, and the difference is one edge.

A relocated release is safe when the reference it drops is not the region's last
*live* one. For a value the callee merely **reads** — the walker's `src` — the
frame's release is the last one and nothing reads the region after the frame is
gone, so nothing needs funding. For a value the callee **returns**, the caller does
read it afterwards, through a reference the callee's own `Return` mints — and that
mint fires *after* the relocated release. Between the two the count must not reach
zero.

The tail callee's own hold is what keeps it off zero, and the system already
counts it. A callee reaches a value this frame owns by exactly two routes: as an
**argument**, where the release stays in the dead block and is the ownership move;
or through its **captured environment**, where the funnel took a counted (or
owning) edge when the closure was built. That edge is dropped by the closure
region's free-time cascade, which the deferred callee release runs at the callee's
*completion* — after its `Return`. So the order over one call is: env edge taken,
frame release, callee mint, env edge falls away — and the reference left standing
is the caller's.

The admission is therefore asked **per relocation point**, not per region: a
region whose only escape facet is the return one is relocated at a point whose
callee is a closure capturing one of the region's holder bindings
(`TailCalleeFacts::capture_funded`, keyed by the call node), and keeps the
baseline at every other point. Nothing here weakens the sole-holder admission —
the two are read together, and a region that clears the sole-holder predicate
needs no funding because no one reads it after the frame.

Every other facet still refuses, and each for the reason it always did: a holder
that crosses the **fiber** frontier may be borrowed uncounted by a parked frame; a
**mutated** holder is a release route that frees whatever the slot holds then,
except where the release names the cell box the mutation leaves alone (§ "A
mutated holder poisons its value route, not its cell box"); a
holder captured by a closure that **escapes** leaves with it. What is dropped is
only the return facet's blanket refusal, and only where the callee's edge replaces
it — which is why escape must be able to say "*this* facet and no other"
(`EscapeInfo::binding_escapes_beyond_return`, the complement of
`binding_escapes_via_return`).

The **residual** is a returned region the tail callee does not capture: some other
path of this frame returns it, so no env edge funds the point and the release
keeps its place in the dead block. That is a leak, never an over-free.

### A compiled capture cell is frame-held exactly as its binding is

Both admissions read the frame's holders through `binding_source_regions`, so a
region **no binding names** offers nothing to judge and both refuse. A compiled
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
says about the binding's regions it says about the cell's, by both facets and by
the mutated-holder reading alike. Projecting each binding's single compiled cell
region (`RegionInfo::single_cell_region_of`) alongside its `binding_source_regions`
therefore asserts no admission the predicate was not already making; it names a
region the predicate could not see.

The funding side owes the same projection. A closure captures a `needs_capture`
binding **through its cell**, so the counted edge the tail callee holds is
`closure ⊇ cell`, and `TailCalleeFacts::capture_funded` names the cell region as
well as the closure region the cell points at. Without it the return half admits
the captured closure and strands the cell holding it — one region short of the
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
that is not exactly load / release-by-value / nil-stamp keeps the baseline. Scope
regions need nothing here anyway — `lower_call` already frees them before every
`TailCall`.

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
the non-self-cancelling boundary, the env-cell faces, and the
handed-back-through-the-callee faces, and the forward-cell faces driven as rows),
the `tail-frame-exit-unused` /
`tail-frame-exit-moved` / `tail-frame-exit-arms` / `tail-frame-exit-captured` /
`tail-frame-exit-handback` / `tail-frame-exit-fwd-cell` /
`tail-frame-exit-fwd-cell-ret` / `fresh-env-cell`
probes in `tests/elle/oracle.lisp` (the per-op rates), the analysis-level
projection pins in `regions::tests::cells`
(`frame_held_names_a_sibling_captured_forward_cell`,
`capture_funded_names_the_captured_binding_forward_cell`, and their
escaping-holder counterfactual), the placement pins in
`lir::lower::tests::release`, and
`tests/elle/region-tail-frame-exit-uaf.lisp` (the soundness complement — a value
moved into the tail callee, reached through its captured environment, filled in
place by it, handed back out through it, handed back when the frame holds the only
other reference, held in an env cell the callee rewrites, held in a sibling's
forward cell the callee reads on every recursion, captured by a closure
that escapes, or read after the call must survive the moved release).

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

