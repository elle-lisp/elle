// audited: 2026-09-29
//! Tofte-Talpin region inference for functional HIR: the walk's state, and how
//! it mints a region.
//!
//! docs/impl/region/model.md
//!
//! A single forward walk assigns every allocation its own unique region — no
//! constraint solver, no merging at this layer. The types this produces live in
//! the parent module. What the walk RECORDS against that state is a submodule
//! per subject. The walk itself is `walk`, and `analyze` runs it and then every
//! post-pass in order.

use super::super::arena::BindingArena;
use super::super::binding::Binding;
use super::super::defuse::DefUseBuilder;
use super::super::expr::{Hir, HirId, HirKind};
use super::super::liveness::{compute_last_use, compute_order, compute_subtree_low};
use super::super::pattern::HirPattern;
use super::{
    CallClassification, CellStores, Region, RegionData, RegionInfo, RegionStats, RestCollection,
};

use std::collections::HashMap;
use tree::RegionTree;

// ── Region inference walk (unique-per-alloc) ─────────────────────

struct RegionInference {
    tree: RegionTree,
    /// HirId → unique region assigned to that allocation site, written by
    /// `alloc_here`. The structural walk's entry stands; an inline re-walk
    /// reuses it.
    alloc_region: HashMap<HirId, Region>,
    /// HirId → region for scope nodes (Let, Letrec, Loop, Block,
    /// Lambda body, non-suspending While). Feeds the `live_regions`
    /// computation in `build_info`.
    scope_region: HashMap<HirId, Region>,
    /// Binding → region where the binding was defined (scope region).
    binding_region: HashMap<Binding, Region>,
    /// Binding → set of source regions a Var(b) reference may produce.
    /// Var(b) returns `binding_regions[b]` to propagate value flow. A parameter
    /// holds a placeholder region, and a pattern leaf the destructured value's
    /// regions.
    binding_regions: HashMap<Binding, Vec<Region>>,
    /// Binding → its stores (each an assign/set-cell site with the regions of
    /// the value stored there) for a MODULE-SCOPE (outside every lambda, or
    /// `is_file_scope`), non-capture binding that is reassigned. The reassign
    /// gate (`analyze::reassign`) models it as a 1-slot container.
    top_level_reassigns: HashMap<Binding, CellStores>,
    /// Reassigned CAPTURED (`needs_capture`) bindings, whatever scope the write
    /// sits in. They stay out of both 1-slot maps, because the cell's update
    /// opcode (`handle_update_capture`) counts the cell's content. The lowerer
    /// still reads the set: a reassigned cell's content changes, so the binder
    /// drops the init value's reference off its own register instead of through
    /// the cell slot (`store_captured_cell_init`).
    captured_reassigns: rustc_hash::FxHashSet<Binding>,
    /// Binding → its stores for a FN-LOCAL (inside a lambda, not `is_file_scope`),
    /// non-capture binding that is reassigned. The reassign gate applies the same
    /// 1-slot-container model as `top_level_reassigns`, but keeps the
    /// assign-value decrefs. A fn-local cell's final value is not a
    /// program-lifetime root: a content drop at the cell's scope demise releases
    /// it.
    local_reassigns: HashMap<Binding, CellStores>,
    /// Loop parameter → the binding its init `Var` forwards from. Functionalization
    /// rewrites a `while` that assigns a binding into a `Loop` whose parameter is a
    /// fresh version of that binding, initialized from the pre-loop version and
    /// standing in for it at every later read. Both versions therefore record the
    /// same source regions while holding ONE reference between them (a `Var` read
    /// mints nothing). The reassign 1-slot gate's sole-held check must not read
    /// them as two holders of one name (`RegionHolders::with_aliases`,
    /// docs/impl/region/bindings.md). An entry is recorded only for an init that
    /// is a bare `Var`; any other init expression is a real value the parameter
    /// does not merely carry forward.
    loop_forwarded_params: HashMap<Binding, Binding>,
    /// Scope node (`Begin`, `Let`, `Letrec`) HirId → one region PER compiled
    /// capture cell the node mints, mirroring the lowerer's `MakeCaptureCell`
    /// pre-passes. See `RegionInfo::begin_cell_regions`.
    begin_cell_regions: HashMap<HirId, Vec<(Binding, Region)>>,
    /// `Destructure`/`Match` HirId → one entry per collection the node's
    /// pattern BUILDS (see `RegionInfo::pattern_rest_regions`).
    pattern_rest_regions: HashMap<HirId, Vec<RestCollection>>,
    /// Every binding a scope arm above minted a COMPILED cell for. Its forward
    /// cell lives in the binding's own slot, so it takes no `populate_env` env
    /// cell — the mirror of the lowerer's own `compiled_cell_bindings`. The
    /// scope arms mint before they walk their inits, so a binding is recorded
    /// here before any use of it is walked.
    compiled_cell_bindings: rustc_hash::FxHashSet<Binding>,
    /// Cross-region edges recorded directly at storage / capture sites:
    /// (storage_site_hir_id, source_region, target_region).
    cross_region_refs: Vec<(HirId, Region, Region)>,
    /// Call sites whose edges are hard (declared native uncounted-store
    /// effects). See `RegionInfo::hard_edge_sites`.
    hard_edge_sites: rustc_hash::FxHashSet<HirId>,
    /// Placeholder regions for a value the walk cannot statically name: a call's
    /// result, a parameter, a rest collection, a counted whole-value read, a
    /// capture cell. The lowerer releases these by value (`DecrefValueRegion`),
    /// never by slot (`DecrefRegion`), at `decref_point`.
    call_result_regions: rustc_hash::FxHashSet<Region>,
    /// Binding-init HirIds that are a whole-value read of a 1-slot container —
    /// the reader half of the model. See `RegionInfo::counted_cell_read_sites`.
    counted_cell_read_sites: rustc_hash::FxHashSet<HirId>,
    /// Binding → the HirId of the init a BINDER stores into its slot
    /// (`Let`/`Letrec`/`Define`). The reassign gate reads this to place the
    /// counted-init retain of a 1-slot container whose init value is aliased.
    /// The retain has to sit where the value is on the operand stack, just ahead
    /// of that store. A binding whose value arrives some other way — a
    /// parameter, a `Loop` parameter's forwarding init — has no entry, and the
    /// gate keeps donate-or-refuse for it (docs/impl/region/bindings.md). The
    /// three arms that record here are exactly the three lowering sites that
    /// emit the retain. `None` records a binding bound by more than one binder
    /// (file-scope duplicate `def`s share a `Binding`): two stores would take
    /// two retains against one release, so the gate refuses rather than guess.
    binder_init_sites: HashMap<Binding, Option<HirId>>,
    /// The subset of `call_result_regions` whose callee declares
    /// `RegionEffect::Fresh` — a result freshly allocated in the call's own
    /// region, genuinely caller-owned. See `RegionInfo::fresh_result_regions`.
    fresh_result_regions: rustc_hash::FxHashSet<Region>,
    /// Call-result regions whose callee declares `RetType::Fiber` — a region
    /// holding a fiber, never a member of a region-rooted Owned subtree. See
    /// `RegionInfo::fiber_result_regions`.
    fiber_result_regions: rustc_hash::FxHashSet<Region>,
    /// Call-result regions whose callee returns a mutable *retaining* container
    /// (`RetType::MutableArray`/`MutableStruct`). Walk-internal: a later `Funnel`
    /// store whose container argument resolves to one of these recovers the
    /// containment the funnel records only at runtime. See `RegionInfo::
    /// containment_edges` and the `Funnel` arm in `region::infer::walk::call`.
    mutable_container_regions: rustc_hash::FxHashSet<Region>,
    /// Structural containment edges `(site, contained, container)` for the
    /// ownership inference. Three sources record them:
    ///
    /// - a `Funnel` store into a mutable retaining container, `container ⊇ value`,
    ///   recovered from the container's `RetType`;
    /// - a `Fresh` native's declared **embed**, `result ⊇ embedded_arg`, from
    ///   `call_embeds`;
    /// - a compiled capture cell, `cell ⊇ content`, from
    ///   `record_cell_content_edges`.
    ///
    /// None drives an `IncrefRegion`: the runtime counts each store. See
    /// `RegionInfo::containment_edges`.
    containment_edges: Vec<(HirId, Region, Region)>,
    /// Retaining funnel-store call site → the regions of the value stored there
    /// (the last argument of `%put`/`%array-push`/…). The runtime funnel increfs
    /// it whether or not the container's type is statically known, which
    /// `containment_edges` needs. Read by `region::infer::compensate` to bound a
    /// per-arm decref to a node where the value is re-incref'd. See
    /// `RegionInfo::funnel_store_sites`.
    funnel_store_sites: HashMap<HirId, Vec<Region>>,
    /// Byte-copy funnel call site → the stored value's regions. See
    /// `RegionInfo::funnel_bytecopy_value_sites`.
    funnel_bytecopy_value_sites: HashMap<HirId, Vec<Region>>,
    /// `Emit` (`yield`/`emit`) site → the regions its payload may live in. See
    /// `RegionInfo::emit_payload_regions`.
    emit_payload_regions: HashMap<HirId, Vec<Region>>,
    /// Container funnel call site → the CONTAINER argument (arg0) regions, for a
    /// store or remove funnel with a monomorphic container `RetType`, of either
    /// mutability, and for a moves-out remove funnel (`%pop`). A dispatch
    /// wrapper uses its container in every arm but releases it in one, so
    /// `region::infer::compensate` places a per-arm release at each such site.
    /// See `RegionInfo::funnel_container_sites`.
    funnel_container_sites: HashMap<HirId, Vec<Region>>,
    /// The `-mut` PASS-THROUGH subset of `funnel_container_sites` (the funnel returns
    /// arg0 in place). Gates the lowerer's ReturnValue suppression to the case where
    /// the result IS the owned container; an immutable fresh result keeps its retain.
    /// See `RegionInfo::funnel_passthrough_sites`.
    funnel_passthrough_sites: HashMap<HirId, Vec<Region>>,
    /// UNCOUNTED container element-READ site (`%get`/`%first`/`%rest`, inline opcodes
    /// that raise no reference count) → the CONTAINER (arg0) regions the read borrows out
    /// of. The container's own lifetime is what keeps the borrow alive, so its release
    /// must follow the READER's (docs/impl/region/rules.md). See
    /// `RegionInfo::uncounted_read_sites`.
    uncounted_read_sites: HashMap<HirId, Vec<Region>>,
    /// COUNTED container element-READ edges `(site, alias, container)` — a native
    /// `get`/`first`/`rest` call, whose pass-through retain covers the borrow under RC but
    /// is inert once adoption freezes the member. See `RegionInfo::counted_read_aliases`.
    counted_read_aliases: Vec<(HirId, Region, Region)>,
    /// Call-result alias edges `(site, result, argument)` for a callee under no
    /// declaration that its heap result lives in the call's own region — so the result
    /// may BE an argument, or a value inside one. See
    /// `RegionInfo::opaque_result_aliases`.
    opaque_result_aliases: Vec<(HirId, Region, Region)>,
    /// Funnel-result identity edges `(site, result, container)` — a `Funnel`'s result is
    /// arg0 in place or a fresh copy of it, so it propagates reachability into arg0's
    /// subtree without needing a lifetime bound of its own. See
    /// `RegionInfo::funnel_result_containers`.
    funnel_result_containers: Vec<(HirId, Region, Region)>,
    /// Call sites of a moves-out ∩ PassThrough native (`%pop`/`%pop-array*`) whose
    /// moved-out element is escape-retained in-body. See
    /// `RegionInfo::moves_out_release_sites`.
    moves_out_release_sites: rustc_hash::FxHashSet<HirId>,
    /// Subset of `call_result_regions` that are capture-cell placeholders for
    /// captured (env-allocated) bindings — released with `DecrefCellRegion`
    /// (`region_of` the cell), not `DecrefValueRegion` (`result_region_of` the
    /// inner value). See `RegionInfo::cell_release_regions`.
    cell_release_regions: rustc_hash::FxHashSet<Region>,
    /// The leaf bindings a `Destructure` pattern binds. See
    /// `RegionInfo::destructure_leaf_bindings`.
    destructure_leaf_bindings: rustc_hash::FxHashSet<Binding>,
    /// `(return_node_id, regions of the returned value)` for every
    /// `HirKind::Return`. The post-pass extends each region's `decref_point`
    /// to the Return node so the region's `DecrefRegion` is emitted
    /// *after* the node's `IncrefValueRegion`. The retain must precede the
    /// callee's own release of a freshly-allocated result region, or the result
    /// is freed before it is handed back.
    return_sites: Vec<(HirId, Vec<Region>)>,
    /// `(destructure_node_id, regions of the destructured value)` for every
    /// `HirKind::Destructure`. A Destructure is a *consuming node*: its
    /// field extraction reads the value AFTER the value expression's own
    /// last read, so the post-pass extends each region's `decref_point` to
    /// the Destructure node (docs/impl/region/rules.md). Without it, a
    /// destructure whose bindings are all unused anchors the value's
    /// release at the inner read, and the extraction reads freed pages.
    destructure_sites: Vec<(HirId, Vec<Region>)>,
    /// BlockId → enclosing region at the point the block was entered. The
    /// `Block` arm writes it, and nothing reads it.
    block_regions: HashMap<super::super::expr::BlockId, Region>,
    /// BlockId → the regions of every `break` value handed to that block, in
    /// walk order. A `break` TRANSFERS its value to the block — the block's
    /// value is its fall-through value OR any break's — so the `Block` arm
    /// unions these into its own result regions and clears the entry
    /// (docs/impl/region/mechanism.md). Without the union, a binding named to
    /// the block's value holds NO region, and the binding-chain `decref_point`
    /// extension never sees the broken value. Its release then stays at the
    /// block's exit label, under every later read of the result. Drained at the
    /// `Block` node into `break_sites`.
    block_break_regions: HashMap<super::super::expr::BlockId, Vec<Region>>,
    /// BlockId → the HirId of every `break` targeting that block, in walk order.
    /// Recorded for EVERY break, valueless and immediate-valued ones included —
    /// unlike `block_break_regions`, which only sees breaks that carry a region.
    /// What the post-pass needs from a break is its *position*: the jump to the
    /// exit label passes over every release from the break site onward, whatever
    /// the break carries. Drained at the `Block` node into `break_skip_blocks`.
    block_break_nodes: HashMap<super::super::expr::BlockId, Vec<HirId>>,
    /// `Block` node HirId → the HirIds of the breaks targeting it. The breaks
    /// jump over a window, from the earliest break site to the exit label, and a
    /// release emitted there never runs on the break path. The post-pass
    /// re-anchors every region whose `decref_point` falls in that window onto
    /// the block (docs/impl/region/mechanism.md).
    break_skip_blocks: Vec<(HirId, Vec<HirId>)>,
    /// `Block` node HirId → the regions every targeting `break` hands it. The
    /// dual of `return_sites`: a `Break` is a *transferring* node, so the
    /// post-pass extends each broken region's `decref_point` to where the
    /// BLOCK's value is consumed (`last_use[block]`). When nothing consumes it,
    /// that point is the block itself, whose decrefs the lowerer emits after the
    /// exit label. A release left inside the body is jumped over and never runs
    /// (docs/impl/region/rules.md).
    break_sites: Vec<(HirId, Vec<Region>)>,
    /// HirIds of the tail `Call`s whose callee may be a bytecode closure — the
    /// calls that REPLACE this frame instead of falling through. A tail call to a
    /// native pushes no frame and is absent here. The branch-arm release window
    /// declines any branch containing one, since its merge label is then not a
    /// point every arm reaches (docs/impl/region/mechanism.md).
    frame_replacing_tail_calls: rustc_hash::FxHashSet<HirId>,
    /// Next region id
    next_region: u32,
    /// Current enclosing region
    current_region: Region,
    /// Call classification: which callees return immediates / escape args
    call_class: CallClassification,
    /// Arena for looking up binding metadata (captures, names)
    arena: *const BindingArena,
    /// Binding → Lambda HIR node for inlining at Call sites.
    /// Populated when a Let/Letrec binds a Lambda. Inlining lets
    /// the walk see intrinsics (push/put/pair) inside known lambda
    /// bodies and emit the corresponding cross-region edges at the call
    /// site.
    binding_lambda: HashMap<Binding, *const Hir>,
    /// Depth counter to prevent infinite recursion during inlining.
    inline_depth: u32,
    /// Regions currently bound to an inlined callee's params, that is, the CALLER's
    /// arg regions, live across an active `try_inline_call`. A `Return` reached
    /// during an inline re-walk names whatever `binding_regions` its value
    /// resolves to, and for a param that is one of these caller regions. Pushing
    /// it into `return_sites` would extend the caller region's `decref_point` to
    /// a node inside the callee body. Take a self-tail-recursive callee whose
    /// tail call transfers its accumulator arg forward (stdlib `fold`'s `go`).
    /// There the extension pins the arg's release onto the base-case (sibling)
    /// arm. Under self-tail-call frame reuse, the branch-union release then
    /// over-frees the value the tail call already moved into the next
    /// accumulator. The caller's own structural walk owns an arg region's release
    /// (including its own `return_sites` if the caller returns it), so the
    /// `Return` and `Break` arms filter these out of `return_sites` and
    /// `block_break_regions`. Empty outside an inline.
    inline_bound_regions: rustc_hash::FxHashSet<Region>,
    /// Lambda nesting depth: the `Lambda` arm and an inline re-walk each add one
    /// around a body. `in_lambda` mirrors the lowerer's own flag, which decides
    /// which bindings take a compiled capture cell at a `Begin`, `Let` or
    /// `Letrec`, and whether a reassigned binding is fn-local.
    in_lambda_depth: u32,
}

impl RegionInference {
    fn new(arena: &BindingArena, call_class: CallClassification) -> Self {
        RegionInference {
            tree: RegionTree::new(),
            alloc_region: HashMap::new(),
            scope_region: HashMap::new(),
            binding_region: HashMap::new(),
            binding_regions: HashMap::new(),
            top_level_reassigns: HashMap::new(),
            captured_reassigns: rustc_hash::FxHashSet::default(),
            local_reassigns: HashMap::new(),
            loop_forwarded_params: HashMap::new(),
            begin_cell_regions: HashMap::new(),
            pattern_rest_regions: HashMap::new(),
            compiled_cell_bindings: rustc_hash::FxHashSet::default(),
            cross_region_refs: Vec::new(),
            hard_edge_sites: rustc_hash::FxHashSet::default(),
            call_result_regions: rustc_hash::FxHashSet::default(),
            counted_cell_read_sites: rustc_hash::FxHashSet::default(),
            binder_init_sites: HashMap::new(),
            fresh_result_regions: rustc_hash::FxHashSet::default(),
            fiber_result_regions: rustc_hash::FxHashSet::default(),
            mutable_container_regions: rustc_hash::FxHashSet::default(),
            containment_edges: Vec::new(),
            funnel_store_sites: HashMap::new(),
            funnel_bytecopy_value_sites: HashMap::new(),
            emit_payload_regions: HashMap::new(),
            funnel_container_sites: HashMap::new(),
            funnel_passthrough_sites: HashMap::new(),
            uncounted_read_sites: HashMap::new(),
            counted_read_aliases: Vec::new(),
            opaque_result_aliases: Vec::new(),
            funnel_result_containers: Vec::new(),
            moves_out_release_sites: rustc_hash::FxHashSet::default(),
            cell_release_regions: rustc_hash::FxHashSet::default(),
            destructure_leaf_bindings: rustc_hash::FxHashSet::default(),
            return_sites: Vec::new(),
            destructure_sites: Vec::new(),
            block_regions: HashMap::new(),
            block_break_regions: HashMap::new(),
            block_break_nodes: HashMap::new(),
            break_skip_blocks: Vec::new(),
            break_sites: Vec::new(),
            frame_replacing_tail_calls: rustc_hash::FxHashSet::default(),
            next_region: 1, // 0 is the reserved sentinel — never assigned to an allocation
            current_region: Region(0),
            call_class,
            arena: arena as *const BindingArena,
            binding_lambda: HashMap::new(),
            inline_depth: 0,
            inline_bound_regions: rustc_hash::FxHashSet::default(),
            in_lambda_depth: 0,
        }
    }

    fn in_lambda(&self) -> bool {
        self.in_lambda_depth > 0
    }

    fn arena(&self) -> &BindingArena {
        // SAFETY: the arena outlives RegionInference (`analyze_regions_with`
        // borrows it for the inference's whole life).
        unsafe { &*self.arena }
    }

    fn fresh_region(&mut self, parent: Region) -> Region {
        let r = Region(self.next_region);
        self.next_region += 1;
        self.tree.add_child(r, parent);
        r
    }

    /// Record an allocation at `hir_id`: assign it a fresh, unique
    /// region parented at `current_region`. Returns the new region.
    /// Every structural visit produces a new region — no merging at this layer.
    ///
    /// IDEMPOTENT UNDER INLINED RE-WALK. `try_inline_call` re-walks an
    /// inlinable callee's body to discover cross-region edges at the call site
    /// (`inline_depth > 0`); the body's HIR nodes are then visited a SECOND time
    /// with the SAME ids as the structural walk. The structural walk OWNS
    /// `alloc_region`. A fresh mint here during a re-walk would OVERWRITE the
    /// structural entry with a region parented in the caller's context. The
    /// compensation and merge passes read escape's return frontier *projected
    /// through* `alloc_region`, so a clobbered entry desyncs the projection from
    /// the lowerer's `alloc_region`. Take a body whose tail allocation is reached
    /// in a discarding caller context. The lowerer then emits a discarded-result
    /// `DecrefValueRegion` INSIDE the closure body, and the closure frees the value
    /// it returns. Reuse the structural region instead. Edge discovery — the
    /// re-walk's only purpose — is unaffected: edges bind to the value's real
    /// (structural) region. Mirrors `env_cell_placeholder`'s re-walk idempotency.
    /// The structural walk (`inline_depth == 0`) visits every node, so its entry
    /// is the one that stands.
    fn alloc_here(&mut self, hir_id: HirId) -> Region {
        if self.inline_depth > 0 {
            if let Some(&r) = self.alloc_region.get(&hir_id) {
                return r;
            }
        }
        let r = self.fresh_region(self.current_region);
        self.alloc_region.insert(hir_id, r);
        r
    }
}

mod analyze;
mod arms;
mod build;
mod capture;
mod compensate;
mod container;
mod edges;
mod escape;
mod format;
mod holders;
mod join;
mod letrec;
mod merge;
mod ownership;
mod placeholder;
mod postdom;
mod tree;
mod walk;
mod yieldborrow;

pub use analyze::{analyze_regions, analyze_regions_with};
pub use format::format_regions;
// The escape→region projection (`region::infer::escape`), reused by the escape dump.
pub(crate) use escape::return_frontier_regions;

#[cfg(test)]
mod tests;
