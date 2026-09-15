// audited: 2026-09-15
//! `RegionInfo`: what region inference produces for one compilation unit — the
//! per-allocation and per-scope assignments, and the cuts the lowerer consults.
//!
//! docs/impl/region/model.md
//!
//! Each field below says what it holds, who writes it, and who reads it. The
//! argument for why it exists is in the document the field names.

use crate::hir::binding::Binding;
use crate::hir::expr::HirId;

use super::{Region, RegionData, RegionStats};

use rustc_hash::FxHashSet;
use std::collections::HashMap;

mod cellstore;
mod query;

pub use cellstore::{CellContainer, CellStore, CellStores};

/// What a frame-replacing tail call's own callee settles about the RC traffic
/// the lowerer emits around it. Recorded per call, being a claim about *this*
/// callee.
///
/// docs/impl/region/relocate.md
#[derive(Debug)]
pub struct TailCalleeFacts {
    /// How many arguments this callee turns into owned parameters. Arguments
    /// past this index are collected into the rest parameter's fresh list.
    pub fixed_params: usize,
}

/// Results of region inference for a compilation unit.
///
/// Every allocation site has a solved region in `alloc_region`; every scope
/// (`Let`, `Letrec`, `Block`, `Loop`, `Lambda`) has one in `scope_region`. A
/// scope is reclaimable when its region appears in `live_regions`.
pub struct RegionInfo {
    /// HirId → solved region for each allocation site.
    pub alloc_region: HashMap<HirId, Region>,
    /// HirId → region introduced by each scope node.
    pub scope_region: HashMap<HirId, Region>,
    /// Binding → region where the binding lives.
    pub binding_region: HashMap<Binding, Region>,
    /// Binding → source regions the binding's value may point into. Copied
    /// from the walk's `binding_regions`; read by the `decref_point` extension
    /// so a binding's last use keeps its source regions alive.
    ///
    /// docs/impl/region/bindings.md
    pub binding_source_regions: HashMap<Binding, Vec<Region>>,
    /// Captured (`needs_capture`) bindings some `assign` reassigns. The lowerer
    /// drops such a binding's init reference off its own register rather than
    /// through the cell slot. Membership is a fact about the binding, not about
    /// the write's scope.
    ///
    /// docs/impl/region/cells.md
    pub captured_reassigned_bindings: FxHashSet<Binding>,
    /// Regions with at least one allocation assigned to them.
    pub live_regions: FxHashSet<Region>,
    /// `(store_site, source_region, target_region)` — a value in `source` is
    /// stored into a structure in `target`. The lowerer emits
    /// `IncrefRegion(source)` at the site; the runtime cascade balances it.
    ///
    /// docs/impl/region/mechanism.md
    pub cross_region_refs: Vec<(HirId, Region, Region)>,
    /// Per-region metadata, `decref_point` above all.
    pub region_data: HashMap<Region, RegionData>,
    /// Per-region last use resolved through its holder bindings' uses, max'd
    /// across holders. Unlike `region_data[r].decref_point` it is NOT also
    /// max'd with the structural alloc-site last use. Read by the ownership
    /// lifetime obligation.
    ///
    /// docs/impl/region/adopt.md
    pub binding_last_use: HashMap<Region, HirId>,
    /// Regions minted at a `Call` HirId — placeholders for the runtime region
    /// of the callee's returned value, which the caller cannot statically name.
    /// The lowerer releases these by value (`DecrefValueRegion`).
    ///
    /// docs/impl/region/mechanism.md
    pub call_result_regions: FxHashSet<Region>,
    /// `Emit` site → the regions its payload may live in. Read by the
    /// borrowed-payload pass below.
    ///
    /// docs/impl/region/owner.md
    pub emit_payload_regions: HashMap<HirId, Vec<Region>>,
    /// `Emit` sites whose payload the emitting body releases nowhere. The
    /// lowerer mints one reference at each SUSPENDING site here — an error or
    /// halt emit is excluded, both being terminal. A payload whose regions the
    /// pass cannot resolve is treated as borrowed.
    ///
    /// docs/impl/region/owner.md
    pub borrowed_emit_payloads: FxHashSet<HirId>,
    /// `Emit` sites whose RESUME VALUE reaches this body counted by nothing, so
    /// the lowerer mints the reference the body holds it by. A site whose
    /// result region is on the return frontier is excluded — the frame's return
    /// transfer already funds one.
    ///
    /// docs/impl/region/owner.md
    pub unfunded_resume_values: FxHashSet<HirId>,
    /// Binding-init HirIds that are a whole-value read of a reassigned captured
    /// cell. The lowerer emits an `IncrefValueRegion` here, and the read's own
    /// placeholder region carries the balancing release. Element reads are
    /// excluded.
    ///
    /// docs/impl/region/bindings.md
    pub counted_cell_read_sites: FxHashSet<HirId>,
    /// Binder-init HirIds whose value a fn-local 1-slot container takes by a
    /// COUNTED store rather than by donation — one per admitted cell, or per
    /// admitted forwarding chain at the chain source. Disjoint from
    /// `suppressed_decref_regions`' init entries by construction.
    ///
    /// docs/impl/region/bindings.md
    pub counted_cell_init_sites: FxHashSet<HirId>,
    /// The subset of `call_result_regions` whose callee declares
    /// `RegionEffect::Fresh`. Widens ownership candidacy only; the baseline
    /// release is unchanged.
    ///
    /// docs/impl/region/effects.md
    pub fresh_result_regions: FxHashSet<Region>,
    /// Call-result regions whose callee declares `RetType::Fiber`. Never a
    /// member of a region-rooted ownership cut; reclaims on the RC baseline.
    ///
    /// docs/impl/region/adopt.md
    pub fiber_result_regions: FxHashSet<Region>,
    /// Containment edges for the ownership inference, from two sources: a
    /// `Funnel` store into a mutable retaining container (`container ⊇ value`)
    /// and a `Fresh` native's declared embed (`result ⊇ arg`). Site-keyed as
    /// `cross_region_refs` is, and driving no `IncrefRegion`.
    ///
    /// docs/impl/region/effects.md
    pub containment_edges: Vec<(HirId, Region, Region)>,
    /// Funnel-store call site → the regions of the heap values stored there.
    /// Recorded even where the container's type is statically unknown, which
    /// `containment_edges` needs. Read by the branch compensation.
    ///
    /// docs/impl/region/effects.md
    pub funnel_store_sites: HashMap<HirId, Vec<Region>>,
    /// Byte-copy funnel call site → the pushed value's regions. `%del` is
    /// excluded: it decrefs the value in-body.
    ///
    /// docs/impl/region/effects.md
    pub funnel_bytecopy_value_sites: HashMap<HirId, Vec<Region>>,
    /// Pass-through funnel-store call site → the CONTAINER argument's regions,
    /// recorded only for a `-mut` store returning a mutable container. Read by
    /// the branch compensation.
    ///
    /// docs/impl/region/effects.md
    pub funnel_container_sites: HashMap<HirId, Vec<Region>>,
    /// The `-mut` PASS-THROUGH subset of `funnel_container_sites`, whose funnel
    /// returns arg0 in place. Gates the lowerer's `ReturnValue` suppression; an
    /// immutable funnel is absent and keeps its retain.
    ///
    /// docs/impl/region/effects.md
    pub funnel_passthrough_sites: HashMap<HirId, Vec<Region>>,
    /// UNCOUNTED element-read site (`%get`/`%first`/`%rest`) → the regions of
    /// the container read from. The read raises no count, so the container's
    /// `decref_point` is extended to the read's last use.
    ///
    /// docs/impl/region/rules.md
    pub uncounted_read_sites: HashMap<HirId, Vec<Region>>,
    /// COUNTED element-read edges `(site, alias, container)` — a native
    /// `get`/`first`/`rest` call, minus the moves-out REMOVEs. Read by the
    /// ownership lifetime obligation and by the lowerer's release order.
    ///
    /// docs/impl/region/adopt.md
    pub counted_read_aliases: Vec<(HirId, Region, Region)>,
    /// Call-result alias edges `(site, result, argument)` for a callee under no
    /// declaration that its heap result lives in the call's own region. An
    /// inlined callee records nothing.
    ///
    /// docs/impl/region/adopt.md
    pub opaque_result_aliases: Vec<(HirId, Region, Region)>,
    /// Funnel-result identity edges `(site, result, container)`: a `Funnel`'s
    /// result is arg0 in place or a fresh copy of it, so it carries
    /// reachability into arg0's subtree without a bound of its own. Container
    /// reads are absent, their result being the interior element.
    ///
    /// docs/impl/region/effects.md
    pub funnel_result_containers: Vec<(HirId, Region, Region)>,
    /// Call sites of a moves-out ∩ `PassThrough` native, whose moved-out
    /// element is escape-retained in-body. The lowerer drops the tail
    /// `IncrefValueRegion` here. A moves-out native with a FRESH result is
    /// absent and keeps its retain.
    ///
    /// docs/impl/region/effects.md
    pub moves_out_release_sites: FxHashSet<HirId>,
    /// Funnel call sites where the per-arm container compensation released the
    /// wrapper's owned-param reference AND the funnel is a `-mut` pass-through.
    /// The lowerer drops the tail `IncrefValueRegion` here. A raw funnel tail
    /// call is absent and keeps its retain.
    ///
    /// docs/impl/region/compensate.md
    pub container_release_sites: FxHashSet<HirId>,
    /// The `call_result_regions` that are capture-cell placeholders. The
    /// lowerer releases these with `LoadCaptureRaw` + `DecrefCellRegion`, never
    /// by unwrapping the cell to the inner value's region.
    ///
    /// docs/impl/region/cells.md
    pub cell_release_regions: FxHashSet<Region>,
    /// The leaf bindings a `Destructure` pattern binds. Each inherits the
    /// destructured value's source regions, so a leaf NAMES the source without
    /// holding it. Read by the tail call's ownership move.
    ///
    /// docs/impl/region/relocate.md
    pub destructure_leaf_bindings: FxHashSet<Binding>,
    /// Call HirIds whose may-store edges are HARD — a declared
    /// `Stores`/`Mixed`/`Unknown` effect. The lowerer emits the edge incref for
    /// a call-result source by VALUE at these sites.
    ///
    /// docs/impl/region/effects.md
    pub hard_edge_sites: FxHashSet<HirId>,
    /// Regions whose ordinary compiler-emitted decref the lowerer must SKIP,
    /// the value's release belonging to a mutable binding's store path.
    ///
    /// docs/impl/region/bindings.md
    pub suppressed_decref_regions: FxHashSet<Region>,
    /// `Assign`/`SetCell` HirIds where the lowerer loads the slot's prior value
    /// BEFORE the store and releases it.
    ///
    /// docs/impl/region/bindings.md
    pub drop_on_overwrite_sites: FxHashSet<HirId>,
    /// The MODULE-SCOPE subset of `drop_on_overwrite_sites`, where the cell
    /// adopts the producer's reference rather than taking a counted one. The
    /// lowerer emits no incref-on-store at these sites. Fn-local sites are
    /// absent: their cell needs a reference of its own.
    ///
    /// docs/impl/region/bindings.md
    pub donated_overwrite_sites: FxHashSet<HirId>,
    /// Init and assign-value regions of every reassigned TOP-LEVEL slot
    /// binding, recorded independently of the suppression gate. The lowerer
    /// skips the value-routed release for any region here. Fn-local reassigns
    /// are excluded.
    ///
    /// docs/impl/region/bindings.md
    pub mutated_binding_value_regions: FxHashSet<Region>,
    /// Every fn-local reassigned mutable binding. The lowerer skips the
    /// value-route release and nil-stamp for a region whose slot is one of
    /// theirs, the slot holding a live value across the binding's whole scope.
    ///
    /// docs/impl/region/bindings.md
    pub reassigned_local_bindings: FxHashSet<Binding>,
    /// The fn-local reassigned mutables that took the 1-slot-container gate.
    /// The module-scope half is absent, its final content being freed by the
    /// file-letrec teardown.
    ///
    /// docs/impl/region/bindings.md
    pub cell_containers: HashMap<Binding, CellContainer>,
    /// Every region a fn-local 1-slot container stores. Such a value's region
    /// is a RUNTIME fact even where its allocation site names a static slot, so
    /// a later mint must read the region off the value.
    ///
    /// docs/impl/region/bindings.md
    pub cell_stored_regions: FxHashSet<Region>,
    /// Begin HirId → per-binding region for each pre-allocated capture cell,
    /// in `collect_preallocate_bindings` order. One region PER CELL. Each
    /// region's `decref_point` is extended over its binding's uses.
    ///
    /// docs/impl/region/cells.md
    pub begin_cell_regions: HashMap<HirId, Vec<(Binding, Region)>>,
    /// `Destructure`/`Match` HirId → per-binding placeholder region for each
    /// rest name whose pattern BUILDS a collection. The region is phantom, is
    /// in `call_result_regions`, and is pinned to the node keying it. A rest
    /// matched by a further pattern binds no name to the collection and is
    /// absent.
    ///
    /// docs/impl/region/anchors.md
    pub pattern_rest_regions: HashMap<HirId, Vec<(Binding, Region)>>,
    /// `merged_parent[child] = parent` for the builder-idiom merge. A forest,
    /// never a cycle; `merged_root` follows it to the outermost region, which
    /// `static_slot` canonicalizes through. Empty unless a merge fired.
    ///
    /// docs/impl/region/merging.md
    pub merged_parent: HashMap<Region, Region>,
    /// Every member region of a letrec closure-cycle merge, roots included.
    /// The merged arena is released exactly once, by the merge's own channel.
    /// Empty when no cycle merged.
    ///
    /// docs/impl/region/letrec.md
    pub closure_cycle_members: FxHashSet<Region>,
    /// Non-member body-tail-call site → the merged arena's canonical root. The
    /// lowerer carries the arena's static slot on that `TailCall`. A MEMBER
    /// body tail is not recorded here. Empty when no cycle merged.
    ///
    /// docs/impl/region/letrec.md
    pub cycle_tail_release: HashMap<HirId, Region>,
    /// Regions whose every holder binding leaves this activation by the RETURN
    /// facet at most — off the fiber frontier, with an unmutated release route
    /// — so the frame holds the region's one reference while the frame lives.
    /// The admission any mechanism owes when it makes a release fire where none
    /// fired before.
    ///
    /// docs/impl/region/relocate.md
    pub frame_held_regions: rustc_hash::FxHashSet<Region>,
    /// Regions a value-routed release can NAME — the analysis-side mirror of
    /// the slot `value_release_slot` would load. The conservative reading of
    /// the emitter's refusals.
    ///
    /// docs/impl/region/relocate.md
    pub value_routed_regions: rustc_hash::FxHashSet<Region>,
    /// Frame-replacing tail-call HirId → [`TailCalleeFacts`]. Populated only
    /// where the callee resolves to a lambda this compilation can see; every
    /// consumer takes its conservative branch when a call is absent.
    pub tail_callee_facts: HashMap<HirId, TailCalleeFacts>,
    /// Ownership forest, STORE half: store-site HirId → the interior
    /// containment edges `(child, parent)` of an Owned subtree. The lowerer
    /// emits `AdoptRegion(parent, child)` there in place of the edge's
    /// `IncrefRegion`. Empty when the shape stays Shared.
    ///
    /// docs/impl/region/ownership.md
    pub owned_adopt_edges: HashMap<HirId, Vec<(Region, Region)>>,
    /// Ownership forest, CAPTURE half: Lambda HirId → the capture containment
    /// edges `(captured, closure)` to adopt at that closure's construction. The
    /// lowerer emits a value-resolved `AdoptRegion` at `MakeClosure` in place
    /// of the capture `IncrefRegion`. Disjoint from `owned_adopt_edges`.
    ///
    /// docs/impl/region/ownership.md
    pub capture_adopt_edges: HashMap<HirId, Vec<(Region, Region)>>,
    /// Ownership forest, CELL ⊇ CONTENT half: the bindings whose compiled
    /// capture cell adopts its stored content. The lowerer emits
    /// `AdoptCellRegion(cell, content)` at the cell store. Only an immutable
    /// letrec cell reaches here.
    ///
    /// docs/impl/region/ownership.md
    pub cell_content_adopt_bindings: FxHashSet<Binding>,
    /// Ownership forest, CO-OWNED-CYCLE cut: the group's drop site → its member
    /// regions. The lowerer emits one `FreeRegionGroup` over the set there, in
    /// place of the members' individual decrefs. Empty when no such cycle is
    /// present.
    ///
    /// docs/impl/region/ownership.md
    pub owned_region_groups: HashMap<HirId, Vec<Region>>,
    /// The union of every `owned_region_groups` member — the O(1) set
    /// `emit_decrefs_for` consults to skip a member's individual decref.
    pub owned_group_members: FxHashSet<Region>,
    /// Ownership forest, TRANSFERRED-RETURNED-SUBTREE cut: the consumer-site
    /// call-result regions of a summarized producer. The lowerer emits
    /// `AdoptIntoActivation` in place of the value-resolved release.
    ///
    /// docs/impl/region/owner.md
    pub transfer_adopt_regions: FxHashSet<Region>,
    /// Ownership forest, ACTIVATION-OWNER cut: adopt site → the member regions
    /// of a capture-back-edge SCC, in allocation program order. An SCC whose
    /// scope can park carries a second entry keyed ahead of every park. Each
    /// member's own decref is suppressed.
    ///
    /// docs/impl/region/owner.md
    pub activation_adopt_sites: HashMap<HirId, Vec<Region>>,
    /// Arm-body HirId → regions whose release the lowerer emits at that arm's
    /// HEAD, the region's own `decref_point` sitting in a sibling arm. Exactly
    /// one of the two fires per path.
    ///
    /// docs/impl/region/compensate.md
    pub branch_compensation: HashMap<HirId, Vec<Region>>,
    /// HirId → regions whose release the lowerer emits AFTER that node — the
    /// region's last use WITHIN a sibling arm that uses it but does not free
    /// it. Exactly one of the per-arm releases fires per path.
    ///
    /// docs/impl/region/compensate.md
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
            frame_held_regions: FxHashSet::default(),
            value_routed_regions: FxHashSet::default(),
            tail_callee_facts: HashMap::new(),
            live_regions: FxHashSet::default(),
            cross_region_refs: Vec::new(),
            region_data: HashMap::new(),
            binding_last_use: HashMap::new(),
            call_result_regions: FxHashSet::default(),
            emit_payload_regions: HashMap::new(),
            borrowed_emit_payloads: FxHashSet::default(),
            unfunded_resume_values: FxHashSet::default(),
            counted_cell_read_sites: FxHashSet::default(),
            counted_cell_init_sites: FxHashSet::default(),
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
            destructure_leaf_bindings: FxHashSet::default(),
            hard_edge_sites: FxHashSet::default(),
            suppressed_decref_regions: FxHashSet::default(),
            drop_on_overwrite_sites: FxHashSet::default(),
            donated_overwrite_sites: FxHashSet::default(),
            mutated_binding_value_regions: FxHashSet::default(),
            reassigned_local_bindings: FxHashSet::default(),
            cell_containers: HashMap::new(),
            cell_stored_regions: FxHashSet::default(),
            begin_cell_regions: HashMap::new(),
            pattern_rest_regions: HashMap::new(),
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
}
