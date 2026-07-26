# Ownership adopts and the root's lifetime obligation

The interior owner-edges that build an [ownership forest](ownership.md)
subtree where no single store site names them: the **capture adopt** (a
closure ⊇ its captures), the **funnel adopt** (the opaque-store face of a
container ⊇ its member), the post-dominance + emit-order
**obligation** the root's single demise must satisfy for a subtree drop to be
sound, and where the hybrid still keeps per-region RC.

## The capture adopt — emitted at the closure, for every capture kind

A capture containment edge (`closure ⊇ captured`) has no store site — capture records no
`cross_region_refs` edge (the RC double-count fix: the runtime auto-incref over the
`Closure` env stands in for a static `IncrefRegion`) — so its adopt is keyed by the
closure's **construction**: at `MakeClosure`, `lower_lambda_expr` reloads each adopted
captured value and emits a value-resolved adopt in place of the capture's baseline
`IncrefRegion`, and `analyze_regions_with` suppresses the member's own compiler decref (the
closure's subtree drop is its sole reclamation). How the edge points depends on how the
captured binding is materialized (`ownership::capture`):

- **By-value capture** — an immutable, non-prebound local the closure holds directly: the
  edge is `closure ⊇ content`, adopted with `AdoptRegion(closure, content)`. The reload
  covers a direct local (from its binding slot, `LoadLocal`) and a by-value
  upvalue/transitive capture (from the constructing function's environment, `LoadCapture`).
- **Immutable letrec forward-reference cell** — a prebound `MakeCaptureCell` a sibling
  references before its initializer runs. The closure holds the CELL, not the content, so
  the edge is re-pointed at the **cell region** (`single_cell_region_of`) → `closure ⊇ cell`.
  Paired with the walk's `cell ⊇ content` edge (`record_cell_content_edges`, keyed at the
  cell's mint scope), external uniqueness sees the true chain `closure ⊇ cell ⊇ content`, so a
  **local, non-escaping `{closure, cell, content}` clique** is externally unique → Owned →
  reclaimed as a unit by the closure's subtree drop, the interior cell↔closure reference
  included. The cell store is UNCOUNTED, so this re-pointed edge is the ONLY way the scan sees
  the cell is held; the runtime realizes it with **`AdoptCellRegion`**, which resolves BOTH
  operands with `region_of` (never `result_region_of`, which would unwrap the cell to its
  content) so a cell's OWN region is named. A second `AdoptCellRegion` at the cell store links
  the content into the cell (`cell ⊇ content`, `maybe_emit_cell_content_adopt`) where the
  lifetime obligation admits; elsewhere the content reclaims by the cell's free-time RC
  cascade. Pinned by `regions::tests::cells::{walk_records_cell_contains_content_for_compiled_letrec_cell,
  capture_edge_points_at_cell_region_not_content}`,
  `regions::tests::adopt::owned_subtrees_admits_local_capture_cell_clique` (with its escaping /
  two-sibling refusals), and the emit by
  `lir::lower::tests::preallocated_capture_cells_get_distinct_regions_each_released`.
- **Re-storable cell** (`is_restorable_capture_cell` — an `@`-mutable captured local or a
  mutated captured parameter): a **borrow**, no owner edge. Its content lifetime is
  per-rebind — the rebind funnel decrefs each displaced prior — and SHORTER than the cell's,
  whose release is hoisted once past enclosing loops; adopting the content into the cell's
  subtree would free a displaced prior under the live cell (the loop over-free). So
  `capture_containment_edges` skips the capture and `compute_adopt_edges`'s `adoptable_cell`
  refuses its `cell ⊇ content` edge — which the walk still records (the cell holds *a*
  content) for external-uniqueness counting — and the content reclaims on the per-region-RC
  baseline. Pinned by `region_capture_cell_loop_uaf_ownership` (the guardfree witness) and
  `regions::tests::adopt::{capture_edge_skips_restorable_cell_admits_immutable_in_one_clique,
  restorable_compiled_cell_records_content_edge_but_is_not_adopted}`.

A mutually-recursive `letrec` closure **cycle** (each closure holds the other's forward
cell) is a *cyclic* clique, not a rooted subtree, so it is reclaimed by the closure-cycle
**MERGE** ([letrec.md](letrec.md) § "The letrec closure-cycle merge"), which collapses the SCC ∪ its cells onto one
arena before this pass — never by this capture-adopt path (`recur-local-mutual` /
`recur-local-self` read closed in `oracle.lisp`). The capture-adopt path serves the *acyclic*
rooted clique above.

The capture-adopt contract — every suppressed member is adopted — is discharged by **emit
capability**, not by refusing shapes the emit cannot reach: `lower_lambda_expr`'s reload
covers every capture kind, and the `debug_assert` at the emit is the backstop that every
adopt edge matches a real capture of the constructed closure. Pinned by
`lir::lower::tests::capture_adopt_reloads_upvalue_via_load_capture` (the env-reloaded
emission) and `regions::tests::adopt::capture_adopt_edges_are_emittable`.

What bounds the *admission* of a capture owner-edge is the general subtree filters — no
lowerability filter exists — and the **cross-activation (upvalue) owner-edge is refused at the
lifetime obligation**. With the capture edge re-pointed through the cell the containment is
now VISIBLE, so a subtree over an upvalue member DOES form: external uniqueness admits
`{nested, cell, enclosing, member}`, the member being captured by BOTH the nested closure and
the enclosing forwarding lambda. The refusal then holds structurally at the lifetime
obligation — the nested closure's region is minted per CALL of the enclosing closure, so
claiming a member that survives across calls would free it under the enclosing environment's
still-live reference and re-adopt an already-owned region on the next call. The forwarding
capture resolves the member's tight last-use to a position at/past the enclosing lambda's
node, after the nested root's in-body drop in post-order, so the obligation cannot prove the
member dies before the root and refuses. A member reachable through an upvalue capture is
therefore Owned only by an owner that outlives **every** capturer — the activation/fiber owner
node of the owner = activation cut — never by a region root; until that owner exists the shape
stays Shared (the always-legal baseline). Pinned by
`regions::tests::adopt::owned_subtree_upvalue_capture_owner_refused_on_lifetime` and
`closure_web_capture_not_yet_claimed`, and at runtime by
`runtime::tests::ownership::upvalue_capture_family_runs_sound`.

## The funnel adopt — the store face

A compiled mutable store — every storing/removing/copying `%`-op lowers as a
native funnel `Call` (`IntrinsicOp::routes_native_funnel()`) — is an opaque
`Funnel` native call that records **no** `cross_region_refs` edge (the runtime funnel
counts the store; a compile-time edge would double-count — effects.md § `Funnel`),
so the store-keyed adopt would find no emittable interior edge and every funnel-built
subtree would refuse to Shared. The containment is recovered instead from the container
argument's `RetType` (`RegionInfo::containment_edges`, recorded **site-keyed** —
`(funnel call site, contained, container)` — by the walk's `Funnel` arm), and
`compute_adopt_edges` admits those edges as a third interior owner-edge kind beside
stores and captures: the adopt rides the same `owned_adopt_edges` map, keyed at the
**funnel call site**, and `emit_increfs_for` emits the same value-resolved
`AdoptRegion(container, member)` there — both endpoints reload from their binding slots,
so no store opcode is needed. This is the same funnel face the activation and transfer
cuts carry (their F-b admission); funnel-recovered `containment_edges` give the forest
the same containment facts a constructor-embedding store declares.

The RC composition needs no explicit balancing: by the time the adopt executes, the
funnel's runtime store-incref has already counted the member, and `adopt_region` moves it
`Counted → Owned` **consuming the whole count**; the member's own later
`DecrefValueRegion` (sequenced before the root's drop by the lifetime obligation) and the
container's free-time cascade decref both land on the frozen `Owned` mode and no-op
structurally — the root's subtree drop is the member's sole demise. A funnel-adopted
member is a **store**-adopted member for the lifetime obligation (its own decref stays
live, so it is bounded by its structural `decref_point`, `EmitMode::Adopt`, loop clause
applied), and the emit site — the funnel call that stored the member into its owner —
structurally follows both allocations, so both slots are populated when the adopt runs. A
containment edge whose member the adopt cannot key (no owner edge at any site) still
refuses the subtree to Shared, the always-legal baseline.

What the funnel face does **not** cover: the builder-idiom MERGE, which is the
constructor emit's mechanism, not the store's. `%pair` lowers as the inline
`Intrinsic` opcode whose `emit_alloc` seeds the `merged_slots` mint-or-reuse
(merging.md § Merging), so the merge rides the `%pair` emit on every compile where a
builder idiom appears — no funnel edge is involved. Pinned by
`regions::tests::adopt::adopt_edges_claims_interior_cycle_member_by_root` (the
funnel-site adopt edges), `…::adopt_edges_refuses_loop_enclosed_member` (the
obligation holds on the funnel face), and at runtime by
`runtime::tests::ownership::region_ownership_reclaims_interior_cycle_subtree`,
`…_reclaims_nested_cycle_subtree`, and `…_reclaims_bare_cycle_group` (bounded flag-on
beside the leaking flag-off counterfactual).

## The fiber member — refused at the class level

The forest is **rooted at fibers**: a fiber (through its activation and fiber owner
nodes — [owner.md](owner.md)) may own regions, but the region a `Fiber` object lives
in is never a *member* of a region-rooted subtree, a co-owned group, an activation
SCC, or a transfer summary. Every other value enters fiber-visible state through a
compile-time-visible escape facet (yield / emit / send / return), so the lifetime
obligation can bound its borrows; a fiber **value** acquires aliases by merely
*running* — the scheduler's parent/child chain, the restarts system, and the
`fiber/child` / `fiber/parent` graph reads all create references at runtime, over
edges no structural post-dominance predicate can see. Adoption freezes a region's
RC, so the pass-through retain such a read hands the caller cannot pin an adopted
fiber's region — which is why the class is refused at admission rather than
compensated at any read site.

The refusal is one class shared by every cut: the region walk records the result
region of each native call whose declared
[`RetType`](../../../src/primitives/def.rs) is `Fiber`
(`RegionInfo::fiber_result_regions` — `fiber/new` is the mint; a fiber reached
through any non-primitive route is a non-`Fresh` call result, already refused as a
runtime placeholder), and `ownership::inputs::not_ownable` counts the class among
the dynamic-lifetime refusals, so any candidate component containing a fiber's
region stays Shared (the always-legal baseline). A refused fiber region reclaims on
ordinary RC — its holders' counted references (bindings, container stores, the
embedding closure/fiber alloc-scan) release as they die, and a parked fiber's chain
state discharges on the free path (owner.md § "The free-path fiber discharge"). The
runtime backstop is a debug assert at `RegionStore::adopt_region`: an adopted
region's pool holds no live `Fiber` object. Pinned by
`regions::tests::adopt::owned_subtree_refuses_fiber_member` (beside its admitting
`@array` twin) and the guardfree fixture pin `region_fiber_exhume_uaf`
(`tests/integration/fixtures/region-fiber-exhume-uaf.lisp`); `tests/elle/fibers.lisp` (the propagate
child-chain reads) and `tests/elle/grpc.lisp` exercise the class under the full
scheduler.

## The lifetime obligation the root carries

Subtree drop fires at the **root's** single `DecrefRegion`, at the root's
`decref_point`. For that to be sound the root's demise must **post-dominate every
member's last use** — a child read after the root's `decref_point` would be freed
out from under the read. Adoption and merging discharge this through **one** structural
post-dominance predicate (`regions::postdom::drop_post_dominates`, decided over the scope
tree's post-order subtree intervals, *not* by `compute_order` magnitude):
`compute_adopt_edges` **refuses** a component with an un-post-dominated member (it stays
Shared, the always-legal baseline) and gate 6 refuses such a merge (merging.md § Merging,
condition 6). The two differ only by `EmitMode` — ADOPT's store-member keeps its own
decref, so a loop enclosing the root's free is the cross-iteration UAF the predicate
refuses; MERGE's child is reachable solely through the parent (conditions 1+4), so the
loop clause is waived. The external-uniqueness walk
(`regions::ownership::compute_owned_subtrees`) proves the *frontier* is unique but
does not by itself order lifetimes, so this obligation is the emit's, not the
walk's. (The pinning test is the e2e reclamation tier; an interior-outlives-root
shape must stay Shared, never adopt.)

**Post-domination is necessary but not sufficient — the emit order carries the
rest.** Node-granularity post-dominance admits a member whose `decref_point` is the
**same node** as the root's demise (the straight-line coincident case — a fresh
container built and consumed in one expression). At that shared node the *intra-node
emission order* then decides soundness, because a store-adopted member keeps its own
`DecrefRegion`, whose no-op depends on the member still being **`Owned`**: it must be
emitted **before** every release that can free the member's owner at that node. Two
facts make "before" non-trivial for a container root:

- The root of a mutable-store subtree is typically a `Fresh` **call-result** region
  freed value-based, and it carries **more than one** runtime reference — the
  holder-binding release *and* the **discarded pass-through result** of the store
  itself (`%array-push`/`%put` return their container, so the store's own result is a
  second call-result region that resolves to the root at runtime and, when the result
  is discarded, releases the root). Whichever release zeroes the root triggers the
  subtree drop; the obligation's single `region_data[root].decref_point` names only
  one of them.
- A store-adopted member's own `DecrefRegion` is a structural no-op **only while the
  member is `Owned`**; once the subtree drop has reclaimed it, that slot-resolved
  decref faults (`regionstore/refcount.rs`, the phantom/double-free assert).

So the emit orders every adopted member's release **before its owner's** at each shared
`decref_point`, by topologically sorting the adopt edges (`owned_adopt_edges` ∪
`capture_adopt_edges`, member → owner) in `with_region_info`. The forest gives each
member exactly one owner, so the graph is acyclic and the order always exists — and it
orders *nested* subtrees innermost-first (member ⊂ mid ⊂ root), which a flat
members-first bucket class could not. A member's release reads and frees nothing while
`Owned`, so it no-ops; whichever root-freeing release fires afterward subtree-drops the
member exactly once. Regions no adopt edge relates fall back to the page-read-depth
tie-break (rules.md Rule 4).
The invariant this restores is stated positively in ownership.md § "The runtime: a reclamation
typestate and `owned_children`": a store-adopted member's decref hits the still-frozen
`Owned` region — a no-op — because it is emitted before the root's drop. The reference
for the inverted-order double-free it prevents is a test, never this prose:
`lir::lower::tests::release::store_adopted_member_release_precedes_owner_in_shared_bucket`
(the emit-order pin), `region_array_push_pair_loop_uaf` (the guardfree witness), and
`runtime::tests::ownership::region_ownership_pair_pushed_into_let_bound_array_in_loop_reclaims`
(bounded + panic-clean).

**A value read back out of the subtree is an alias the obligation must cover too.** A
native container element READ (`get`/`first`/`rest` — the
`CallClassification::container_read_funnels` borrow face, minus the moves-out REMOVEs)
hands the reader a value that still lives *inside* the container: a member the root's
subtree drop reclaims, whose own `decref_point` says nothing about when the reader is
done. The alias is invisible to the member obligation above, because the read's result is
a fresh **call-result** placeholder region the walk cannot statically equate with any
member. Under the per-region RC baseline nothing is owed here — `dispatch_native_call`
takes the Rule 5 pass-through retain, so the reader holds its own counted reference — but
**adoption freezes the member's RC**, which makes that retain inert: the root's drop
reclaims the element under the live reader, and the reader's own value-resolved release
then faults resolving a freed page. So the walk records `(read site, alias, container)`
(`RegionInfo::counted_read_aliases`) and the forest owes two things:

- **Admission.** A subtree whose member a read alias can name is admitted only when the
  root's `decref_point` post-dominates the **alias's** own release, exactly as for a
  store-adopted member. Otherwise it refuses to Shared — where the pass-through retain is
  live again, and the element reclaims on RC. A read whose result dies before the
  container's release keeps its subtree (the bound is a bound, not a blanket refusal).
  Aliasing is **transitive** — `(get (get c 0) 0)` names a value inside a member of a
  member, and the inner alias's bound says nothing about the outer one's — so the read
  edges are closed over the member set to a fixpoint before the check.
- **Order.** Post-domination admits the coincident case — one consumer taking both the
  read's result and the container lands the two releases on one node — where the intra-node
  order decides: the alias's `DecrefValueRegion` resolves its runtime region by reading the
  value's own page, so it must be emitted before the release that can tear it.
  `order_releases` sorts each read's `alias → container` edge alongside the adopt edges
  (rules.md Rule 4).

An **opcode** read (`%get`/`%first`/`%rest`) is a different problem, not this one: it takes
no retain at all, so the borrow has no RC protection with or without adoption, and what
covers it is the container's own lifetime (rules.md Rule 4, the borrowing node) — no
ownership decision is involved. A `%pop`-style REMOVE is excluded from both faces at the
source: it *extracts* the element from the subtree (`extract_owned_region`), so the element
is no longer interior and the container is not borrowed from. Pinned by
`regions::tests::borrow` (the refusal and its admitting twin),
`lir::lower::tests::release::container_read_alias_release_precedes_container_in_shared_bucket`,
and the guardfree witness `region_container_read_borrow_uaf`; the sibling face — a read
result that *escapes* — is escape's, not this pass's (docs/impl/escape.md, pinned by
`region_container_read_escape_uaf`).

## Why this is hybrid, and where RC remains

Owned subtrees reclaim structurally; **Shared** regions keep the unchanged
per-region RC and cascade. A region is Shared whenever the compiler cannot bound
its frontier — a value genuinely escaping its fiber with no common dominating
activation, or a may-store the solver could not resolve. The split is the endpoint,
not a transition: the same hybrid Project Verona keeps (Owned `iso`/`mut` subtrees,
RC'd `imm`). Adoption runs on every compile; a shape no cut admits keeps every
region `Counted` with empty `owned_children` and every path above is inert — the
per-region-RC baseline, always legal and always available as the fallback.
