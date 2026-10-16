# The letrec closure-cycle merge

<!-- audited: 2026-09-28 -->

Mutually recursive closures hold each other through forward cells, so RC never reaches zero; the merge collapses the cycle onto one arena.

The builder-idiom seed merges one tight `child → parent` store edge. The same
collapse-to-one-region mechanism reclaims a shape per-region RC cannot: the
**immutable reference cycle that mutual recursion forms** (`ping`/`pong`). Each
member is a capture-cell↔closure structure. The forward-reference **cell** holds
the closure (`StoreCaptureCell`), and a *sibling* closure **captures** the cell,
both at the binding scope, never mutated. The cells and closures reference each
other around the SCC, so per-region RC never reaches zero
([rules.md](rules.md) Rule 8), and the cycle leaks. A *mutable* `@array` cycle is
the deliberate boundary ([adopt.md](adopt.md)). An immutable cycle is
reclaimable, and a fiber that builds one per loop iteration would otherwise leak
without bound.

**The binder form does not decide the shape.** A run of local `defn`s in one
body prebinds the same forward cells a `letrec` does, for the same reason: a
sibling reads a name before its initializer has run. So the two express one
shape, and the merge reads one predicate for both
(`BindingInner::compiled_forward_cell`). This document says **binding scope**
for the node that prebinds the cells: the `Letrec`, or the `Begin` the `defn`
run sits in. Its **body** is the `Letrec`'s body, or the `Begin`'s last
expression. That is the position whose value the scope yields, and the only one
of a `Begin`'s expressions a tail call can sit in. Where a claim is about the
letrec form alone, it says `letrec`.

Reading the `defn` run as a different shape leaks the whole cycle of the
closure-as-module idiom on every construction. That idiom is a constructor that
defines a set of mutually recursive helpers over its own mutable state and
returns a struct of them. The async scheduler has that shape.

**Self-recursion is not this shape.** A purely self-recursive local fn (`loop`
references only itself) is **cell-free**. Its self-edge does not mark it
captured ([scopes.rs](../../../src/hir/analyze/scopes.rs)), so it has no forward
cell and no cell↔closure cycle. Its self-reference resolves to the
currently-executing closure ([selfrec.md](../selfrec.md)). Ordinary RC and the
tail-call deferred release reclaim it, RC-identical to a top-level recursive
`defn`. So the merge is the **mutual**-recursion instrument, and a pure
self-recursive letrec never has a cell and never reaches it.

The merge collapses the whole cycle, the closure SCC **and** its cells, onto one
region. Every interior reference then becomes intra-region, and all three
ref-counting paths self-skip a same-region reference (`rid != own_id`):

- the alloc-scan incref over the closure env (`incref_cross_region_refs`);
- the capture-cell store incref (`capture_store_with_rebind` in
  [mutate.rs](../../../src/value/arena/mutate.rs));
- the free-time cascade
  ([introspect.rs](../../../src/value/fiberheap/regionpool/introspect.rs)).

So the merged arena carries RC 1, and one `DecrefRegion` frees the cycle
wholesale, with no edge accounting and no member list. A group-free is the wrong
instrument. It would free the closure SCC wholesale while the *cells*, in their
own regions outside the freed set, still referenced it. Each cell's own
`DecrefRegion` would then over-free a dangling closure, a use-after-free that
`--trace=guardfree` detonates under the full stdlib. Collapsing the cells *into*
the closures' region removes the dangling reference by construction.

The merge extends the `merged_parent` forest the builder seed populates, and
every tier already resolves its `merged_root` and `merged_slots`. So it runs on
every tier, with no new opcode and no JIT helper.

## Detection has two layers

`compute_closure_cycle_merges`
([merge/cycle.rs](../../../src/hir/region/infer/merge/cycle.rs)) detects the
cycle in two layers, because the cell↔closure structure is not one SCC in the
graphs the other passes build:

- The **closures** carry the cycle. A `closure ⊇ closure` capture graph is
  re-derived from each lambda's captures, since `binding_source_regions` of a
  captured binding is its *closure* region. Unlike `capture_containment_edges`,
  which drops the `r == closure_r` self-edge, this graph **admits the
  self-edge**. An SCC of size ≥ 2 is a mutual cycle, and the self-edge is
  redundant there. It matters only for the one mixed shape that still has a
  cell: a self-recursive member that a *sibling* also captures, but that is not
  itself in a mutual cycle. That is a size-1 SCC, and the self-edge admits it so
  its retained cell can merge into the closure. A *purely* self-recursive
  closure is cell-free and never reaches the merge.
- The **cells** are coincident-lifetime members hung off the SCC: each cycle
  binding's prebound capture cell (`begin_cell_regions`), paired in through the
  binding's source closure region.

The gates run in this order, and a cycle that fails any of them keeps the
Shared baseline:

1. ONE binding scope prebinds every member's cell.
2. Per member: off the fiber frontier, sole-held (waived for a handed-out
   member), with a sole-held static-slot cell.
3. The drop site: the binding scope, or the release point a handed-out member's
   own region already carries.
4. Every member's allocation site lies in the binding scope's subtree.
5. Every tail call in the scope's body has a release channel.
6. A member on the return frontier has its arena released after the mint that
   funds the caller.

The two frontier gates read escape's two frontiers apart:
`fiber_frontier_regions` for gate 2 and `return_frontier_regions` for gate 6.
They do *not* read `EscapeInfo::lambda_escapes_definition`, which also folds in
the capture facet (a value captured by an escaping closure). That is a
containment relation. An SCC's closures capture each other, so one member
crossing a frontier would propagate "escaping" around the whole cycle and
falsely refuse a mergeable one.

### The frontier gate — the fiber half refuses, the return half is return-funded

The two halves of the frontier read differently, because the merge's release is
a **decref, not a free**. What matters is only whether it can reach zero while
something else still reads the arena.

- **The fiber half always refuses.** An emitted or sent member crosses to a
  resumer or receiver whose hold the compiler did not place, and a parked frame
  may borrow it uncounted. The cycle stays Shared, the always-legal baseline.

- **The return half is admitted where the arena's release runs after the return
  mint.** A returned member does not outlive the arena's *count*. The merge
  collapses that member's region onto the arena, so the value handed out lives
  **in** the arena, and the mint that funds the caller raises the arena's own
  count. Three references are in play over one call: the frame's own (taken at
  the letrec setup), the caller's (that mint), and any container's (taken by the
  store funnel). The release the merge owns is the frame's. So the admission is
  entirely a question of **order**. A release that runs after the mint drops the
  frame's reference with the caller's already standing. On a path that returns
  something else, nobody else holds the arena, and the frame's reference is
  correctly the last.

  One structural question settles the order: **does the body hand the value
  over itself?** Every tail exit of the body must be a node that leaves the
  frame: a `Return`, or a tail `Call`. Each of the three ways such an exit mints
  puts the mint ahead of the merge's release:

  - a **`Return`** mints where it stands (`lower_return`'s
    `IncrefValueRegion`), inside the body. The binding-scope `DecrefRegion` the
    merge owns is emitted at the scope node, after the body's whole lowering, so
    the release follows the mint.
  - a tail call to a **closure** replaces the frame, so that `DecrefRegion` is
    dead code, and the release rides a deferral instead. That is the member
    channel (`stranded_cycle_bindings`) or the non-member channel
    (`deferred_release_slot`), whichever applies. `trampoline_loop` runs either
    at the recursion's **normal completion**, after the callee's `Return` mint.
  - a tail call to a **native** keeps the frame and falls through to that same
    binding-scope `DecrefRegion`. The call's own mint is emitted at the call
    site, inside the body, so it precedes the release too. That mint is the
    post-`TailCall` fall-through retain, or `lower_return`'s retain where ANF
    named the result.

  So *which* channel carries the release does not enter the ordering argument.
  Neither does whether the compiler can classify the callee: the merge wires
  both channels rather than choose between them, and both are late enough. The
  cell-free self-recursive deferral makes the same ordering argument for its
  return admission ([selfrec.md](../selfrec.md)). The release runs *after* the
  mint, so unlike the frame-exit relocation there is nothing to bridge.

  A body that falls out to a bare value hands its value to an *enclosing*
  consumer instead. The mint that funds the caller, if there is one at all, then
  sits outside the scope node. That is no reason to refuse. It is a reason the
  release cannot stay at the binding scope, and "Drop site — following a
  handed-out member" below follows the value out to where its own release
  already sits.

  This reading decides only whether the mint is inside the body, so an exit it
  does not recognize must read as outside. A loop and a short-circuit
  `And`/`Or` fall out to a value that way. A `Cond` with no else arm and a
  `Match` fall out on the path where no arm is taken. So this reading needs the
  arms EXHAUSTIVE, where branch compensation, which asks a different question,
  admits a `Match` arm exactly as an `If` arm
  ([merge/cycle/body.rs](../../../src/hir/region/infer/merge/cycle/body.rs)).

The gate is asked per SCC. The return admission is asked only when some member
carries the return facet, so a non-escaping cycle never consults the tail shape.

**A sibling capture that is not a cycle has no merge, and its cell rides the
frame-exit release.** The merge only reaches a cell whose closure is in an
admitted SCC. Where one member merely *calls* another (`go` calls `helper`, and
`helper` does not call back), there is no cycle at all. `helper` keeps a forward
cell for `go`'s benefit, and per-region RC reclaims both. The cell's release is
then the ordinary binding-scope `DecrefRegion`, which the lowerer emits after
the body. That release is dead code when the body ends in a frame-replacing
tail call, exactly as any other of the frame's releases is. The frame-exit
release carries it back ahead of the `TailCall`, and its count argument reads
the cell under its binding's own escape verdict ([relocate.md](relocate.md)).
Both faces apply: the one where nothing leaves the frame, and the one where the
capturing member is handed to the caller.

## The cells

The static-slot cell requirement is met in **every position**, top level and
inside a lambda body alike, and by a `defn` run exactly as by a `letrec`. A
binding that is immutable, never mutated, and lambda-initialized (the
recursive-closure shape) lowers its forward cell as a compiled
`MakeCaptureCell`, held in the binding's own stack slot.
`BindingInner::compiled_forward_cell` is the one predicate every site reads:
`lower_letrec`, `lower_begin`'s slot pre-pass, and the region walk's matching
arms. So a cell region is never a phantom (a region with no allocation) and
never missing (an allocation with no region). A binding **outside** that shape,
mutated or not lambda-initialized, keeps the runtime `populate_env` env-cell
route inside a lambda. It has no static slot and no `begin_cell_regions` cell,
so it refuses the merge.

**One binding, one cell.** `lower_begin`'s pre-pass descends nested `Begin`s,
so an inner one reaches a binding the outer one already claimed. It emits
nothing the second time, because the binding's slot exists. The region walk
owes the same reading. A second cell region for one cell is a phantom, but the
cost lands earlier. The cells of a cycle are keyed by the scope that minted
them, so a split mints the two cells of one cycle at two scopes. A cycle whose
cells name two binding scopes has no single drop site, and the merge refuses
it. The walk therefore skips a binding some scope already minted a cell for.

## Drop site — the binding scope

A cycle whose members stay inside their scope has no member whose natural last
use post-dominates the rest, since no containing parent pins it as the builder
idiom's does. So the merge sets the canonical root region's `decref_point` to
the cycle's **binding scope**: the single node that prebinds every member's
capture cell (the `begin_cell_regions` key). Structural ancestry decides this,
never a numeric `compute_order` compare ([adopt.md](adopt.md)). The root is the
SCC closure of least program order, since region ids order nothing. Any member
mints the shared physical region at runtime (mint-or-reuse), so the root only
names the merged slot and carries the single decref.

The binding scope is the right post-dominator, and its enclosing scope is not.
The members are bound in that one scope, so **every direct reference to a member
is lexically within it**. The scope exit, the lowerer's `emit_decrefs_for` on
the node after its whole body, post-dominates them all. Two things can carry a
reference *out* of the scope, and only one of them is handled here.

A **foreign capture**, a closure outside the SCC that holds a member, is a
cross-region reference *into* the merged arena, and it is RC-counted.
`incref_cross_region_refs` increfs it when the capturing closure is built,
scanning the env and recording the outgoing edge. The free-time cascade
releases it when the capturer's region frees. So it keeps the arena's RC ≥ 1
past the single decref, and the arena survives until the capturer dies. The
single `DecrefRegion` therefore releases only the cycle's own allocation
reference, promptly, and can never free a still-referenced arena. The other
carrier, the scope's own value, is uncounted and moves the drop site instead,
as the next section shows.

Eligibility is gated on **subtree containment**: every member's allocation site
must lie within the binding scope's own subtree. The cells' sites *are* the
scope node, and the closures' `Lambda` nodes are its init descendants, so the
binding scope is a structural ancestor-or-self of every member by construction.
A member whose region reaches the SCC from outside that subtree refuses the
cycle, such as a reused binding identity naming a foreign lambda.

The binding-scope drop is strictly tighter than the enclosing structural
post-dominator. The cell target sits *at* the binding node, whose
enclosing-scope stack excludes itself, and that drags the allocation-site common
ancestor up to the binding scope's **parent**. For a top-level discarded cycle,
that parent is the file `Begin`, so the cycle would wait for program teardown.
Dropping at the binding scope itself frees a discarded cycle at its scope
instead. The remaining slack is bounded by that one scope: the scope exit can
still fall after a member's last use within the body, a granularity nit.

## Drop site — following a handed-out member

Foreign capture is not the only way out of the binding scope. The **scope's own
value** is a second, and unlike a capture it is uncounted. When the body falls
out to a bare value that reads a member binding, the enclosing consumer's
binding names that member's region directly. A `Var`/`DerefCell` read mints
nothing, so the value outlives the binding scope on no reference but the
cycle's own. Pinning the release at the binding scope there would take the
arena to zero under a live holder.

One fact discharges both consequences. The handed-out member's region already
carries a `decref_point`: the ordinary last-use rule extended it through the
outer binding. Where the value is returned, the rule pinned it at the enclosing
`Return`, whose `IncrefValueRegion` mints the caller's reference *before* its
own `emit_decrefs_for` runs (the `return_sites` extension,
[mechanism.md](mechanism.md)). That point is where the unmerged compilation
already emits this member's release. Adopting it as the **arena's** drop site
adds no release the program did not have. It only widens what the one release
covers, to members whose uses all lie inside a scope that point post-dominates.
So:

- the release **follows the value out**. The drop site is that common
  `decref_point`, admitted when it lies outside the binding scope's subtree and
  post-dominates it (`postdom::drop_post_dominates`, `EmitMode::Merge`). The
  handed-out members must agree on it, since one arena carries one release.
- the **sole-held** requirement is waived for exactly those members.
  Sole-heldness is a proxy for "no second name reaches this member", asked
  because the binding-scope drop cannot see past itself. The adopted point is
  the real thing, computed over every holder's last use rather than assumed from
  their count. Cells and members the scope does not hand out keep the proxy.

A body whose handed-out value the reading cannot follow to a single
post-dominating point keeps the Shared baseline.

## A tail-call body

When the body ends in a frame-replacing tail call, the drop site is dead code,
and the arena's release rides a deferral instead
([a merged cycle behind a tail call](cycletail.md)). Comments across the tree
name that argument's parts by their titles here. Each title below resolves in
one step.

| Argument | Now in |
|---|---|
| A tail-call body hands the drop to a deferred release | [cycletail.md](cycletail.md) |
| The arena channel and the callee channel are independent | [cycletail.md](cycletail.md) |
| The by-move boundary | [cycletail.md](cycletail.md) |
| The refusal answers a callee that REPLACES the frame | [cycletail.md](cycletail.md) |

## Pinning tests

`region::infer::tests::merge::recursion` pins the collapse:

- `merge_collapses_mutual_recursion_letrec_closure_cycle`: the mutual SCC and
  cells collapse onto one `merged_root`.
- `merge_collapses_in_lambda_mutual_recursion_letrec_closure_cycle`: the same
  for a letrec that is a lambda body.
- `merge_mutual_recursion_cycle_drops_at_binding_scope_not_enclosing`.
- `self_recursive_letrec_is_cell_free_not_merged`: a pure self-recursive letrec
  has no cell and is never a member.
- `merge_collapses_self_and_sibling_captured_member_cell`: the mixed member's
  retained cell still merges.

The **`defn` run** is its own family in `merge::recursion`, because the binder
form is what the reading turns on. In
`merge_collapses_defn_run_closure_cycle_in_lambda`, the same SCC and cells,
written as two local `defn`s, collapse onto one `merged_root` and drop at the
`Begin`. `merge_collapses_defn_module_factory_cycle` pins the closure-as-module
shape: a struct of the members, bound out of tail position, over a mutable table
the members capture. Their counterfactual is that both read the drop site at
ONE scope. A walk that minted a second cell for a binding a nested `Begin`
reaches again splits them, and the merge refuses.
`region_ownership_reclaims_defn_module_factory_per_call` measures that from the
other side.

The **frontier gate**'s faces are pinned as one family in `merge::escape`, so
the two halves cannot drift into each other. The return half's admission is
pinned across all three ways a body leaves the frame:
`merge_admits_returned_member_cycle_on_member_tail` (a member tail call),
`merge_admits_returned_cycle_on_non_member_tail` (a non-member one, whose two
callee kinds take the two late channels), and
`merge_admits_returned_cycle_on_value_tail` (a bare member value, whose `Return`
mints inside the body). `merge_admits_returned_cycle_bound_out_of_tail_position`
pins the value handed to an enclosing consumer, where the release follows the
member out. It asserts the merge AND the drop site, since either alone is
unsound.

Their counterfactual is `merge_refuses_returned_cycle_whose_body_hands_out_nothing`.
A body that neither leaves the frame nor names a member at its tail has no order
to read either way, so "returned and out of tail position" is not by itself an
admission. `merge_refuses_fiber_crossing_letrec_cycle` holds the fiber half,
which no mint can fund. Its body's tail *is* a member call, so the fiber facet is
the only gate left to bite. `merge_refuses_returned_cell_free_self_recursive_closure`
(in `merge::recursion`) guards the reading itself. A returned *self*-recursive
closure is refused by CELL-FREEDOM, not by the frontier, so that refusal must
not be read as "returned ⇒ refused".

At runtime, two guardfree tests pin the merge. [region-letrec-return-cycle-uaf.lisp](../../../tests/impl/region-letrec-return-cycle-uaf.lisp)
re-enters a returned member after the deferral, across churn that recycles a freed page, and
holds handles live across later mint/free cycles. [region-defn-cycle-uaf.lisp](../../../tests/impl/region-defn-cycle-uaf.lisp) is the
`defn` run's own fixture. It re-enters a member of a returned factory after the
arena's drop site has passed, and drives the factory across churn that recycles
a freed page.

`runtime::tests::ownership::selfrec` holds the reclamation gauges:

- `region_ownership_reclaims_mutual_recursion_closure_cycle`: bounded per-run
  region growth beside a leaking discriminator.
- `region_ownership_reclaims_returned_mutual_cycle_per_call`: the same for the
  returned cycle.
- `region_ownership_reclaims_returned_cycle_bound_out_of_tail_position`: the
  handed-out member, both halves. One returns the value out of the frame, and
  the other calls it in place and never hands it further.
- `region_ownership_reclaims_nested_mutual_recursion_per_call`: the in-lambda
  cycle per call, bounded beside the live-chain discriminator, base case
  included.
- `closure_cycle_discarded_release_is_prompt`: the binding-scope drop's
  promptness. A discarded top-level cycle is freed at its scope, not held to
  teardown, which is the case that must NOT pick up a later drop site.
- `region_ownership_reclaims_defn_module_factory_per_call`: both drivers of the
  closure-as-module factory on the VM.
- `region_ownership_reclaims_self_recursion_closure_cycle`: the same bounded
  growth for a pure self-recursive closure, which is reclaimed cell-free
  ([selfrec.md](../selfrec.md)), not by this merge.

`runtime::tests::ownership::jit::region_ownership_defn_module_member_call_under_jit`
gauges the member call with the CALLER compiled. On that tier, the tail call into
the member hands its deferral forward to the callee's activation
([relocate.md](relocate.md)).

The oracle reads `recur-local-mutual-ret`, `recur-local-mutual-ret-foreign`,
`recur-local-mutual-ret-value` and `recur-local-mutual-ret-bound`, the four body
shapes, all closed at 0. `recur-local-defn-mutual` and `defn-module-factory` gauge the `defn` run and the closure-as-module factory
built from one. The `defn-module-factory` op constructs the module and calls a
member back through the returned struct, which is the driver the interpreter and
the JIT must agree on. The probes live in
[the tail-call probes](../../../tests/impl/probe/tailcall.lisp).
