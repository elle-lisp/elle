// audited: 2026-10-14
//! Tofte-Talpin region inference for functional HIR: the walk's state, and how it mints a region.
//!
//! docs/impl/region/model.md
//!
//! A single forward walk assigns every allocation its own unique region, with no
//! constraint solver and no merging at this layer. The types this produces live in
//! the parent module. What the walk RECORDS against that state is a submodule
//! per subject; the walk itself is `walk`, and the post-passes are `analyze`.

use super::super::arena::BindingArena;
use super::super::binding::Binding;
use super::super::defuse::DefUseBuilder;
use super::super::expr::{Hir, HirId, HirKind};
use super::super::liveness::{compute_last_use, compute_order, Order};
use super::super::pattern::HirPattern;
use super::{
    CallClassification, CellStores, Region, RegionData, RegionInfo, RegionStats, RestCollection,
};

use std::collections::HashMap;
use tree::RegionTree;

// ── Region inference walk (unique-per-alloc) ─────────────────────

struct RegionInference {
    tree: RegionTree,
    /// HirId → unique region assigned to that allocation site. The structural
    /// walk's `alloc_here` inserts one fresh entry per site; an inline re-walk
    /// reuses it.
    alloc_region: HashMap<HirId, Region>,
    /// HirId → region for scope nodes (Let, Letrec, Loop, Block,
    /// Lambda body, non-suspending While). Feeds the `live_regions`
    /// computation in `build_info`.
    scope_region: HashMap<HirId, Region>,
    /// Binding → region where the binding was defined (scope region).
    binding_region: HashMap<Binding, Region>,
    /// Binding → set of source regions a Var(b) reference may produce.
    /// Var(b) returns `binding_regions[b]` to propagate value flow. Empty when
    /// the walk knows of no heap value the binding holds.
    binding_regions: HashMap<Binding, Vec<Region>>,
    /// Binding → its stores, each an assign/set-cell site with the regions of
    /// the value stored there. Recorded for a reassigned MODULE-SCOPE non-capture
    /// binding: one outside every lambda, or a file-scope binding
    /// (`is_file_scope`) inside the whole-module thunk. Drives
    /// the mutable-reassign decref placement in `analyze_regions_with`.
    top_level_reassigns: HashMap<Binding, CellStores>,
    /// Reassigned CAPTURED (`needs_capture`) bindings, whatever scope the write
    /// sits in. They are excluded from `top_level_reassigns` and
    /// `local_reassigns`, because `handle_make_capture` and
    /// `handle_update_capture` own the cell's RC, not the 1-slot model. The
    /// lowerer still needs to know them: a reassigned cell's content CHANGES.
    /// So the init value's alloc reference is dropped at the define off its own
    /// register, NOT routed through the cell slot, which a later reassignment
    /// has repointed. See `Lowerer::store_captured_cell_init`; pinned by
    /// tests/impl/region-capture-cell-reassign-uaf.lisp.
    captured_reassigns: rustc_hash::FxHashSet<Binding>,
    /// Binding → (assign/set-cell sites, value regions stored) for a FN-LOCAL
    /// non-capture binding that is reassigned: inside a lambda, and not
    /// file-scope. Same 1-slot-container model as `top_level_reassigns`, but the
    /// post-pass suppresses ONLY the init region's decref. A fn-local cell's
    /// final value is freed at the binding's scope-exit `decref_point`, not held
    /// as a program-lifetime root, so its assign-value decrefs must stay. The
    /// cell's counted reference comes from
    /// `lower_assign`'s incref-on-store; drop-on-overwrite releases the priors,
    /// the first overwrite releases the (decref-suppressed) init. Without this,
    /// the cell slot holds an UNCOUNTED reference yet still receives a scope-exit
    /// `DecrefValueRegion`, one decref too many for the final value.
    local_reassigns: HashMap<Binding, CellStores>,
    /// Loop parameter → the binding its init `Var` forwards from. Functionalization
    /// rewrites a `while` that assigns a binding into a `Loop` whose parameter is a
    /// fresh version of that binding, initialized from the pre-loop version and
    /// standing in for it at every later read. Both versions therefore record the
    /// same source regions while holding ONE reference between them (a `Var` read
    /// mints nothing), which the reassign 1-slot gate's sole-held check must not
    /// read as two holders of one name. See `RegionHolders::with_aliases` and
    /// docs/impl/region/bindings.md. An entry is recorded only for an
    /// init that is a bare `Var`; any other init expression is a real value the
    /// parameter does not merely carry forward.
    loop_forwarded_params: HashMap<Binding, Binding>,
    /// Scope HirId (`Begin`, `Let` or `Letrec`) → per-binding region for each
    /// compiled capture cell the lowerer mints there, one region PER CELL. See
    /// `RegionInfo::begin_cell_regions`.
    begin_cell_regions: HashMap<HirId, Vec<(Binding, Region)>>,
    /// `Destructure`/`Match` HirId → one entry per collection the node's
    /// pattern BUILDS (see `RegionInfo::pattern_rest_regions`).
    pattern_rest_regions: HashMap<HirId, Vec<RestCollection>>,
    /// Every binding a scope arm (`Begin`, `Let`, `Letrec`) minted a COMPILED
    /// cell for. Its forward
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
    /// Regions no static slot names: call, `Emit` and `Eval` results, and the
    /// placeholders minted for values with no compiled allocation. A placeholder
    /// stands for a parameter, a rest collection, an env cell or a counted cell
    /// read. The lowerer releases these by value
    /// (`DecrefValueRegion`, or `DecrefCellRegion` for `cell_release_regions`)
    /// rather than by `DecrefRegion` at `decref_point`.
    call_result_regions: rustc_hash::FxHashSet<Region>,
    /// Binding-init HirIds that are a whole-value read of a 1-slot container, a
    /// reassigned binding with or without a cell: the reader half of the
    /// container. See
    /// `RegionInfo::counted_cell_read_sites`.
    counted_cell_read_sites: rustc_hash::FxHashSet<HirId>,
    /// Binding → the HirId of the init a BINDER stores into its slot
    /// (`Let`/`Letrec`/`Define`). The reassign gate reads this to place the
    /// counted-init retain of a 1-slot container whose init value is aliased.
    /// The retain has to sit just ahead of that store, where the value is on
    /// the operand stack. A binding whose value arrives some other way — a
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
    /// containment_edges` and the `Funnel` arm in `region::infer::walk`.
    mutable_container_regions: rustc_hash::FxHashSet<Region>,
    /// Structural containment edges `(site, contained, container)` for the ownership
    /// inference. A `Funnel` store into a mutable retaining container gives
    /// `container ⊇ value`, recovered from the container's `RetType`. A `Fresh`
    /// native's declared **embed** gives `result ⊇ embedded_arg`, from `call_embeds`,
    /// for example `with-traits`'s trait side-field. Both are site-keyed exactly like
    /// `cross_region_refs`, so the forest can hang a value-resolved `AdoptRegion` on the
    /// call (the funnel store face). Neither drives an `IncrefRegion`: the funnel
    /// counts the store at runtime, and the alloc-scan counts the embedding. See
    /// `RegionInfo::containment_edges`.
    containment_edges: Vec<(HirId, Region, Region)>,
    /// Retaining funnel-store call site (`%put`/`%array-push`/…) → the regions of
    /// the value stored there, its last argument, less the container's own
    /// regions. The runtime funnel increfs it, so a value stored at such a site has its RC
    /// raised regardless of whether the container's type is statically known
    /// (unlike `containment_edges`, which needs a recognized
    /// `mutable_container_regions` container). Read by `region::infer::compensate` to
    /// bound a per-arm decref to a node where the value is provably re-incref'd —
    /// the only place a sibling-arm release cannot over-free. See
    /// `RegionInfo::funnel_store_sites`.
    funnel_store_sites: HashMap<HirId, Vec<Region>>,
    /// Byte-copy funnel call site → the stored value's regions. See
    /// `RegionInfo::funnel_bytecopy_value_sites`.
    funnel_bytecopy_value_sites: HashMap<HirId, Vec<Region>>,
    /// `Emit` (`yield`/`emit`) site → the regions its payload may live in. See
    /// `RegionInfo::emit_payload_regions`.
    emit_payload_regions: HashMap<HirId, Vec<Region>>,
    /// Monomorphic store/remove funnel call site → the CONTAINER argument (arg0)
    /// regions, recorded when the declared return is a recognized container type
    /// of either mutability. A dispatch wrapper uses its container in every arm
    /// while its one release sits in a single arm, stranding the owned-param
    /// reference on the others; `region::infer::compensate` places a per-arm
    /// release there. See `RegionInfo::funnel_container_sites`.
    funnel_container_sites: HashMap<HirId, Vec<Region>>,
    /// The `-mut` PASS-THROUGH subset of `funnel_container_sites` (the funnel returns
    /// arg0 in place). Gates the lowerer's ReturnValue suppression to the case where
    /// the result IS the owned container; an immutable fresh result keeps its retain.
    /// See `RegionInfo::funnel_passthrough_sites`.
    funnel_passthrough_sites: HashMap<HirId, Vec<Region>>,
    /// UNCOUNTED container element-READ site (`%get`/`%first`/`%rest`, inline opcodes
    /// that raise no reference count) → the CONTAINER (arg0) regions the read borrows out
    /// of. The container's own lifetime is what keeps the borrow alive, so its release
    /// must follow the READER's (region/rules.md Rule 4, the borrowing node). See
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
    /// to the Return node, so the region's `DecrefRegion` is emitted
    /// *after* the node's `IncrefValueRegion`. The retain must precede the
    /// callee's own release of a freshly-allocated result region, or the result
    /// is freed before it is handed back.
    return_sites: Vec<(HirId, Vec<Region>)>,
    /// `(destructure_node_id, regions of the destructured value)` for every
    /// `HirKind::Destructure`. A Destructure is a *consuming node*: its
    /// field extraction reads the value AFTER the value expression's own
    /// last read, so the post-pass extends each region's `decref_point` to
    /// the Destructure node (docs/impl/region/rules.md Rule 4). Without it, a
    /// destructure whose bindings are all unused anchors the value's
    /// release at the inner read, and the extraction reads freed pages.
    /// Pinned by tests/impl/region-named-param-uaf.lisp.
    destructure_sites: Vec<(HirId, Vec<Region>)>,
    /// BlockId → enclosing region at the point the block was entered.
    /// The `Block` arm writes it, and nothing reads it.
    block_regions: HashMap<super::super::expr::BlockId, Region>,
    /// BlockId → the regions of every `break` value handed to that block, in
    /// walk order. A `break` TRANSFERS its value to the block, whose value is
    /// its fall-through value OR any break's. So the `Block` arm unions these
    /// into its own result regions and clears the entry
    /// (docs/impl/region/anchors.md). Without the union, a binding named to the
    /// block's value holds NO region, and the binding-chain `decref_point`
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
    /// `Block` node HirId → the HirIds of the breaks targeting it. The post-pass
    /// re-anchors onto the block every region whose `decref_point` falls in the
    /// window those breaks jump over, from the earliest break site to the exit
    /// label. A release emitted in that window never runs on the break path
    /// (docs/impl/region/anchors.md).
    break_skip_blocks: Vec<(HirId, Vec<HirId>)>,
    /// `Block` node HirId → the regions every targeting `break` hands it. The
    /// dual of `return_sites`: a `Break` is a *transferring* node. So the
    /// post-pass extends each broken region's `decref_point` to where the
    /// BLOCK's value is consumed, `last_use[block]`. That is the block itself
    /// when nothing consumes it, and the lowerer emits its decrefs after the exit
    /// label. A release left inside the body is jumped over and never runs
    /// (docs/impl/region/rules.md Rule 4).
    break_sites: Vec<(HirId, Vec<Region>)>,
    /// HirIds of the tail `Call`s whose callee may be a bytecode closure — the
    /// calls that REPLACE this frame instead of falling through. A tail call to a
    /// native pushes no frame and is absent here. The branch-arm release window
    /// declines any branch containing one, since its merge label is then not a
    /// point every arm reaches (docs/impl/region/window.md).
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
    /// Populated when a Let or Letrec binds a Lambda. Inlining lets
    /// the walk see intrinsics (push/put/pair) inside known lambda
    /// bodies and emit the corresponding cross-region edges at the call
    /// site.
    binding_lambda: HashMap<Binding, *const Hir>,
    /// Inline nesting depth. `try_inline_call` stops at four levels.
    inline_depth: u32,
    /// The lambdas whose bodies the walk is inside, structurally or through an
    /// inline. A call to one of them reaches a new activation of a body whose
    /// allocation sites this walk already names, so it is not inlined
    /// (docs/impl/region/mechanism.md).
    walking_lambdas: Vec<*const Hir>,
    /// Regions currently bound to an inlined callee's params, that is the CALLER's
    /// arg regions, live across an active `try_inline_call`. A `Return` reached
    /// during an inline re-walk names whatever `binding_regions` its value
    /// resolves to. When that is a param, it is one of these caller regions.
    /// Pushing it into `return_sites` would extend the caller region's
    /// `decref_point` to a node inside the callee body. The caller's own
    /// structural walk owns an arg region's release, including its own
    /// `return_sites` if the caller returns it, so the inline filters these out
    /// (docs/impl/region/mechanism.md). Empty outside an inline.
    inline_bound_regions: rustc_hash::FxHashSet<Region>,
    /// Lambda nesting depth, raised around a lambda body's walk and around an
    /// inline's re-walk of a callee body. `in_lambda()` mirrors the lowerer's
    /// own `in_lambda`: it decides which compiled capture cells a scope arm
    /// mints, whether a captured binding takes an env cell placeholder, and
    /// whether a reassigned binding is module-scope.
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
            walking_lambdas: Vec::new(),
            inline_bound_regions: rustc_hash::FxHashSet::default(),
            in_lambda_depth: 0,
        }
    }

    fn in_lambda(&self) -> bool {
        self.in_lambda_depth > 0
    }

    fn arena(&self) -> &BindingArena {
        // SAFETY: the arena is borrowed for the whole `analyze_regions_with` call,
        // and RegionInference lives only inside that call.
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
    /// ownership and compensation passes read escape's return frontier projected
    /// through `alloc_region`. A clobbered entry desyncs that projection from the
    /// lowerer's `alloc_region`, and a closure can then release the value it
    /// returns (pinned by tests/impl/region-loop-local-closure-tail-uaf.lisp).
    /// Reuse the structural region instead. Edge discovery, the re-walk's only
    /// purpose, is unaffected: edges bind to the value's real (structural)
    /// region. `env_cell_placeholder` (in `placeholder`) is idempotent under a
    /// re-walk the same way. The structural walk (`inline_depth == 0`) visits
    /// every node and is the sole writer.
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
mod letrec;
mod merge;
mod ownership;
mod placeholder;
mod postdom;
mod restlist;
mod tree;
mod walk;
mod yieldborrow;

pub use analyze::{analyze_regions, analyze_regions_with};
pub use format::format_regions;
// The escape→region projection (`region::infer::escape`), reused by the escape dump.
pub(crate) use escape::return_frontier_regions;

#[cfg(test)]
mod tests;
