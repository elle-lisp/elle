// audited: 2026-09-15
//! Tofte-Talpin region inference for functional HIR: the walk's state, and how
//! it mints a region.
//!
//! docs/impl/region/model.md
//!
//! A single forward walk assigns every allocation its own unique region — no
//! constraint solver, no merging at this layer. The types this produces live in
//! the parent module. What the walk RECORDS against that state is a submodule
//! per subject; the walk itself is `walk`, and the post-passes are `analyze`.

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
    /// HirId → unique region assigned to that allocation site.
    /// Every alloc_here() call inserts a fresh entry here.
    alloc_region: HashMap<HirId, Region>,
    /// HirId → region for scope nodes (Let, Letrec, Loop, Block,
    /// Lambda body, non-suspending While). Feeds the `live_regions`
    /// computation in `build_info`.
    scope_region: HashMap<HirId, Region>,
    /// Binding → region where the binding was defined (scope region).
    binding_region: HashMap<Binding, Region>,
    /// Binding → set of source regions a Var(b) reference may produce.
    /// Empty for opaque bindings (params, pattern bindings).
    /// Var(b) returns `binding_regions[b]` to propagate value flow.
    binding_regions: HashMap<Binding, Vec<Region>>,
    /// Binding → its stores (each an assign/set-cell site with the regions of
    /// the value stored there) for a TOP-LEVEL (file-letrec,
    /// `in_lambda_depth == 0`), non-capture binding that is reassigned. Drives
    /// the mutable-reassign decref placement in `analyze_regions_with`.
    top_level_reassigns: HashMap<Binding, CellStores>,
    /// Top-level CAPTURED (`needs_capture`, outside a lambda) bindings that are
    /// reassigned — the `@x`-boxed-in-a-`MakeCaptureCell`-and-reassigned class.
    /// These are excluded from `top_level_reassigns` (the cell's RC is owned by
    /// `handle_make_capture`/`handle_update_capture`, not the 1-slot model), but
    /// the lowerer still needs to know: a reassigned cell's content CHANGES, so
    /// the init value's alloc reference must be dropped at the define off its own
    /// register, NOT routed through the cell slot (which a later reassignment has
    /// repointed). See `Lowerer::store_captured_cell_init` and
    /// region-capture-cell-reassign-uaf.lisp.
    captured_reassigns: rustc_hash::FxHashSet<Binding>,
    /// Binding → (assign/set-cell sites, value regions stored) for an
    /// IN-LAMBDA (fn-local, `in_lambda_depth > 0`), non-capture binding that is
    /// reassigned. Same 1-slot-container model as `top_level_reassigns`, but the
    /// post-pass suppresses ONLY the init region's decref, not the assign-value
    /// regions': a fn-local cell's final value is freed at the binding's
    /// scope-exit `decref_point` (it is not a program-lifetime root), so its
    /// assign-value decrefs must stay. The cell's counted reference comes from
    /// `lower_assign`'s incref-on-store; drop-on-overwrite releases the priors,
    /// the first overwrite releases the (decref-suppressed) init. Without this,
    /// the cell slot holds an UNCOUNTED reference yet still receives a scope-exit
    /// `DecrefValueRegion`, one decref too many for the final value → the
    /// fn-local mutable-reassign double-free (`fn/cfg … :mermaid`).
    local_reassigns: HashMap<Binding, CellStores>,
    /// Loop parameter → the binding its init `Var` forwards from. Functionalization
    /// rewrites a `while` that assigns a binding into a `Loop` whose parameter is a
    /// fresh version of that binding, initialized from the pre-loop version and
    /// standing in for it at every later read. Both versions therefore record the
    /// same source regions while holding ONE reference between them (a `Var` read
    /// mints nothing), which the reassign 1-slot gate's sole-held check must not
    /// read as two holders of one name — see `RegionHolders::with_aliases` and
    /// docs/impl/region/bindings.md § "The gate". An entry is recorded only for an
    /// init that is a bare `Var`; any other init expression is a real value the
    /// parameter does not merely carry forward.
    loop_forwarded_params: HashMap<Binding, Binding>,
    /// Begin HirId → per-binding region for each pre-allocated capture cell
    /// (mirrors `lower_begin`'s MakeCaptureCell pre-pass; one region PER CELL —
    /// see `RegionInfo::begin_cell_regions`).
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
    /// Regions whose `alloc_here` happened at a Call HirId. Lowerer
    /// uses these to choose `DecrefValueRegion(reg)` over
    /// `DecrefRegion(rid)` at `decref_point`.
    call_result_regions: rustc_hash::FxHashSet<Region>,
    /// Binding-init HirIds that are a whole-value read of a reassigned captured
    /// cell — the reader half of the 1-slot container. See
    /// `RegionInfo::counted_cell_read_sites`.
    counted_cell_read_sites: rustc_hash::FxHashSet<HirId>,
    /// Binding → the HirId of the init a BINDER stores into its slot
    /// (`Let`/`Letrec`/`Define`). The reassign gate reads this to place the
    /// counted-init retain of a 1-slot container whose init value is aliased:
    /// the retain has to sit where the value is on the operand stack, just ahead
    /// of that store. A binding whose value arrives some other way — a
    /// parameter, a `Loop` parameter's forwarding init — has no entry, and the
    /// gate keeps donate-or-refuse for it (docs/impl/region/bindings.md § "What
    /// the cell donates it must hold alone; what it counts it need not"). The
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
    /// inference, from two sources: a `Funnel` store into a mutable retaining container
    /// (`container ⊇ value`, recovered from the container's `RetType`), and a `Fresh`
    /// native's declared **embed** (`result ⊇ embedded_arg`, from `call_embeds` — e.g.
    /// `with-traits`'s trait side-field). Both are site-keyed exactly like
    /// `cross_region_refs`, so the forest can hang a value-resolved `AdoptRegion` on the
    /// call (the funnel store face), and both drive NO `IncrefRegion` (the funnel
    /// counts the store at runtime; the alloc-scan counts the embedding), feeding only
    /// the ownership inference. See `RegionInfo::containment_edges`.
    containment_edges: Vec<(HirId, Region, Region)>,
    /// Funnel-store call site → the regions of the heap values stored there (the
    /// non-container args of a `Funnel` intrinsic — `%put`/`%array-push`/…). The
    /// runtime funnel increfs each, so a value stored at such a site has its RC
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
    /// Pass-through funnel-store call site → the CONTAINER argument (arg0) regions,
    /// recorded only for a `-mut` store whose declared return is a mutable container
    /// (the funnel returns arg0 in place). A dispatch wrapper's mutable arm returns
    /// this container pass-through, stranding the owned-param reference the wrapper
    /// holds; `region::infer::compensate` places a per-arm release there. See
    /// `RegionInfo::funnel_container_sites`.
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
    /// to the Return node so the region's `DecrefRegion` is emitted
    /// *after* the node's `IncrefValueRegion` (the retain must precede
    /// the callee's own release of a freshly-allocated result region —
    /// otherwise the result is freed before it is handed back).
    return_sites: Vec<(HirId, Vec<Region>)>,
    /// `(destructure_node_id, regions of the destructured value)` for every
    /// `HirKind::Destructure`. A Destructure is a *consuming node*: its
    /// field extraction reads the value AFTER the value expression's own
    /// last read, so the post-pass extends each region's `decref_point` to
    /// the Destructure node (docs/impl/region/rules.md Rule 4). Without it, a
    /// destructure whose bindings are all unused anchors the value's
    /// release at the inner read and the extraction reads freed pages (the
    /// `&named`-param prologue UAF, region-named-param-uaf.lisp).
    destructure_sites: Vec<(HirId, Vec<Region>)>,
    /// BlockId → enclosing region at the point the block was entered.
    /// Reserved for tooling; the region walk does not read it.
    block_regions: HashMap<super::super::expr::BlockId, Region>,
    /// BlockId → the regions of every `break` value handed to that block, in
    /// walk order. A `break` TRANSFERS its value to the block — the block's
    /// value is its fall-through value OR any break's — so the `Block` arm
    /// unions these into its own result regions and clears the entry
    /// (docs/impl/region/mechanism.md § "`break` transfers its value; it does
    /// not consume it"). Without the union, a binding named to the block's
    /// value holds NO region, the binding-chain `decref_point` extension never
    /// sees the broken value, and its release stays at the block's exit label —
    /// under every later read of the result. Drained at the `Block` node into
    /// `break_sites`.
    block_break_regions: HashMap<super::super::expr::BlockId, Vec<Region>>,
    /// BlockId → the HirId of every `break` targeting that block, in walk order.
    /// Recorded for EVERY break, valueless and immediate-valued ones included —
    /// unlike `block_break_regions`, which only sees breaks that carry a region.
    /// What the post-pass needs from a break is its *position*: the jump to the
    /// exit label passes over every release from the break site onward, whatever
    /// the break carries. Drained at the `Block` node into `break_skip_blocks`.
    block_break_nodes: HashMap<super::super::expr::BlockId, Vec<HirId>>,
    /// `Block` node HirId → the HirIds of the breaks targeting it. The post-pass
    /// re-anchors every region whose `decref_point` falls in the window those
    /// breaks jump over — from the earliest break site to the exit label — onto
    /// the block, since a release emitted there never runs on the break path
    /// (docs/impl/region/mechanism.md § "A release the break jumps over is not a
    /// release").
    break_skip_blocks: Vec<(HirId, Vec<HirId>)>,
    /// `Block` node HirId → the regions every targeting `break` hands it. The
    /// dual of `return_sites`: a `Break` is a *transferring* node, so the
    /// post-pass extends each broken region's `decref_point` to where the
    /// BLOCK's value is consumed (`last_use[block]` — the block itself when
    /// nothing consumes it, whose decrefs the lowerer emits after the exit
    /// label). A release left inside the body is jumped over and never runs
    /// (docs/impl/region/rules.md Rule 4).
    break_sites: Vec<(HirId, Vec<Region>)>,
    /// HirIds of the tail `Call`s whose callee may be a bytecode closure — the
    /// calls that REPLACE this frame instead of falling through. A tail call to a
    /// native pushes no frame and is absent here. The branch-arm release window
    /// declines any branch containing one, since its merge label is then not a
    /// point every arm reaches (docs/impl/region/mechanism.md § "A release inside
    /// one arm is not a release on the other arms").
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
    /// Populated when a Let/Letrec/Define binds a Lambda. Inlining lets
    /// the walk see intrinsics (push/put/pair) inside known lambda
    /// bodies and emit the corresponding cross-region edges at the call
    /// site.
    binding_lambda: HashMap<Binding, *const Hir>,
    /// Depth counter to prevent infinite recursion during inlining.
    inline_depth: u32,
    /// Regions currently bound to an inlined callee's params — i.e. the CALLER's
    /// arg regions, live across an active `try_inline_call`. A `Return` reached
    /// during an inline re-walk names whatever `binding_regions` its value
    /// resolves to; when that is a param, it is one of these caller regions, and
    /// pushing it into `return_sites` would extend the caller region's
    /// `decref_point` to a node inside the callee body. For a self-tail-recursive
    /// callee whose accumulator arg the tail call transfers forward (stdlib
    /// `fold`'s `go`), that pins the arg's release onto the base-case (sibling)
    /// arm, and under self-tail-call frame reuse the branch-union release
    /// over-frees the value the tail call already moved into the next
    /// accumulator. The caller's own structural walk owns an arg region's release
    /// (including its own `return_sites` if the caller returns it), so the inline
    /// filters these out — while still propagating the callee's genuine
    /// body-result regions, which the call site needs. Empty outside an inline.
    inline_bound_regions: rustc_hash::FxHashSet<Region>,
    /// Lambda nesting depth — incremented around lambda body walks.
    /// Used to mirror the lowerer's `!self.in_lambda` predicate: inside
    /// a lambda body, MakeCaptureCell is not emitted by `lower_begin` /
    /// `lower_letrec` (the VM materializes cells via the closure-
    /// construction path), so the regions walker must not register an
    /// alloc_region for Begin/Letrec inside a lambda either.
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
        // SAFETY: the arena outlives RegionInference (both created in analyze_regions)
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
    /// ownership/compensation passes read escape's return frontier *projected
    /// through* `alloc_region`, so a clobbered entry desyncs the projection from
    /// the lowerer's `alloc_region`: for a body whose tail allocation is reached in
    /// a discarding caller context, the lowerer emits a discarded-result
    /// `DecrefValueRegion` INSIDE the closure body — the closure frees the value it
    /// returns and the caller's release derefs freed memory (the stale-region-deref
    /// UAF; tests/elle/region-loop-local-closure-tail-uaf.lisp). Reuse the
    /// structural region instead. Edge discovery — the re-walk's only purpose — is
    /// unaffected: edges bind to the value's real (structural) region. Mirrors
    /// `env_cell_placeholder`'s re-walk idempotency below. The structural walk
    /// (`inline_depth == 0`) always visits every node and is the sole writer.
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
mod tree;
mod walk;
mod yieldborrow;

pub use analyze::{analyze_regions, analyze_regions_with};
pub use format::format_regions;
// The escape→region projection (`region::infer::escape`), reused by the escape dump.
pub(crate) use escape::return_frontier_regions;

#[cfg(test)]
mod tests;
