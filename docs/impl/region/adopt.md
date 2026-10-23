# Ownership adopts and the root's lifetime obligation

<!-- audited: 2026-10-21 -->

The owner edges no store site names, the capture and funnel adopts, and the lifetime obligation a root's single demise must meet.

These edges build an [ownership forest](ownership.md) subtree. The **capture
adopt** links a closure ⊇ its captures, and the **funnel adopt** is the
opaque-store face of a container ⊇ its member. The post-dominance + emit-order
**obligation** is what the root's single demise must satisfy for a subtree drop
to be sound. The last section says where the hybrid still keeps per-region RC.

## The capture adopt — emitted at the closure, for every capture kind

A capture containment edge (`closure ⊇ captured`) has no store site. Capture
records no `cross_region_refs` edge: the runtime auto-incref over the `Closure`
env stands in for a static `IncrefRegion`, so a static edge would count it
twice. So its adopt is keyed by the closure's **construction**. At
`MakeClosure`, `lower_lambda_expr` reloads each adopted captured value and emits
a value-resolved adopt in place of the capture's baseline `IncrefRegion`.
`analyze_regions_with` suppresses the member's own compiler decref, because the
closure's subtree drop is its sole reclamation. How the edge points depends on
how the captured binding is materialized (`ownership::capture`):

- **By-value capture** — an immutable, non-prebound local the closure holds
  directly. The edge is `closure ⊇ content`, adopted with
  `AdoptRegion(closure, content)`. The reload covers a direct local (from its
  binding slot, `LoadLocal`). It also covers a by-value upvalue/transitive
  capture (from the constructing function's environment, `LoadCapture`).
- **Immutable letrec forward-reference cell** — a prebound `MakeCaptureCell` a
  sibling references before its initializer runs. The closure holds the CELL,
  not the content, so the edge is re-pointed at the **cell region**
  (`single_cell_region_of`) → `closure ⊇ cell`. The walk pairs it with a
  `cell ⊇ content` edge (`record_cell_content_edges`, keyed at the cell's mint
  scope). So external uniqueness sees the true chain `closure ⊇ cell ⊇ content`.
  A **local, non-escaping `{closure, cell, content}` clique** is then externally
  unique → Owned. The closure's subtree drop reclaims it as a unit, the interior
  cell↔closure reference included.

  The cell store is UNCOUNTED, so this re-pointed edge is the ONLY way the scan
  sees the cell is held. The runtime realizes it with **`AdoptCellRegion`**,
  which resolves BOTH operands with `region_of`. It never uses
  `result_region_of`, which would unwrap the cell to its content, so a cell's OWN
  region is named. A second `AdoptCellRegion` at the cell store links the
  content into the cell (`cell ⊇ content`, `maybe_emit_cell_content_adopt`)
  where the lifetime obligation admits. Elsewhere the content reclaims by the
  cell's free-time RC cascade.

  Pinned by
  `region::infer::tests::cells::{walk_records_cell_contains_content_for_compiled_letrec_cell,
  capture_edge_points_at_cell_region_not_content}`, and by
  `region::infer::tests::adopt::owned_subtrees_admits_local_capture_cell_clique`
  with its escaping / two-sibling refusals. The emit is pinned by
  `lir::lower::tests::release::preallocated_capture_cells_get_distinct_regions_each_released`.
- **Re-storable cell** (`is_restorable_capture_cell` — an `@`-mutable captured
  local or a mutated captured parameter) is a **borrow**, with no owner edge.
  Its content lifetime is per-rebind, because the rebind funnel decrefs each
  displaced prior. That lifetime is SHORTER than the cell's, whose release is
  hoisted once past enclosing loops. Adopting the content into the cell's
  subtree would free a displaced prior under the live cell (the loop
  over-free).

  So `capture_containment_edges` skips the capture, and
  `compute_adopt_edges`'s `adoptable_cell` refuses its `cell ⊇ content` edge.
  The walk still records that edge (the cell holds *a* content) for
  external-uniqueness counting, and the content reclaims on the per-region-RC
  baseline. Pinned by
  [region-capture-cell-loop-uaf.lisp](../../../tests/impl/region-capture-cell-loop-uaf.lisp)
  (the guardfree witness) and
  `region::infer::tests::adopt::{capture_edge_skips_restorable_cell_admits_immutable_in_one_clique,
  restorable_compiled_cell_records_content_edge_but_is_not_adopted}`.

A mutually-recursive `letrec` closure **cycle**, where each closure holds the
other's forward cell, is a *cyclic* clique, not a rooted subtree. The
closure-cycle **MERGE** ([letrec.md](letrec.md)) reclaims it, collapsing the
SCC ∪ its cells onto one arena before this pass. The capture-adopt path never
does; the `recur-local-mutual` and `recur-local-self` probes in
[the tail-call probes](../../../tests/impl/probe/tailcall.lisp) read closed. The
capture-adopt path serves the *acyclic* rooted clique above.

The capture-adopt contract is that every suppressed member is adopted. **Emit
capability** discharges it, not a refusal of shapes the emit cannot reach.
`lower_lambda_expr`'s reload covers every capture kind. The `debug_assert` at
the emit is the backstop that every adopt edge matches a real capture of the
constructed closure. Pinned by
`lir::lower::tests::basics::capture_adopt_reloads_upvalue_via_load_capture` (the
env-reloaded emission) and
`region::infer::tests::adopt::capture_adopt_edges_are_emittable`.

The general subtree filters bound the *admission* of a capture owner-edge, and
no lowerability filter exists. The **cross-activation (upvalue) owner-edge is
refused at the lifetime obligation**. With the capture edge re-pointed through
the cell, the containment is VISIBLE, so a subtree over an upvalue member DOES
form. External uniqueness admits `{nested, cell, enclosing, member}`: the member
is captured by BOTH the nested closure and the enclosing forwarding lambda.

The refusal then holds structurally at the lifetime obligation. The nested
closure's region is minted per CALL of the enclosing closure. Claiming a member
that survives across calls would free it under the enclosing environment's
still-live reference, and re-adopt an already-owned region on the next call.
The forwarding capture resolves the member's tight last-use to a position
at/past the enclosing lambda's node. That is after the nested root's in-body
drop in post-order, so the obligation cannot prove the member dies before the
root, and refuses.

A member reachable through an upvalue capture is therefore Owned only by an
owner that outlives **every** capturer. That owner is the activation/fiber owner
node of the owner = activation cut, never a region root. Until that owner
exists the shape stays Shared, the always-legal baseline. Pinned by
`region::infer::tests::adopt::owned_subtree_upvalue_capture_owner_refused_on_lifetime`
and `closure_web_capture_not_yet_claimed`, and at runtime by
`runtime::tests::ownership::upvalue_capture_family_runs_sound`.

## The funnel adopt — the store face

Every storing/removing/copying `%`-op lowers as a native funnel `Call`
(`IntrinsicOp::routes_native_funnel()`). Such a compiled mutable store is an
opaque `Funnel` native call that records **no** `cross_region_refs` edge. The
runtime funnel counts the store, and a compile-time edge would double-count it
([effects.md](effects.md)). So the store-keyed adopt would find no emittable
interior edge, and every funnel-built subtree would refuse to Shared.

The containment is recovered instead from the container argument's `RetType`.
The walk's `Funnel` arm records it in `RegionInfo::containment_edges`,
**site-keyed** as `(funnel call site, contained, container)`.
`compute_adopt_edges` admits those edges as a third interior owner-edge kind,
beside stores and captures. The adopt rides the same `owned_adopt_edges` map,
keyed at the **funnel call site**. `emit_increfs_for` emits the same
value-resolved `AdoptRegion(container, member)` there. Both endpoints reload from
their binding slots, so no store opcode is needed. The activation and transfer
cuts carry the same funnel face. Funnel-recovered `containment_edges` give the
forest the same containment facts a constructor-embedding store declares.

The RC composition needs no explicit balancing. By the time the adopt executes,
the funnel's runtime store-incref has already counted the member.
`adopt_region` moves it `Counted → Owned`, **consuming the whole count**. Two
decrefs then land on the frozen `Owned` mode and no-op structurally: the
member's own later `DecrefValueRegion`, and the container's free-time cascade.
The lifetime obligation sequences the first before the root's drop. The root's
subtree drop is the member's sole demise.

A funnel-adopted member is a **store**-adopted member for the lifetime
obligation. Its own decref stays live, so its structural `decref_point` bounds
it, under `EmitMode::Adopt` with the loop clause applied. The emit site is the
funnel call that stored the member into its owner. It structurally follows both
allocations, so both slots are populated when the adopt runs. A containment
edge whose member the adopt cannot key (no owner edge at any site) still refuses
the subtree to Shared, the always-legal baseline.

The funnel face does **not** cover the builder-idiom MERGE. That is the
constructor emit's mechanism, not the store's. `%pair` lowers as the inline
`Intrinsic` opcode, whose `emit_alloc` seeds the `merged_slots` mint-or-reuse
([merging.md](merging.md)). So the merge rides the `%pair` emit on every compile
where a builder idiom appears, and no funnel edge is involved.

Pinned by
`region::infer::tests::adopt::adopt_edges_claims_interior_cycle_member_by_root`
(the funnel-site adopt edges) and `…::adopt_edges_refuses_loop_enclosed_member`
(the obligation holds on the funnel face). At runtime,
`runtime::tests::ownership::region_ownership_reclaims_interior_cycle_subtree`,
`…_reclaims_nested_cycle_subtree` and `…_reclaims_bare_cycle_group` pin it,
bounded flag-on beside the leaking flag-off counterfactual.

## The fiber member — refused at the class level

The forest is **rooted at fibers**. A fiber may own regions, through its
activation and fiber owner nodes ([owner.md](owner.md)). But the region a
`Fiber` object lives in is never a *member* of a region-rooted subtree, a
co-owned group, an activation SCC, or a transfer summary.

Every other value enters fiber-visible state through a compile-time-visible
escape facet (yield / emit / send / return), so the lifetime obligation can
bound its borrows. A fiber **value** acquires aliases by merely *running*. The
scheduler's parent/child chain, the restarts system, and the `fiber/child` /
`fiber/parent` graph reads all create references at runtime. They do so over
edges no structural post-dominance predicate can see. Adoption freezes a
region's RC, so the pass-through retain such a read hands the caller cannot pin
an adopted fiber's region. So the class is refused at admission rather than
compensated at any read site.

The refusal is one class shared by every cut. The region walk records the
result region of each native call whose declared
[`RetType`](../../../src/primitives/def.rs) is `Fiber`, in
`RegionInfo::fiber_result_regions`. `fiber/new` is the mint. A fiber reached
through any non-primitive route is a non-`Fresh` call result, already refused as
a runtime placeholder. `ownership::inputs::not_ownable` counts the class among
the dynamic-lifetime refusals. So any candidate component containing a fiber's
region stays Shared, the always-legal baseline.

A refused fiber region reclaims on ordinary RC. Its holders' counted references
release as they die: bindings, container stores, and the embedding
closure/fiber alloc-scan. A parked fiber's chain state discharges on the free
path ([owner.md](owner.md)). The runtime backstop is a debug assert at
`RegionStore::adopt_region`: an adopted region's pool holds no live `Fiber`
object.

Pinned by `region::infer::tests::adopt::owned_subtree_refuses_fiber_member`
(beside its admitting `@array` twin) and the guardfree witness
[region-fiber-exhume-uaf.lisp](../../../tests/impl/region-fiber-exhume-uaf.lisp).
[fibers.lisp](../../../tests/lang/fibers.lisp) (the propagate child-chain reads)
and [grpc.lisp](../../../tests/lang/grpc.lisp) exercise the class under the full
scheduler.

## The lifetime obligation the root carries

Subtree drop fires at the **root's** single `DecrefRegion`, at the root's
`decref_point`. For that to be sound, the root's demise must **post-dominate
every member's last use**. A child read after the root's `decref_point` would
be freed out from under the read.

Adoption and merging discharge this through **one** structural post-dominance
predicate, `region::infer::postdom::drop_post_dominates`. It decides over the
scope tree's post-order subtree intervals, *not* by `compute_order` magnitude.
`compute_adopt_edges` **refuses** a component with an un-post-dominated member,
which stays Shared, the always-legal baseline. Gate 6 refuses such a merge
([merging.md](merging.md)). Both read each region's `lifetime_point`: the last
use before [the branch-arm window](window.md) moved any release.

The two differ only by `EmitMode`. ADOPT's store-member keeps its own decref,
so a loop enclosing the root's free is the cross-iteration UAF the predicate
refuses. MERGE's child is reachable solely through the parent (conditions 1+4),
so the loop clause is waived. The external-uniqueness walk
(`region::infer::ownership::compute_owned_subtrees`) proves the *frontier* is
unique, but does not by itself order lifetimes. So this obligation is the
emit's, not the walk's. The e2e reclamation tier pins it: an
interior-outlives-root shape must stay Shared, never adopt.

**Post-domination is necessary but not sufficient — the emit order carries the
rest.** Node-granularity post-dominance admits a member whose `decref_point` is
the **same node** as the root's demise. That is the straight-line coincident
case: a fresh container built and consumed in one expression. At that shared
node the *intra-node emission order* then decides soundness. A store-adopted
member keeps its own `DecrefRegion`, whose no-op depends on the member still
being **`Owned`**. So it must be emitted **before** every release that can free
the member's owner at that node. Two facts make "before" non-trivial for a
container root:

- The root of a mutable-store subtree is typically a `Fresh` **call-result**
  region freed value-based. It carries **more than one** runtime reference: the
  holder-binding release *and* the **discarded pass-through result** of the
  store itself. `%array-push`/`%put` return their container, so the store's own
  result is a second call-result region that resolves to the root at runtime.
  When that result is discarded, it releases the root. Whichever release zeroes
  the root triggers the subtree drop. The obligation's single
  `region_data[root].decref_point` names only one of them.
- A store-adopted member's own `DecrefRegion` is a structural no-op **only while
  the member is `Owned`**. Once the subtree drop has reclaimed it, that
  slot-resolved decref faults
  ([refcount.rs](../../../src/value/fiberheap/regionstore/refcount.rs), the
  phantom/double-free assert).

So the emit orders every adopted member's release **before its owner's** at each
shared `decref_point`. `with_region_info` topologically sorts the adopt edges
(`owned_adopt_edges` ∪ `capture_adopt_edges`, member → owner). The forest gives
each member exactly one owner, so the graph is acyclic and the order always
exists. It orders *nested* subtrees innermost-first (member ⊂ mid ⊂ root), which
a flat members-first bucket class could not. A member's release reads and frees
nothing while `Owned`, so it no-ops. Whichever root-freeing release fires
afterward subtree-drops the member exactly once. Regions no adopt edge relates
fall back to the page-read-depth tie-break ([rules.md](rules.md)).

[ownership.md](ownership.md) states the invariant this restores positively: a
store-adopted member's decref hits the still-frozen `Owned` region, a no-op,
because it is emitted before the root's drop. The reference for the
inverted-order double-free it prevents is a test, never this prose:
`lir::lower::tests::release::store_adopted_member_release_precedes_owner_in_shared_bucket`
(the emit-order pin),
[region-array-push-pair-loop-uaf.lisp](../../../tests/impl/region-array-push-pair-loop-uaf.lisp)
(the guardfree witness), and
`runtime::tests::ownership::region_ownership_pair_pushed_into_let_bound_array_in_loop_reclaims`
(bounded + panic-clean).

**A value read back out of the subtree is an alias the obligation must cover
too.** A native container element READ (`get`/`first`/`rest`) is the
`CallClassification::container_read_funnels` borrow face, minus the moves-out
REMOVEs. It hands the reader a value that still lives *inside* the container: a
member the root's subtree drop reclaims, whose own `decref_point` says nothing
about when the reader is done. The alias is invisible to the member obligation
above. The read's result is a fresh **call-result** placeholder region the walk
cannot statically equate with any member.

Under the per-region RC baseline nothing is owed here. `dispatch_native_call`
takes the Rule 5 pass-through retain, so the reader holds its own counted
reference. But **adoption freezes the member's RC**, which makes that retain
inert. The root's drop reclaims the element under the live reader, and the
reader's own value-resolved release then faults resolving a freed page. So the
walk records `(read site, alias, container)` (`RegionInfo::counted_read_aliases`),
and the forest owes two things:

- **Admission.** A subtree whose member a read alias can name is admitted only
  when the root's `decref_point` post-dominates the **alias's** own release,
  exactly as for a store-adopted member. Otherwise it refuses to Shared, where
  the pass-through retain is live again and the element reclaims on RC. A read
  whose result dies before the container's release keeps its subtree: the bound
  is a bound, not a blanket refusal. Aliasing is **transitive**.
  `(get (get c 0) 0)` names a value inside a member of a member, and the inner
  alias's bound says nothing about the outer one's. So the read edges are closed
  over the member set to a fixpoint before the check.
- **Order.** Post-domination admits the coincident case, where one consumer
  taking both the read's result and the container lands the two releases on one
  node. There the intra-node order decides. The alias's `DecrefValueRegion`
  resolves its runtime region by reading the value's own page, so it must be
  emitted before the release that can tear it. `order_releases` sorts each
  `alias → source` edge alongside the adopt edges ([rules.md](rules.md)).
  It does so over all three alias relations: the read edges and the two result
  relations below. So the ordering composes transitively where a call stands
  between the read and the container that frees the page.

**An opaque call's result is not provably distinct from its arguments.** The
read edges above are recorded where the walk can *see* the container: a native
read whose arg0 is a member's own region. A **call** hides both endpoints. A
callee may hand back an argument itself (`concat` extends a mutable first
argument in place and returns it), or a value it read out of one (`last`).
Either way the caller's call-result placeholder names a region *inside* an
argument's subtree, while relating to no member statically.

Only a declaration that the heap result lives in the call's **own** minted
region rules that out. Those are `Fresh`, `Stores` and `Sends`, whose result
claim the effects oracle checks on every debug run ([effects.md](effects.md)),
plus `Immediate`, which returns no region at all. So the walk records
`(call site, result, argument)` for every other callee
(`RegionInfo::opaque_result_aliases`). The closure above treats such a result as
reachable whenever the argument is.

Two obligations follow from the one edge. The result's **own** release must be
bounded by the root's drop, because the result may *be* a member. Reads out of
the result reach on into the subtree through the read edges, because the result
may be the *container*. An **inlined** callee records nothing here. The walk
re-walks its body with the caller's argument regions bound to the parameters, so
the regions it returns are the real ones, and the read edges name the true
container. This is the conservatism the may-store clique applies to what a
callee *stores*, applied to what it *returns*.

**Reachability is not boundedness, and a `Funnel` needs only the first.** A
`Funnel` declares its result to be arg0 in place, or a fresh copy of arg0
([effects.md](effects.md)). It is the container either way, never an element
interior to it. So its result owes no bound. On the in-place path it resolves
to arg0 and carries arg0's own counted pass-through reference: it *is* the
discarded store result that co-owns a mutable-store subtree's root, above.
Where arg0 is itself an adopted member, the decref lands on the frozen region
and no-ops. Bounding it would refuse every builder over its own trailing store.

What a `Funnel` result must still do is carry reachability, because a read out
of the funnel's result is a read out of arg0. So those edges ride a separate
relation (`RegionInfo::funnel_result_containers`), which the closure propagates
through without recording a release point. The two properties are distinct, so
the closure tracks them in **two** sets. A region a funnel edge reached first
still owes whatever bound a read or opaque-call edge asks of it. The container
READS are excluded from both result relations at the recording site. They
declare `Funnel` too, yet their result is precisely the element the exemption
would be wrong about. The read edge carries them instead, with the tighter
container.

Pinned by `region::infer::tests::borrow::{opaque_call_result_refuses_the_adopt,
read_out_of_an_opaque_call_result_refuses_the_adopt,
read_out_of_a_funnel_result_refuses_the_adopt, container_read_is_not_recorded_as_a_result_alias,
fresh_call_result_records_no_alias}` with their admitting twin, and by the
guardfree witness
[region-call-result-alias-uaf.lisp](../../../tests/impl/region-call-result-alias-uaf.lisp).

An **opcode** read (`%get`/`%first`/`%rest`) is a different problem. It takes no
retain at all, so the borrow has no RC protection with or without adoption.
What covers it is the container's own lifetime as the borrowing node
([rules.md](rules.md)), and no ownership decision is involved. A `%pop`-style
REMOVE is excluded from both faces at the source. It *extracts* the element from
the subtree (`extract_owned_region`), so the element is no longer interior and
the container is not borrowed from.

Pinned by `region::infer::tests::borrow` (the refusal and its admitting twin),
`lir::lower::tests::release::container_read_alias_release_precedes_container_in_shared_bucket`,
and the guardfree witness
[region-container-read-borrow-uaf.lisp](../../../tests/impl/region-container-read-borrow-uaf.lisp).
The sibling face, a read result that *escapes*, is escape's, not this pass's
([escape.md](../escape.md), pinned by
[region-container-read-escape-uaf.lisp](../../../tests/impl/region-container-read-escape-uaf.lisp)).

**A release routed through a cell is a read of the cell.** A compiled capture
cell sits in its binding's own stack slot. A binder whose init is a call result
records that result's region against the slot ([cells.md](cells.md)). The
lowerer then releases the region by value: it loads the slot, finds the cell,
and unwraps it to the content. That release reads the cell's page at the content
region's own `decref_point`, which the binding chain can carry past the
closure's last use. The cell's tight last use does not include it, because the
release is no node of the tree. So a cell adopted into that closure is freed by
the closure's subtree drop before the routed release unwraps it.

The analysis therefore records each such pair, the cell and its routed region
(`RegionInfo::cell_route_regions`). The obligation closes the pair into the
alias set above: the root's drop must post-dominate the routed region's release,
or the subtree refuses to Shared. On that baseline the cell's own release is
clamped after the routed release ([cells.md](cells.md)). The lowerer orders the
same pair at a shared `decref_point`, the routed release ahead of the cell's.
Only a call-result region takes the value route, so a literal init records no
pair. A captured reassigned binding records none, because its binder drops the
init off its own register.

Pinned by
`region::infer::tests::adopt::structural::cell_routed_release_past_the_root_refuses_the_adopt`
and the guardfree witness
[region-capture-cell-adopt-uaf.lisp](../../../tests/impl/region-capture-cell-adopt-uaf.lisp).
The witness is a top-level binding captured by a closure handed to
`sys/spawn-vm`, whose join lands after the closure's last use.

## Why this is hybrid, and where RC remains

Owned subtrees reclaim structurally; **Shared** regions keep the unchanged
per-region RC and cascade. A region is Shared whenever the compiler cannot bound
its frontier. That is a value genuinely escaping its fiber with no common
dominating activation, or a may-store the solver could not resolve. The split is
the endpoint, not a transition: Project Verona keeps the same hybrid (Owned
`iso`/`mut` subtrees, RC'd `imm`).

Adoption runs on every compile. A shape no cut admits keeps every region
`Counted` with empty `owned_children`, and every path above is inert. That is
the per-region-RC baseline, always legal and always available as the fallback.
