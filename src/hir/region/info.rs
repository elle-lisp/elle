//! `RegionInfo`: region inference's output for a compilation unit — the
//! per-allocation and per-scope region assignments and the ownership-forest
//! cuts the lowerer consults, plus the queries over them.

use crate::hir::binding::Binding;
use crate::hir::expr::HirId;

use super::{Region, RegionData, RegionStats};

use rustc_hash::FxHashSet;
use std::collections::HashMap;

/// A fn-local reassigned mutable that took the 1-slot-container gate
/// (docs/impl/region/bindings.md § "Reassigned mutable bindings are 1-slot
/// containers"). The cell holds exactly ONE counted reference to its current
/// content, and that reference needs both of its release channels named:
/// drop-on-overwrite at each `store` for the displaced prior, and the content
/// drop at `demise` for the final, never-overwritten value. The producer's
/// separate claim on each stored value dies at the store, which is why
/// `value_regions` are pinned to the store sites instead of riding the cell
/// binding's uses (a cell binding names the slot, not any one value).
pub struct CellContainer {
    /// The `Assign`/`SetCell` sites that store into the cell.
    pub stores: Vec<HirId>,
    /// The regions of the values stored there.
    pub value_regions: Vec<Region>,
    /// The node whose exit is the cell's scope demise — the enclosing scope
    /// node, so a loop-carried cell drops once after the loop and a cell bound
    /// inside a loop body drops once per iteration.
    pub demise: HirId,
}

impl CellContainer {
    pub fn new(stores: Vec<HirId>, value_regions: Vec<Region>, demise: HirId) -> Self {
        Self {
            stores,
            value_regions,
            demise,
        }
    }
}

/// What a frame-replacing tail call's own callee settles about the RC traffic the
/// lowerer emits around it. Both fields are claims about *this* callee, which is
/// why they are recorded per call rather than per region or per function.
#[derive(Debug)]
pub struct TailCalleeFacts {
    /// Regions the callee holds through its **captured environment** — the
    /// allocation funnel's counted (or, under the forest, owning) edge, taken when
    /// the closure was built and dropped only by the closure region's free-time
    /// cascade at the callee's completion. It therefore spans the gap between a
    /// release relocated ahead of the `TailCall` and the callee's return mint,
    /// which is what admits a region escaping by the return facet alone
    /// (docs/impl/region/mechanism.md § "The callee's return mint, and the edge
    /// that funds the gap"). A sibling arm's callee, capturing nothing, funds
    /// nothing — hence per call.
    pub capture_funded: rustc_hash::FxHashSet<Region>,
    /// How many arguments this callee turns into **owned parameters**, each of
    /// which releases once. Arguments past this index are collected into the rest
    /// parameter's fresh list, whose own allocation scan and surplus release
    /// balance them without help.
    ///
    /// The tail-call move hands over one reference per REGION, so a repeated
    /// argument needs a mint for every occurrence after the first — but only where
    /// the occurrence lands in a fixed parameter (rules.md Rule 5). Minting into a
    /// rest position instead strands one region per call.
    pub fixed_params: usize,
}

/// Results of region inference for a compilation unit.
///
/// Every allocation site has a solved region in `alloc_region`.
/// Every scope (Let, Letrec, Block, Loop, Lambda) has a region in
/// `scope_region`. A scope is reclaimable when its region appears
/// in `live_regions` (at least one allocation's death point is
/// that scope's exit).
pub struct RegionInfo {
    /// HirId → solved region for each allocation site.
    pub alloc_region: HashMap<HirId, Region>,
    /// HirId → region introduced by each scope node.
    pub scope_region: HashMap<HirId, Region>,
    /// Binding → region where the binding lives.
    pub binding_region: HashMap<Binding, Region>,
    /// Binding → source regions the binding's value may point into.
    ///
    /// Populated from the inference's `binding_regions` after the walk;
    /// used by the `decref_point` extension pass so a binding's last use
    /// keeps its source regions alive. Exposed publicly because tests
    /// pin the invariant that a side-effecting destructure inside a
    /// letrec init does not get its `binding_regions[b]` overwritten by
    /// a subsequent placeholder init (e.g. `r = nil`) for the same
    /// binding — without this, the source region's `decref_point` is left
    /// at the destructure's id and a stale ptr survives to a later
    /// use of `r`, panicking in arena::deref.
    pub binding_source_regions: HashMap<Binding, Vec<Region>>,
    /// Top-level captured (`needs_capture`, outside a lambda) bindings that are
    /// reassigned — `@x` boxed in a compiled `MakeCaptureCell` AND later
    /// reassigned. For these the lowerer must drop the init value's alloc
    /// reference at the define off its own register, and must NOT route that
    /// decref through the cell slot (a reassignment repoints the cell, so a
    /// slot-load + `result_region_of` unwrap frees a different, live value —
    /// region-capture-cell-reassign-uaf.lisp). A captured binding that is
    /// NEVER reassigned is absent here and keeps the ordinary
    /// routed-through-cell release (the cell content is stable, so the unwrap
    /// always names the right value).
    pub captured_reassigned_bindings: FxHashSet<Binding>,
    /// Regions that have at least one allocation assigned to them.
    pub live_regions: FxHashSet<Region>,
    /// Cross-region references detected by the solver.
    /// Each entry is (store_site, source_region, target_region) where a
    /// value in source_region is stored into a structure in target_region.
    /// The lowerer emits IncrefRegion(source) at the store site; the
    /// runtime cascade at `DecrefRegion(target)` releases the source
    /// reference when target's RC hits 0.
    pub cross_region_refs: Vec<(HirId, Region, Region)>,
    /// Per-region metadata (decref_point HirId, etc.).
    ///
    /// Populated by region inference.
    pub region_data: HashMap<Region, RegionData>,
    /// Per-region **tight** last-use, resolved through the region's holder
    /// bindings' uses — the binding-chain `max_use` of `analyze_regions_with`,
    /// folding `capture_loop_ext`, recorded per region (max-by-order across a
    /// region's holder bindings). For a region a binding holds this is the
    /// binding's true last-use over ALL its uses; unlike
    /// `region_data[r].decref_point` it is **not** also max'd with the structural
    /// alloc-site last-use, so it EXCLUDES the over-estimate the grow-only
    /// last-use fixpoint can leave locked one step past a captured value's owning
    /// closure (the lambda-as-`let`-init position, ordered after the closure's
    /// last call).
    ///
    /// Read by the ownership lifetime obligation
    /// (`regions::ownership::compute_adopt_edges`) for a **captured** member: a value
    /// reachable only through a local closure is then admitted as Owned (freed at the
    /// closure's true death by subtree drop) rather than refused for a phantom
    /// over-extension. A value also used *after* its closure keeps a later direct use
    /// here, so the obligation still correctly refuses it.
    pub binding_last_use: HashMap<Region, HirId>,
    /// Set of regions created by `alloc_here` at a Call HirId. Their
    /// compile-time region ID is just a placeholder for the runtime
    /// region of the callee's returned value (which the caller cannot
    /// statically name). The lowerer emits `DecrefValueRegion(reg)`
    /// at these regions' `decref_point` instead of `DecrefRegion(rid)`,
    /// reading the runtime region from the value at last use.
    pub call_result_regions: FxHashSet<Region>,
    /// HirIds of a binding init that is a WHOLE-VALUE read of a reassigned
    /// captured cell (`is_restorable_capture_cell`) — the reader of the 1-slot
    /// container. The cell's overwrite (`capture_store_with_rebind`) decrefs the
    /// displaced prior unconditionally, so a reader that merely aliases the
    /// cell's value is freed under it by the next overwrite (the captured-alias
    /// use-after-free; docs/impl/region/bindings.md § "Captured reassigned
    /// cells"). The read is treated as Rule 5's "new reference" pass-through: the
    /// lowerer emits an `IncrefValueRegion` here so the reader holds a counted
    /// reference of its own, and the read's placeholder region (minted at this
    /// HirId, so it lands in `call_result_regions`) carries the balancing
    /// `DecrefValueRegion` at the reader's last use. Element reads
    /// (`first`/`get`/destructuring) are excluded — an element is independently
    /// counted by its parent's alloc-scan, so it cascades rather than freeing
    /// under the reader. Pinned by
    /// tests/elle/region-reassign-captured-cell-reader.lisp.
    pub counted_cell_read_sites: FxHashSet<HirId>,
    /// The subset of `call_result_regions` whose callee declares
    /// [`RegionEffect::Fresh`](crate::primitives::def::RegionEffect::Fresh): the
    /// result is freshly allocated in the call's own region, so it is genuinely
    /// caller-owned (region/effects.md § `Fresh`). A `call_result` region is
    /// ordinarily refused by the ownership inference as a runtime placeholder
    /// (a possible borrow / opaque result), but a `Fresh` one is a legitimate
    /// Owned candidate — `regions::ownership` admits it, which is what makes a
    /// mutable container or aggregate built by a native call (the `@array`/
    /// `array`/`@struct` constructors, opaque to the walk) ownable. Baseline release
    /// is unchanged — every `call_result` (Fresh or not) still frees by value
    /// (`DecrefValueRegion`); this set only widens the ownership candidacy.
    pub fresh_result_regions: FxHashSet<Region>,
    /// Call-result regions whose callee declares
    /// [`RetType::Fiber`](crate::primitives::def::RetType::Fiber) (`fiber/new`) —
    /// regions holding a `Fiber` object. A fiber acquires aliases by merely
    /// running (the scheduler's parent/child chain, the `fiber/child`/
    /// `fiber/parent` graph reads), so no structural obligation can bound its
    /// borrows and adoption's frozen RC would leave every such read's
    /// pass-through retain inert; the region is therefore never a member of any
    /// region-rooted ownership cut (`ownership::inputs::not_ownable` counts this
    /// class among the dynamic-lifetime refusals) and reclaims on the RC
    /// baseline (docs/impl/region/adopt.md § "The fiber member — refused at the
    /// class level"). A fiber reached through a non-primitive route is a
    /// non-`Fresh` call result, refused already; this set closes the `Fresh`
    /// mint route. Baseline release is unchanged.
    pub fiber_result_regions: FxHashSet<Region>,
    /// Structural containment edges `(funnel_call_site, contained_region,
    /// container_region)` recovered for the ownership inference from a `Funnel`
    /// store (`%array-push`/`%put`) into a **mutable retaining** container
    /// (`RetType::MutableArray`/`MutableStruct`): the stored value's region is
    /// retained by the container's, so `container ⊇ contained` for subtree
    /// membership. The storing ops lower as opaque `Funnel` native calls that
    /// record NO `cross_region_refs` edge
    /// (the funnel counts the store at runtime — a compile-time edge would
    /// double-count; region/effects.md § `Funnel`). This set re-supplies that
    /// containment for the forest **without** any `IncrefRegion` — it is read
    /// only by `regions::ownership`, never by the lowerer's incref/decref
    /// emission of RC, so the baseline stream is unchanged. The **site** is the
    /// funnel `Call` node, carried exactly as `cross_region_refs` carries its
    /// store site, so an adopt for a funnel-contained member can be keyed at the
    /// funnel call (the funnel store face — region/adopt.md § "The funnel
    /// adopt"; the emit is value-resolved, needing no store opcode).
    /// `@string`/`@bytes` containers (which copy bytes, retaining no region) are
    /// excluded by their non-container `RetType`. This vector ALSO carries a `Fresh`
    /// native's **embed** edges `(embed_call_site, embedded_arg, result)`: a native
    /// whose fresh result references an argument declares which via
    /// [`PrimitiveDef::embeds`](crate::primitives::def::PrimitiveDef::embeds), and the
    /// walk's `Fresh` arm (`call_embeds`) records the same `result ⊇ arg` containment
    /// — the compile-time analog of the runtime alloc-scan (`find_object_cross_refs`)
    /// that counts the embedding. `Fresh` alone is too vague to carry it (`popn` is
    /// `Fresh` yet embeds none of its args). `with-traits` declares its `traits`
    /// side-field embed (`&[1]`), so a value it embeds into an escaping result is seen
    /// referenced from outside the capturing closure's subtree and stays Shared. The
    /// `pair`/`array` CONSTRUCTORS do not declare it: their embedding is
    /// the `cross_region_refs` edge recorded by the intrinsic walk arm.
    pub containment_edges: Vec<(HirId, Region, Region)>,
    /// Funnel-store call site HirId → the regions of the heap values stored there
    /// (the non-container args of a `Funnel` intrinsic). The runtime mutable-store
    /// funnel increfs each stored value, so a per-arm decref placed at such a site
    /// (`regions::branch_arm_decrefs`) is guaranteed to leave the value's RC ≥ 1
    /// (the container's reference) — it releases only the value's own owning
    /// reference and can never over-free. Recorded even when the container's type
    /// is statically unknown (a parameter container — the `put`/`set` dispatch),
    /// where `containment_edges` records nothing.
    pub funnel_store_sites: HashMap<HirId, Vec<Region>>,
    /// Byte-copy funnel call site HirId → the region(s) of the pushed VALUE
    /// (`%string-push`/`%string-push-mut`/`%bytes-push` — a `Funnel` that copies the
    /// value's bytes into the container instead of retaining its region). A dispatch
    /// wrapper stores the value through such a funnel in ONE arm while its `val` param
    /// is used across arms, so `val`'s owned reference strands on the sibling arms — the
    /// byte-copy dual of the retaining `funnel_store_sites` strand. `regions::compensate`
    /// places a per-arm release from here. Sound BECAUSE the byte-copy touched neither
    /// the value's incref nor its decref: the per-arm release is the value's true
    /// last-use release, NOT a redundant strand (as for a retaining store, whose
    /// container keeps the value alive) and NOT a double-free (as `%del` would be — it
    /// decrefs the value in-body, so it is deliberately EXCLUDED from this set).
    pub funnel_bytecopy_value_sites: HashMap<HirId, Vec<Region>>,
    /// Pass-through funnel-store call site HirId → the region(s) of the CONTAINER
    /// argument (arg0) stored into there, recorded only for a `-mut` store whose
    /// declared return is a mutable container (`MutableStruct`/`MutableArray`/
    /// `MutableSet` — the funnel returns arg0 in place, not a fresh copy). A
    /// polymorphic dispatch wrapper's mutable arm tail-calls such a funnel and
    /// returns its container pass-through, so the container is return-escaping and
    /// the wrapper never releases the owning reference it holds as an owned param —
    /// yet the funnel's `pass_through_retain` leaves the returned value's RC ≥ 1.
    /// A per-arm decref placed at such a site (`regions::compensate`) therefore
    /// releases only that stranded owned-param reference and can never drop the
    /// live returned container to zero. Recorded even for a parameter container
    /// (the `put`/`push`/`add` dispatch), where `containment_edges` records nothing.
    pub funnel_container_sites: HashMap<HirId, Vec<Region>>,
    /// The `-mut` PASS-THROUGH subset of `funnel_container_sites` — sites whose funnel
    /// returns arg0 (the container) IN PLACE (`%put-*-mut`/`%add-set-mut`/
    /// `%push-array-mut`/`%del-*-mut`, a mutable-container RetType). Here the result IS
    /// the container the caller passed in, so the caller already owns a reference to it
    /// and the tail ReturnValue retain is redundant — the compensation gates its
    /// `container_release_sites` (the lowerer's suppression trigger) on this. An
    /// IMMUTABLE funnel is ABSENT: its FRESH result's ReturnValue retain is the
    /// caller's move/reassign reference, so suppressing it over-frees a result stored
    /// into a reassigned slot (the container's own owned-param leak still closes via
    /// `funnel_container_sites`; only the redundant-retain drop is withheld).
    pub funnel_passthrough_sites: HashMap<HirId, Vec<Region>>,
    /// **Uncounted** container element-READ site HirId → the regions of the CONTAINER
    /// read from (arg 0). These are the inline-opcode reads `%get`/`%first`/`%rest`: the
    /// value handed back still lives inside the container — a pair's car in the pair's
    /// own region, an `@array` element in a member region the container holds — and the
    /// opcode raises NO reference count on it (unlike a native read, whose dispatch takes
    /// the Rule 5 pass-through retain). So the container's own lifetime is the only thing
    /// keeping the borrow alive, and the container is in use for as long as the read's
    /// RESULT is: `analyze::decref` extends each of these regions' `decref_point` to the
    /// read's last use — the *borrowing node* of region/rules.md Rule 4. Anchored at the
    /// read instead, the container's free-time cascade drops the element's last count and
    /// the reader derefs a freed page. Pinned by
    /// `regions::tests::borrow::opcode_read_extends_container_decref_to_the_reader` and
    /// `region_container_read_borrow_uaf`.
    pub uncounted_read_sites: HashMap<HirId, Vec<Region>>,
    /// **Counted** container element-READ edges `(read_call_site, alias_region,
    /// container_region)` — a native `get`/`first`/`rest` call
    /// (`CallClassification::container_read_funnels` minus the moves-out REMOVEs, which
    /// extract their element rather than borrowing it). `dispatch_native_call` takes the
    /// Rule 5 pass-through retain, so the RC baseline already keeps the element alive
    /// across the reader and the container needs no lifetime extension — but **adoption
    /// freezes the member's RC**, leaving that retain inert. Two consumers keep the forest
    /// honest about it (docs/impl/region/adopt.md § "The lifetime obligation the root
    /// carries"): `compute_adopt_edges` refuses a subtree whose root's drop does not
    /// post-dominate the alias's own release (the alias may name any frozen member), and
    /// the lowerer's `order_releases` sorts the alias before its container where the two
    /// releases share a `decref_point` — the alias's `DecrefValueRegion` resolves its
    /// region by reading the value's own page, which the container's release can tear.
    /// Pinned by `regions::tests::borrow` and `region_container_read_borrow_uaf`.
    pub counted_read_aliases: Vec<(HirId, Region, Region)>,
    /// Call-result alias edges `(call_site, result_region, argument_region)` — the
    /// result side's analogue of the may-store arg clique. A callee may hand back an
    /// argument itself (`concat` extends a mutable first argument in place and returns
    /// it) or a value it read out of one (`last`), and either way the caller's
    /// call-result placeholder names a region *inside* that argument's subtree while
    /// relating to no member statically. Only a declaration that the heap result lives
    /// in the call's OWN minted region rules that out —
    /// [`Fresh`](crate::primitives::def::RegionEffect::Fresh),
    /// [`Stores`](crate::primitives::def::RegionEffect::Stores) and
    /// [`Sends`](crate::primitives::def::RegionEffect::Sends), whose result claim the
    /// declaration oracle checks on every debug run (region/effects.md) — plus
    /// `Immediate`, which returns no region at all. Every other callee, a non-primitive
    /// one included, records an edge per heap argument.
    ///
    /// `compute_adopt_edges` closes these edges together with `counted_read_aliases`
    /// over a subtree's member set: a result reachable from a member must itself be
    /// bounded by the root's drop (it may BE a member), and a read out of it reaches on
    /// into the subtree (it may be the CONTAINER). An INLINED callee records nothing —
    /// the walk re-walks its body with the caller's argument regions bound to the
    /// parameters, so the regions it returns are the real ones. Read by the ownership
    /// inference and by the lowerer's release order, never by its incref/decref emission;
    /// the baseline RC stream is unchanged. Pinned by `regions::tests::borrow` and
    /// `region_call_result_alias_uaf`.
    pub opaque_result_aliases: Vec<(HirId, Region, Region)>,
    /// Funnel-result identity edges `(funnel_call_site, result_region,
    /// container_region)` — the same relation as `opaque_result_aliases` minus its
    /// bound. A [`Funnel`](crate::primitives::def::RegionEffect::Funnel) declares that
    /// its result is arg0 in place or a fresh copy of arg0 (region/effects.md
    /// § `Funnel`): the CONTAINER either way, never an element interior to it. So the
    /// result needs no lifetime bound of its own — on the in-place path it resolves to
    /// arg0 and carries arg0's own counted pass-through reference (the discarded store
    /// result that co-owns a mutable-store subtree's root), and where arg0 is itself an
    /// adopted member the decref lands on the frozen region and no-ops, which the emit
    /// order already guarantees (region/adopt.md § "The lifetime obligation the root
    /// carries"). What it does carry is REACHABILITY: a read out of the funnel's result
    /// is a read out of arg0, so `compute_adopt_edges` propagates through these edges
    /// while bounding only what the read and opaque-call edges reach — the two are
    /// tracked as separate sets there, so a region this relation reached first still owes
    /// whatever bound another asks of it. The lowerer's `order_releases` reads all three
    /// relations alike (`alias → source`), which is what composes the ordering across a
    /// call standing between a read and the container whose release frees the page.
    /// Container READS declare `Funnel` too but are absent here — their result is the
    /// interior element, recorded by `counted_read_aliases` against its true container.
    pub funnel_result_containers: Vec<(HirId, Region, Region)>,
    /// Call sites of a moves-out ∩ PassThrough native (`%pop`/`%pop-array*`) — a
    /// non-fresh element REMOVED from a container and escape-retained IN-BODY
    /// (`arena::pop_with_decref` increfs the element before releasing the container;
    /// `dispatch_native_call` then skips its own pass-through retain via
    /// `def.moves_out`). At exactly these sites the lowerer DROPS the tail
    /// `IncrefValueRegion` (ReturnValue) retain in TAIL position
    /// (`lir::lower::control::call`): the in-body escape retain already handed the
    /// caller one owning reference, so a second ReturnValue retain double-counts and
    /// frees the moved-out element under a live reference
    /// (`region_pop_tail_moves_out_uaf`). Gated to `PassThrough` at recording time
    /// (`RegionInference::call_moves_out_passthrough`) so a moves-out native with a
    /// FRESH result (`@string` grapheme / `@bytes` int pop) is ABSENT and KEEPS its
    /// tail retain — its result is born rc=1 with no in-body retain and would
    /// over-free if suppressed. The moves-out analogue of `container_release_sites`;
    /// unlike it, this is set by the region walk (an intrinsic property of the
    /// callee), not the branch compensation. In NON-tail position no such retain is
    /// emitted, so this set is consulted only on the tail path.
    pub moves_out_release_sites: FxHashSet<HirId>,
    /// Monomorphic store/remove funnel call sites where the per-arm CONTAINER
    /// compensation (`regions::compensate`) released the wrapper's owned-param
    /// reference to the container AND the funnel is a `-mut` pass-through (so the
    /// result IS that container). At exactly these sites the lowerer DROPS the
    /// redundant tail `IncrefValueRegion` (ReturnValue) retain
    /// (`lir::lower::control::call`): the wrapper no longer holds the container after
    /// the arm, and the funnel already handed the caller one owning reference (arg0
    /// pass-through via `pass_through_retain`, or a fresh copy owned by the caller's
    /// binding), so a second ReturnValue retain would out-count the caller's single
    /// release. A RAW (non-wrapper) funnel tail call is absent from this set (no
    /// branch, no compensation), so it KEEPS its ReturnValue retain — dropping it
    /// there over-frees a fresh result whose sole owning reference is that retain
    /// (`region_native_tail_return_uaf`). Set from the compensation pass, so it
    /// reflects exactly where locus A fired.
    pub container_release_sites: FxHashSet<HirId>,
    /// Subset of `call_result_regions` that are CAPTURE-CELL placeholders: a
    /// captured (env-allocated) binding's per-value env cell (an `@x` lbox, a
    /// captured-mutable local cell). The lowerer releases these with
    /// `LoadCaptureRaw` + `DecrefCellRegion` (free the CELL's own region via
    /// `region_of`) instead of `LoadLocal` + `DecrefValueRegion` (which would
    /// unwrap the cell to the inner value's caller-owned region). docs/impl/region/rules.md
    /// Rule 8 (no leaks) — these env cells need an explicit release.
    pub cell_release_regions: FxHashSet<Region>,
    /// Call HirIds whose may-store edges are HARD: native call sites with a
    /// declared uncounted-store effect (`Stores`/`Mixed`/`Unknown`). At these
    /// sites the lowerer emits the edge incref for a call-result source by
    /// VALUE (the slot-keyed `IncrefRegion` never resolves for a call-result
    /// placeholder, and the target's free cascade then steals a live
    /// reference — the call-result-arg clique UAF). Edges recorded at opaque
    /// user-fn sites keep the slot path, the no-op for call-result
    /// sources: a wrapper's inner runtime funnel already counts a real store,
    /// so a real outer incref would never balance (docs/impl/region/effects.md "Hard
    /// edges: how a may-store edge is emitted").
    pub hard_edge_sites: FxHashSet<HirId>,
    /// Regions whose ordinary compiler-emitted decref the lowerer must SKIP,
    /// because the value's release is owned by a mutable binding's store path
    /// (drop-on-overwrite for a displaced prior value, or the kept binding-slot
    /// decref for the reaching value). Populated by `analyze_regions_with` for
    /// the assign-value regions of a sole-held, reassigned, top-level
    /// (file-letrec) binding.
    pub suppressed_decref_regions: FxHashSet<Region>,
    /// `Assign`/`SetCell` HirIds where the lowerer must emit drop-on-overwrite:
    /// load the binding slot's CURRENT (prior) value BEFORE the store and
    /// decref its region (its true demise is the overwrite, where the slot
    /// still holds it).
    pub drop_on_overwrite_sites: FxHashSet<HirId>,
    /// The subset of `drop_on_overwrite_sites` that are MODULE-SCOPE
    /// (file-letrec) 1-slot containers, where the cell **adopts the producer's
    /// reference** rather than taking a fresh counted one. Such a binding's
    /// assign-value regions have their ordinary decref suppressed
    /// (`suppressed_decref_regions`), so the producer's single reference is
    /// donated to the cell and the drop-on-overwrite is that reference's sole
    /// release. The lowerer must therefore SKIP the incref-on-store at these
    /// sites: born + drop-on-overwrite already balances, so an extra incref would
    /// hold every displaced prior to frame teardown — the unbounded per-iteration
    /// over-keep of a reassign-in-loop (docs/impl/region/bindings.md "Reassigned
    /// mutable bindings are 1-slot containers"). FN-LOCAL
    /// drop-on-overwrite sites are absent here: a fn-local cell's scope EXITS, so
    /// it needs a release of its own ([`CellContainer`]'s content drop) and
    /// therefore a reference of its own — the counted incref-on-store.
    pub donated_overwrite_sites: FxHashSet<HirId>,
    /// Init + assign-value regions of every reassigned TOP-LEVEL (file-letrec)
    /// slot binding, recorded unconditionally — independent of the suppression
    /// gate. The backstop for docs/impl/region/bindings.md "a mutated slot is not
    /// a release route": a reassigned binding's slot holds different values over
    /// time, so the lowerer's value-routed release (a `LoadLocal slot` then
    /// `DecrefValueRegion`) at a region's `decref_point` would load whatever the
    /// slot holds THEN — not the value whose region is being released — and
    /// mis-free a live, unrelated value (the no-alias-corruption UAF,
    /// region-mutable-reassign-flow facet 3). When the gate SUCCEEDS these
    /// regions are already in `suppressed_decref_regions`; when it FAILS (the
    /// unsuppressed baseline) the lowerer skips the value-routed release for any
    /// region here — an over-keep until file-letrec frame teardown, never a
    /// mis-free. Fn-local reassigns are excluded: their final value's release is
    /// a legitimate scope-exit slot route, and the scope-based solver shares
    /// regions, so skipping there would leak (region-tailcall-arg-transfer).
    pub mutated_binding_value_regions: FxHashSet<Region>,
    /// Every fn-local (in-lambda) reassigned mutable binding — the atoms the
    /// fn-local arm of `apply_reassign_containers` models as 1-slot containers.
    /// Their stack slot is a mutated container that holds a live value across the
    /// binding's whole scope (`allocate_slot` never reuses a slot, so the slot is
    /// the binding's alone). The lowerer skips the value-route decref + nil-stamp
    /// at `emit_decrefs_for` for any region whose `region_to_slot` names such a
    /// binding's slot: the slot's own value must never be nil-stamped mid-scope.
    /// This closes the reassigned-loop-counter clobber — an immediate-valued
    /// counter (`(assign ii (%add ii 1))`) whose spurious assign-value region the
    /// gate KEEPS (the fn-local scope-exit demise), whose `decref_point` the
    /// analysis places inside the loop, and whose nil-stamp then zeroes the
    /// counter before the increment reads it (pinned by
    /// `tests/elle/region-capture-cell-loop-uaf.lisp` under `--wasm=full`).
    pub reassigned_local_bindings: FxHashSet<Binding>,
    /// The fn-local reassigned mutables that took the 1-slot-container gate, by
    /// binding. Carries the two release channels the cell's own counted
    /// reference needs (see [`CellContainer`]); the module-scope half is absent
    /// because there the producer's reference is donated to the cell and the
    /// final content is freed by the file-letrec frame teardown.
    pub cell_containers: HashMap<Binding, CellContainer>,
    /// Begin HirId → per-binding region for each pre-allocated capture cell
    /// (`lower_begin`'s MakeCaptureCell pre-pass), in `collect_preallocate_
    /// bindings` order. One region PER CELL — emitting every cell against the
    /// Begin's single slot orphans all but the last minted physical region
    /// (docs/impl/region/model.md, "one allocation execution per slot between drops";
    /// the shared-slot capture-cell leak). Each region's `decref_point` is
    /// extended over its own binding's uses by the binding-chain post-pass.
    pub begin_cell_regions: HashMap<HirId, Vec<(Binding, Region)>>,
    /// The builder-idiom merge seed (docs/impl/region/merging.md § Merging): for a
    /// fresh child aggregate that is stored into the parent `%pair` it becomes a
    /// field of — sole-held, non-escaping, and dying at the same `decref_point` —
    /// `merged_parent[child] = parent`. A region has at most one merge parent
    /// (a child is stored into exactly one parent to qualify), so this is a
    /// forest, never a cycle; `merged_root` follows it to the outermost region.
    ///
    /// Computed by `regions::merge` and consumed by the lowerer: `static_slot`
    /// canonicalizes every region through `merged_root`, so a merge tree's child,
    /// parent, and deeper nests all allocate against, incref, and decref ONE static
    /// slot (the root's), landing in one physical region freed by the root's single
    /// `DecrefRegion` (docs/impl/region/merging.md § Merging). Empty unless a
    /// builder-idiom merge fired (a nested `%pair` builder idiom); when empty
    /// `merged_root` is the identity and the
    /// lowerer's behaviour is the unmerged one-region-per-value baseline.
    pub merged_parent: HashMap<Region, Region>,
    /// Every member region of a letrec closure-cycle merge — the SCC closures
    /// and their forward cells, roots included
    /// (docs/impl/region/letrec.md § The letrec closure-cycle merge). The merged
    /// arena is released exactly once by the merge's own channel: the root's
    /// binding-scope `DecrefRegion` (a non-tail body, OR a native body tail whose
    /// frame is not replaced), the stranded-cycle tail-call deferred release (a MEMBER body
    /// tail — `stranded_cycle_bindings`), or the explicit arena deferred release on a
    /// NON-member body tail whose callee resolves to a closure (`cycle_tail_release`
    /// → `TailCall::deferred_release_slot`). So `tail_callee_defers_release` refuses any OTHER
    /// tail call to a member — an interior sibling rotation's callee region demises
    /// at that call node and would otherwise pass the general dies-here deferral,
    /// double-releasing the arena. A subset of the merge forest's keys/roots; empty
    /// when no cycle merged.
    pub closure_cycle_members: FxHashSet<Region>,
    /// Non-member body-tail-call sites of a closure-cycle merge: tail-call HirId →
    /// the merged arena's canonical root region (docs/impl/region/letrec.md § The
    /// letrec closure-cycle merge). A `letrec` whose body ends in a tail call to a
    /// NON-member (a native `%add`, a redefined operator `+`, a foreign closure `g`)
    /// strands the merged arena's binding-scope `DecrefRegion` as dead code past the
    /// frame-replacing `TailCall`; the lowerer reads this to carry the arena's static
    /// slot on that `TailCall` (`deferred_release_slot`), so when the callee resolves to a
    /// closure the new activation takes over its release, freeing it at the recursion's completion,
    /// while a native callee falls through to the live scope-exit drop. A MEMBER body
    /// tail keeps the `stranded_cycle_bindings` → `tail_callee_defers_release` path and is NOT
    /// recorded here. Empty when no cycle merged (or every merged cycle's body
    /// tail-calls only members). Populated from `ClosureCycleMerge::tail_release_sites`.
    pub cycle_tail_release: HashMap<HirId, Region>,
    /// Regions whose every holder binding leaves this activation by NO facet —
    /// non-mutated, non-escaping, off the return/fiber frontiers — so the frame
    /// holds the region's one reference.
    ///
    /// This is escape's answer to the **count** question, projected onto regions,
    /// and it is the admission any mechanism owes when it makes a release fire
    /// where none fired before. The lowerer reads it for the frame-exit release at
    /// a tail call (docs/impl/region/mechanism.md § "A release past a
    /// frame-replacing tail call is not a release"), which converts a release the
    /// closure path never ran into one it does; the branch-arm release window
    /// applies the same predicate inline for the same reason.
    ///
    /// Lexical capture is deliberately not one of the refusals: a closure's hold on
    /// what it captures is counted (or owning), never an uncounted borrow, and
    /// capture by an *escaping* closure is already an escape facet
    /// (`regions::escape::sole_frame_held_regions`). The mutated refusal is not an
    /// escape fact but compensation's release-route one, so it is asked per region
    /// rather than per holder: a `cell_release_regions` member names the cell BOX,
    /// which no `assign` repoints, and keeps its mutated holder
    /// (docs/impl/region/mechanism.md § "A mutated holder poisons its value route,
    /// not its cell box").
    pub sole_frame_held_regions: rustc_hash::FxHashSet<Region>,
    /// Regions whose every holder binding leaves this activation by the **return**
    /// facet and no other — off the fiber frontier, escaping nowhere but a tail,
    /// and with the same mutated-holder reading as `sole_frame_held_regions`. A
    /// superset of it.
    ///
    /// Not an admission on its own: something *does* read such a region after the
    /// frame, namely the caller, through a reference the tail callee's own
    /// `Return` mints — and that mint fires after a relocated release would have
    /// run. So the lowerer pairs this with `TailCalleeFacts::capture_funded` at each
    /// relocation point, and the pair is the count argument
    /// (docs/impl/region/mechanism.md § "The callee's return mint, and the edge
    /// that funds the gap").
    pub return_frame_held_regions: rustc_hash::FxHashSet<Region>,
    /// Frame-replacing tail-call HirId → what that call's own callee tells the
    /// lowerer about the releases and mints around it ([`TailCalleeFacts`]).
    /// Populated only where the callee resolves to a lambda this compilation can
    /// see — a `Var` naming a `Let`/`Letrec`/`Define`-bound lambda in this unit;
    /// every consumer takes its conservative branch when a call is absent.
    pub tail_callee_facts: HashMap<HirId, TailCalleeFacts>,
    /// Ownership forest (docs/impl/region/ownership.md § "Adoption and subtree
    /// drop"), populated by the ownership pass; empty when the shape stays Shared,
    /// so the lowerer's emission is then the per-region-RC baseline. Store-site HirId → the interior
    /// containment edges `(child_region, parent_region)` of an externally-unique
    /// Owned subtree (`regions::ownership::compute_owned_subtrees`). At each such
    /// site the lowerer emits `AdoptRegion(parent, child)` — linking the child's
    /// runtime region into the parent's Owned subtree (no RC) — instead of the
    /// interior edge's `IncrefRegion`. The subtree's root keeps its single decref;
    /// freeing the root's region subtree-drops every adopted member, reclaiming
    /// interior cycles the per-region RC cascade cannot. Only subtrees whose root
    /// decref post-dominates every member's last use are admitted (the lifetime
    /// obligation, merge gate 6 generalized).
    pub owned_adopt_edges: HashMap<HirId, Vec<(Region, Region)>>,
    /// Ownership forest, **capture** half: Lambda HirId → the capture containment
    /// edges `(captured_region, closure_region)` to adopt at that closure's
    /// construction site. A value captured by a *local* Owned closure is interior to
    /// the closure's subtree, but capture records no `cross_region_refs` store site
    /// (the RC double-count fix — the closure's auto-incref over its env stands in for
    /// a static `IncrefRegion`), so its adopt cannot ride the `owned_adopt_edges`
    /// store-site path. Instead the lowerer, at `MakeClosure`, emits a value-resolved
    /// `AdoptRegion(closure, captured)` in place of the capture `IncrefRegion` for each
    /// edge here (`lower_lambda_expr`). Populated by the ownership pass; empty when the
    /// shape stays Shared. Disjoint from `owned_adopt_edges`: a member is adopted by its
    /// single owner through exactly one of the two maps (the store site or the capture
    /// site), never both.
    pub capture_adopt_edges: HashMap<HirId, Vec<(Region, Region)>>,
    /// Ownership forest, **cell⊇content** half: the bindings whose compiled capture cell
    /// adopts its stored content. At each such binding's cell-store site (the
    /// `MakeCaptureCell`/`StoreCaptureCell`) the lowerer emits
    /// `AdoptCellRegion(cell, content)` — linking the content's runtime region into the
    /// CELL's own region (`region_of`, not the unwrapped content), so a local
    /// `closure ⊇ cell ⊇ content` clique frees as one subtree. The cell store is uncounted,
    /// but the runtime alloc-scan over the cell already increfs the content, so the adopt
    /// consumes that count with no explicit balancing (the funnel-adopt discipline). Only
    /// an IMMUTABLE letrec cell reaches here — a re-storable cell's content is refused (the
    /// loop hazard; `region-capture-cell-loop-uaf.lisp`). The cell region itself is
    /// capture-adopted into the holding closure via `capture_adopt_edges`, and its own
    /// decref is suppressed (`suppressed_decref_regions`); the content keeps its own decref
    /// (a frozen no-op under the Owned region). Populated by the ownership pass; empty when
    /// the shape stays Shared.
    pub cell_content_adopt_bindings: FxHashSet<Binding>,
    /// Ownership forest, **co-owned-cycle** cut, populated by the ownership pass;
    /// empty when no such cycle is present. A
    /// mutual reference cycle with no container parent (an externally-unique source
    /// strongly-connected component of the containment graph) has no owner among its
    /// members — each owns and is owned by the others — so it is reclaimed
    /// symmetrically as one unit rather than by promoting a member to root. Keyed by
    /// the group's **drop site**: the HirId of the innermost structural scope enclosing
    /// every member's allocation, whose scope-exit post-dominates every member's last use
    /// and every pass-through-alias deref, where the lowerer emits a single
    /// `FreeRegionGroup` over the whole member set in place of the members' individual
    /// decrefs. The runtime
    /// frees the set as one four-phase subtree drop, so interior member↔member
    /// references reclaim with the group (the cycle per-region RC cannot collect,
    /// region/rules.md Rule 8) and only genuinely-Shared frontier references cascade.
    pub owned_region_groups: HashMap<HirId, Vec<Region>>,
    /// The union of every [`owned_region_groups`](Self::owned_region_groups) member
    /// region — the O(1) set the lowerer's `emit_decrefs_for` consults to SKIP a
    /// co-owned member's individual decref (the `FreeRegionGroup` at the group's drop
    /// site is its sole release). Kept beside the keyed map purely to avoid scanning
    /// every group per region at emit; populated together, empty when no group is present.
    pub owned_group_members: FxHashSet<Region>,
    /// Ownership forest, **transferred-returned-subtree** cut, populated by the
    /// ownership pass; empty when no such transfer is present. The consumer-site
    /// call-result regions of a summarized producer (a callee/fiber body whose
    /// returned subtree is externally unique and cyclic — docs/impl/
    /// region/owner.md § "Owner nodes" — "The transferred returned subtree").
    /// At each such region's release point the lowerer emits
    /// `AdoptIntoActivation` IN PLACE OF the value-resolved `DecrefValueRegion`
    /// (slot-loaded or discarded-result path alike): the adopt consumes the
    /// whole count — the cycle's stuck back-edge reference included — and the
    /// consuming activation's owner-node release set-drops root + interior
    /// members (adopted under it by the producer-side edges merged into the
    /// adopt maps above). Computed by
    /// `regions::ownership::compute_transfer_adopts`.
    pub transfer_adopt_regions: FxHashSet<Region>,
    /// Ownership forest, **activation-owner** cut, populated by the ownership pass;
    /// empty when no capture-back-edge SCC is present. Adopt-site HirId — the innermost
    /// structural scope enclosing every member's allocation — → the member regions of
    /// a capture-back-edge SCC (a container captured by a closure it holds:
    /// `m ⊇ c` by store, `c ⊇ m` by capture — the cycle no region root can own;
    /// docs/impl/region/owner.md § "Owner nodes" — "The capture-back-edge SCC").
    /// At the site the lowerer emits one value-resolved `AdoptIntoActivation` per
    /// member (`emit_adopt_into_activation`), moving it `Counted → Owned` under the
    /// executing activation's owner node; each member's own compiler decref is
    /// suppressed (`suppressed_decref_regions`, the suppress ⊆ adopt contract), so
    /// the node's completion release is the members' sole demise — the interior
    /// m↔c references reclaim with the set. Members are in allocation program
    /// order. Computed by `regions::ownership::compute_activation_adopts`.
    pub activation_adopt_sites: HashMap<HirId, Vec<Region>>,
    /// Per-path branch compensation: an arm-body HirId → regions whose
    /// `DecrefRegion` the lowerer must emit at that arm's HEAD. A region whose
    /// single `decref_point` sits inside a *sibling* arm is freed by that arm's
    /// in-arm decref on the used path, but leaks on this arm (the path never
    /// reaches the use); the compensating release at this arm's head frees it
    /// once on this path, before any tail call. The two releases are on mutually
    /// exclusive arms, so exactly one fires per path. Computed by
    /// `regions::compensate`; empty when no branch leaks a live-in region.
    pub branch_compensation: HashMap<HirId, Vec<Region>>,
    /// Per-arm decref placement: HirId → regions whose release the lowerer must
    /// emit AFTER that node (through `emit_decrefs_for`'s value-route). The
    /// used-sibling-arm counterpart to `branch_compensation`'s dead-arm head
    /// releases: when a region is used in MULTIPLE arms of an `If`/`Match`, its
    /// single `decref_point` lands in one arm, so every OTHER arm uses it but
    /// never frees it (the per-call leak stdlib `put`/`set` take through their
    /// `(match (type-of coll) …)` store dispatch). For each such sibling arm the
    /// node here is the region's last use WITHIN that arm, so the release fires
    /// after the arm's own use (not at its head — that would precede the use, a
    /// UAF). Exactly one of the per-arm releases (or the `decref_point` itself)
    /// fires per path, the arms being mutually exclusive. Computed by
    /// `regions::compensate`; empty when no branch leaks a multiply-used region.
    pub branch_arm_decrefs: HashMap<HirId, Vec<Region>>,
    /// Statistics.
    pub stats: RegionStats,
}

impl RegionInfo {
    pub fn empty() -> Self {
        RegionInfo {
            alloc_region: HashMap::new(),
            scope_region: HashMap::new(),
            binding_region: HashMap::new(),
            binding_source_regions: HashMap::new(),
            captured_reassigned_bindings: FxHashSet::default(),
            sole_frame_held_regions: FxHashSet::default(),
            return_frame_held_regions: FxHashSet::default(),
            tail_callee_facts: HashMap::new(),
            live_regions: FxHashSet::default(),
            cross_region_refs: Vec::new(),
            region_data: HashMap::new(),
            binding_last_use: HashMap::new(),
            call_result_regions: FxHashSet::default(),
            counted_cell_read_sites: FxHashSet::default(),
            fresh_result_regions: FxHashSet::default(),
            fiber_result_regions: FxHashSet::default(),
            containment_edges: Vec::new(),
            funnel_store_sites: HashMap::new(),
            funnel_bytecopy_value_sites: HashMap::new(),
            funnel_container_sites: HashMap::new(),
            funnel_passthrough_sites: HashMap::new(),
            uncounted_read_sites: HashMap::new(),
            counted_read_aliases: Vec::new(),
            opaque_result_aliases: Vec::new(),
            funnel_result_containers: Vec::new(),
            moves_out_release_sites: FxHashSet::default(),
            container_release_sites: FxHashSet::default(),
            cell_release_regions: FxHashSet::default(),
            hard_edge_sites: FxHashSet::default(),
            suppressed_decref_regions: FxHashSet::default(),
            drop_on_overwrite_sites: FxHashSet::default(),
            donated_overwrite_sites: FxHashSet::default(),
            mutated_binding_value_regions: FxHashSet::default(),
            reassigned_local_bindings: FxHashSet::default(),
            cell_containers: HashMap::new(),
            begin_cell_regions: HashMap::new(),
            merged_parent: HashMap::new(),
            closure_cycle_members: FxHashSet::default(),
            cycle_tail_release: HashMap::new(),
            owned_adopt_edges: HashMap::new(),
            capture_adopt_edges: HashMap::new(),
            cell_content_adopt_bindings: FxHashSet::default(),
            owned_region_groups: HashMap::new(),
            owned_group_members: FxHashSet::default(),
            transfer_adopt_regions: FxHashSet::default(),
            activation_adopt_sites: HashMap::new(),
            branch_compensation: HashMap::new(),
            branch_arm_decrefs: HashMap::new(),
            stats: RegionStats::default(),
        }
    }

    /// The compiled capture-cell region for `binding`, ONLY when the binding minted
    /// exactly ONE cell across all `begin_cell_regions` scopes. `None` for a binding with
    /// no compiled cell (a `populate_env` route or a by-value capture) OR with MORE than
    /// one — a file-body/nested-`begin` double-declare, where which physical cell a given
    /// closure holds is not resolvable from the binding alone (the two cells have distinct
    /// regions). The ownership forest's `closure ⊇ cell` re-point
    /// (`regions::ownership::capture`) and its `AdoptCellRegion` emit
    /// (`lir::lower::cell_region_of_binding`) both gate on this, so analysis and the lowerer
    /// name the same cell — or agree to refuse (leave the capture a borrow, Shared).
    pub fn single_cell_region_of(&self, binding: Binding) -> Option<Region> {
        let mut found: Option<Region> = None;
        for cells in self.begin_cell_regions.values() {
            for &(b, r) in cells {
                if b == binding {
                    match found {
                        None => found = Some(r),
                        Some(prev) if prev == r => {}
                        // A second, distinct cell for the same binding — ambiguous.
                        Some(_) => return None,
                    }
                }
            }
        }
        found
    }

    /// Does this scope have any allocations whose solved region matches it?
    pub fn scope_has_local_allocs(&self, hir_id: HirId) -> bool {
        self.scope_region
            .get(&hir_id)
            .is_some_and(|r| self.live_regions.contains(r))
    }

    /// The region a builder-idiom merge collapses `r` onto — the outermost
    /// ancestor in the `merged_parent` forest (docs/impl/region/merging.md
    /// § Merging). `r` itself when it is not a merge child. Bounded by the forest
    /// depth; a small guard rejects any cycle (there are none by construction).
    pub fn merged_root(&self, r: Region) -> Region {
        let mut cur = r;
        let mut guard = 0u32;
        while let Some(&parent) = self.merged_parent.get(&cur) {
            cur = parent;
            guard += 1;
            if guard > 10_000 {
                break;
            }
        }
        cur
    }

    /// True when the cross-region store edge `source → target` becomes an
    /// intra-region **self-edge** once the builder-idiom merge collapses both
    /// endpoints onto one physical region — the eliminable class of transform 2
    /// (docs/impl/region/mechanism.md § "Self-edge elimination").
    ///
    /// Soundness of elimination: the free-time cascade skips a region's
    /// references into *itself* (regionpool/introspect.rs decrefs a referenced
    /// region only when `rid != own_id`), so a merged `source → target` edge's
    /// `IncrefRegion(source)` has **no** balancing decref — keeping it leaks the
    /// region, dropping it is exact (the compiler-side mirror of the cascade's
    /// own self-skip). Post-merge both endpoints resolve to one slot iff they
    /// share a `merged_root`; that root equality is precisely the slot equality
    /// `static_slot` produces (it canonicalizes through the forest), so when this
    /// returns true `emit_increfs_for` drops the edge's `IncrefRegion`.
    ///
    /// This isolates the eliminable edge from the two must-keep classes by
    /// construction, because the merge seed never collapses either: a `(%pair x
    /// x)` alias whose `x` escapes is not sole-held, so it stays unmerged and its
    /// two `x → pair` edges keep distinct roots (N references need N increfs —
    /// dropping one is a UAF); a native may-store clique edge is not an immutable
    /// `%pair` store, so it too stays unmerged and kept (its balancing decref is
    /// the target's runtime content scan). `record_edge` already drops a raw
    /// `source == target` edge, so this fires only on a merge-collapsed edge.
    pub fn is_merge_self_edge(&self, source: Region, target: Region) -> bool {
        self.merged_root(source) == self.merged_root(target)
    }
}
