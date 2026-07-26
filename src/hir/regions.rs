//! Tofte-Talpin region inference for functional HIR.
//!
//! A single forward walk assigns every allocation its own unique region —
//! no constraint solver, no merging at this layer. See `region.rs` for types.

use super::arena::BindingArena;
use super::binding::Binding;
use super::defuse::DefUseBuilder;
use super::expr::{Hir, HirId, HirKind};
use super::liveness::{compute_last_use, compute_order, compute_subtree_low};
use super::region::{CallClassification, Region, RegionData, RegionInfo, RegionStats};

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
    /// Binding → (assign/set-cell sites, value regions stored) for a TOP-LEVEL
    /// (file-letrec, `in_lambda_depth == 0`), non-capture binding that is
    /// reassigned. Drives the mutable-reassign decref placement in
    /// `analyze_regions_with`.
    top_level_reassigns: HashMap<Binding, (Vec<HirId>, Vec<Region>)>,
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
    local_reassigns: HashMap<Binding, (Vec<HirId>, Vec<Region>)>,
    /// Begin HirId → per-binding region for each pre-allocated capture cell
    /// (mirrors `lower_begin`'s MakeCaptureCell pre-pass; one region PER CELL —
    /// see `RegionInfo::begin_cell_regions`).
    begin_cell_regions: HashMap<HirId, Vec<(Binding, Region)>>,
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
    /// containment_edges` and the `Funnel` arm in `regions::walk`.
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
    /// `mutable_container_regions` container). Read by `regions::compensate` to
    /// bound a per-arm decref to a node where the value is provably re-incref'd —
    /// the only place a sibling-arm release cannot over-free. See
    /// `RegionInfo::funnel_store_sites`.
    funnel_store_sites: HashMap<HirId, Vec<Region>>,
    /// Byte-copy funnel call site → the stored value's regions. See
    /// `RegionInfo::funnel_bytecopy_value_sites`.
    funnel_bytecopy_value_sites: HashMap<HirId, Vec<Region>>,
    /// Pass-through funnel-store call site → the CONTAINER argument (arg0) regions,
    /// recorded only for a `-mut` store whose declared return is a mutable container
    /// (the funnel returns arg0 in place). A dispatch wrapper's mutable arm returns
    /// this container pass-through, stranding the owned-param reference the wrapper
    /// holds; `regions::compensate` places a per-arm release there. See
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
    /// Call sites of a moves-out ∩ PassThrough native (`%pop`/`%pop-array*`) whose
    /// moved-out element is escape-retained in-body. See
    /// `RegionInfo::moves_out_release_sites`.
    moves_out_release_sites: rustc_hash::FxHashSet<HirId>,
    /// Subset of `call_result_regions` that are capture-cell placeholders for
    /// captured (env-allocated) bindings — released with `DecrefCellRegion`
    /// (`region_of` the cell), not `DecrefValueRegion` (`result_region_of` the
    /// inner value). See `RegionInfo::cell_release_regions`.
    cell_release_regions: rustc_hash::FxHashSet<Region>,
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
    block_regions: HashMap<super::expr::BlockId, Region>,
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
    block_break_regions: HashMap<super::expr::BlockId, Vec<Region>>,
    /// BlockId → the HirId of every `break` targeting that block, in walk order.
    /// Recorded for EVERY break, valueless and immediate-valued ones included —
    /// unlike `block_break_regions`, which only sees breaks that carry a region.
    /// What the post-pass needs from a break is its *position*: the jump to the
    /// exit label passes over every release from the break site onward, whatever
    /// the break carries. Drained at the `Block` node into `break_skip_blocks`.
    block_break_nodes: HashMap<super::expr::BlockId, Vec<HirId>>,
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
            begin_cell_regions: HashMap::new(),
            cross_region_refs: Vec::new(),
            hard_edge_sites: rustc_hash::FxHashSet::default(),
            call_result_regions: rustc_hash::FxHashSet::default(),
            counted_cell_read_sites: rustc_hash::FxHashSet::default(),
            fresh_result_regions: rustc_hash::FxHashSet::default(),
            fiber_result_regions: rustc_hash::FxHashSet::default(),
            mutable_container_regions: rustc_hash::FxHashSet::default(),
            containment_edges: Vec::new(),
            funnel_store_sites: HashMap::new(),
            funnel_bytecopy_value_sites: HashMap::new(),
            funnel_container_sites: HashMap::new(),
            funnel_passthrough_sites: HashMap::new(),
            uncounted_read_sites: HashMap::new(),
            counted_read_aliases: Vec::new(),
            moves_out_release_sites: rustc_hash::FxHashSet::default(),
            cell_release_regions: rustc_hash::FxHashSet::default(),
            return_sites: Vec::new(),
            destructure_sites: Vec::new(),
            block_regions: HashMap::new(),
            block_break_regions: HashMap::new(),
            block_break_nodes: HashMap::new(),
            break_skip_blocks: Vec::new(),
            break_sites: Vec::new(),
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

    /// Mirror of `lower_begin`'s collect_preallocate_bindings: a Begin
    /// emits MakeCaptureCell at its HirId iff some reachable Define or
    /// Destructure binding (reachable via Let/Begin/Loop/Block, NOT via
    /// If/Match/Cond/Lambda) has `needs_capture()` true.
    fn begin_has_capturable_binding(&self, exprs: &[Hir]) -> bool {
        fn walk(arena: &BindingArena, h: &Hir) -> bool {
            match &h.kind {
                HirKind::Define { binding, .. } => arena.get(*binding).needs_capture(),
                HirKind::Destructure { pattern, .. } => pattern
                    .bindings()
                    .bindings
                    .iter()
                    .any(|b| arena.get(*b).needs_capture()),
                HirKind::Lambda { .. } => false,
                HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                    bindings.iter().any(|(_, init)| walk(arena, init)) || walk(arena, body)
                }
                HirKind::Loop { bindings, body } => {
                    bindings.iter().any(|(_, init)| walk(arena, init)) || walk(arena, body)
                }
                HirKind::Begin(es) => es.iter().any(|e| walk(arena, e)),
                HirKind::Block { body, .. } => body.iter().any(|e| walk(arena, e)),
                _ => false,
            }
        }
        exprs.iter().any(|e| walk(self.arena(), e))
    }

    /// Mirror of `lower_begin`'s collect_preallocate_bindings: collect
    /// every Define/Destructure binding reachable via Let/Begin/Loop/Block
    /// (NOT via If/Match/Cond/Lambda) whose `needs_capture()` is true.
    /// Each of these gets a MakeCaptureCell at the Begin's HirId during
    /// lowering, so the Begin's alloc region must outlive each binding's
    /// last use. This populates `binding_regions[b]` with the Begin's
    /// alloc region so the post-pass `decref_point` extension covers them.
    fn collect_begin_capturable_bindings(
        arena: &BindingArena,
        exprs: &[Hir],
        out: &mut Vec<Binding>,
    ) {
        fn walk(arena: &BindingArena, h: &Hir, out: &mut Vec<Binding>) {
            match &h.kind {
                HirKind::Define { binding, .. } if arena.get(*binding).needs_capture() => {
                    out.push(*binding);
                }
                HirKind::Destructure { pattern, .. } => {
                    for b in &pattern.bindings().bindings {
                        if arena.get(*b).needs_capture() {
                            out.push(*b);
                        }
                    }
                }
                HirKind::Lambda { .. } => {}
                HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                    for (_, init) in bindings {
                        walk(arena, init, out);
                    }
                    walk(arena, body, out);
                }
                HirKind::Loop { bindings, body } => {
                    for (_, init) in bindings {
                        walk(arena, init, out);
                    }
                    walk(arena, body, out);
                }
                HirKind::Begin(es) => {
                    for e in es {
                        walk(arena, e, out);
                    }
                }
                HirKind::Block { body, .. } => {
                    for e in body {
                        walk(arena, e, out);
                    }
                }
                _ => {}
            }
        }
        for e in exprs {
            walk(arena, e, out);
        }
    }

    /// Record a reassignment of a non-capture (stack-local) mutable binding,
    /// classifying it as MODULE-SCOPE (`top_level_reassigns` — program/module
    /// extent, the file-letrec mutable class) or FN-LOCAL (`local_reassigns` — a
    /// value that shares its enclosing scope region, freed by scope demise). The
    /// split is by `is_file_scope`, NOT raw `in_lambda`, so a file-letrec mutable
    /// stays module-scope even inside the `%file-body` whole-module thunk. Both
    /// become drop-on-overwrite + suppressed-decref in the post-pass, differing
    /// only in which decrefs are suppressed. Capture-cell bindings (`needs_capture`)
    /// are excluded from both — their RC is owned by `handle_update_capture` /
    /// `handle_store_upvalue` — and recorded in `captured_reassigns` when
    /// module-scope.
    fn record_top_level_reassign(&mut self, b: Binding, site: HirId, val_regions: &[Region]) {
        // MODULE-SCOPE classification, not the raw `in_lambda` flag: a file-letrec
        // (top-level `def`/`var`) binding is program-extent even when the
        // file-letrec runs inside the synthetic `%file-body` whole-module thunk
        // (`compile/whole-module`, where `in_lambda` is spuriously true — the thunk
        // wrapper). Its value's true demise is the file-letrec scope-region
        // teardown, identical to a direct `elle FILE` run, so it must be classified
        // top-level there too. Without this an `elle test` whole-file run routed a
        // reassigned top-level mutable to `local_reassigns`, which keeps the
        // assign-value decrefs; the file-letrec lifts each statement into a dead
        // `__file_expr_N` wrapper whose slot-routed decref then freed the
        // just-stored value while the cell still held it — the
        // `(assign x (pair … x))` UAF (region-toplevel-reassign-thunk-uaf.lisp; the
        // advanced.lisp match-in-loop crash under `elle test`).
        let module_scope = !self.in_lambda() || self.arena().get(b).is_file_scope;
        // Capture-cell bindings are excluded from BOTH container maps: their RC is
        // owned by `handle_update_capture`, not the 1-slot-container model. A
        // module-scope captured reassign is still recorded separately so the lowerer
        // drops the init's alloc reference at the define (the cell content changes;
        // routing that decref through the cell slot is a UAF). A genuine fn-local
        // captured reassign goes through the env-cell (`StoreCapture`) path, not a
        // compiled `MakeCaptureCell`, so it is not in this class.
        if self.arena().get(b).needs_capture() {
            if module_scope {
                self.captured_reassigns.insert(b);
            }
            return;
        }
        // Genuine fn-local reassigns go to `local_reassigns` — same container model,
        // but the post-pass keeps the assign-value decrefs (the cell's final value
        // is freed at scope exit, not a program root). Module-scope reassigns go to
        // `top_level_reassigns` (final value freed by the file-letrec scope-region
        // teardown).
        let map = if module_scope {
            &mut self.top_level_reassigns
        } else {
            &mut self.local_reassigns
        };
        let entry = map.entry(b).or_insert_with(|| (Vec::new(), Vec::new()));
        entry.0.push(site);
        for &r in val_regions {
            if !entry.1.contains(&r) {
                entry.1.push(r);
            }
        }
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

    /// The reader half of a reassigned-captured-cell 1-slot container: when a
    /// binding `reader` is initialised from a WHOLE-VALUE read of an
    /// `is_restorable_capture_cell` binding, give the reader a COUNTED reference
    /// of its own instead of aliasing the cell's value uncounted. The cell's
    /// overwrite (`capture_store_with_rebind`) decrefs the displaced prior
    /// unconditionally, so an uncounted alias is freed under the reader by the
    /// next overwrite — the captured-alias use-after-free
    /// (docs/impl/region/bindings.md § "Captured reassigned cells").
    ///
    /// Realised as Rule 5's "new reference" pass-through: mint a placeholder
    /// region at the read node (it lands in `call_result_regions`, so the reader
    /// carries a value-based `DecrefValueRegion` at its last use) and record the
    /// read site so the lowerer emits the balancing `IncrefValueRegion`. Returns
    /// `[read_r]` for the reader's `binding_regions`, or the unmodified
    /// `init_regions` when the treatment does not apply.
    ///
    /// Applies to BOTH scopes (fn-local upvalue read and module-scope cell read),
    /// which the `is_restorable_capture_cell` predicate covers uniformly. Skipped
    /// when the reader is itself a capture cell — its own store/overwrite
    /// accounting owns its references (the alias-of-a-mutable-by-a-mutable
    /// pairing) — and for an immediate-valued read (no heap reference to count).
    /// Element reads (`first`/`get`/destructure) never reach here: they are not a
    /// bare `Var`/`DerefCell` of the cell, and an element is independently counted
    /// by its parent's alloc-scan (it cascades, never frees under the reader).
    fn counted_cell_read_regions(
        &mut self,
        reader: Binding,
        init: &Hir,
        init_regions: Vec<Region>,
    ) -> Vec<Region> {
        // A whole-value read is a bare `Var(b)`, or the `DerefCell`-wrapped form
        // `functionalize` puts around a needs-capture read (the fn-local upvalue).
        let source = match &init.kind {
            HirKind::Var(b) => *b,
            HirKind::DerefCell { cell } => match &cell.kind {
                HirKind::Var(b) => *b,
                _ => return init_regions,
            },
            _ => return init_regions,
        };
        if init_regions.is_empty()
            || !self.arena().get(source).is_restorable_capture_cell()
            || self.arena().get(reader).needs_capture()
        {
            return init_regions;
        }
        let read_r = self.alloc_here(init.id);
        self.call_result_regions.insert(read_r);
        self.counted_cell_read_sites.insert(init.id);
        vec![read_r]
    }

    /// A captured (`needs_capture`) binding introduced INSIDE a lambda body is
    /// materialized as a per-value env cell by `populate_env` (a `StoreCapture`
    /// into a cell pre-allocated from `capture_locals_mask` — NOT a compiled
    /// `MakeCaptureCell`, which the lowerer emits only at top level / outside a
    /// lambda; see `lower_define`/`lower_let` `self.in_lambda` split). Such an
    /// env cell has no compiled `DecrefRegion`, so without a release its initial
    /// rc=1 leaks (docs/impl/region/rules.md Rule 8 — the env region needs an explicit release).
    ///
    /// Give it a phantom cell placeholder (no `alloc_here` → filtered from
    /// `live_regions`, so no spurious compile-time `IncrefRegion` edge) in
    /// `call_result_regions` + `cell_release_regions`, so the lowerer releases
    /// the CELL at the binding's last use via `LoadCaptureRaw` +
    /// `DecrefCellRegion` (`region_of` the cell, never unwrapping to the inner
    /// value's region). `decref_point` comes from the binding-chains post-pass
    /// over the binding's uses (`binding_regions[b] = [cell_r]`), then the
    /// `hoist_cell_release_past_loops` post-pass lifts it to the outermost
    /// enclosing loop: the box is minted once per activation, so its release
    /// must fire once per activation — a binding-last-use release that sits
    /// inside a loop frees the box on iteration 1 and the next iteration reads
    /// the recycled cell (the env-cell-in-loop UAF; docs/impl/region/bindings.md
    /// "Env cells in loops: release once per activation, not per iteration").
    /// This mirrors the captured-param treatment in the Lambda arm. Returns the
    /// placeholder region iff the binding is such an env cell; `None` for
    /// top-level captured defs (compiled `MakeCaptureCell`, already released)
    /// and non-captured locals.
    fn env_cell_placeholder(&mut self, binding: Binding) -> Option<Region> {
        if self.in_lambda() && self.arena().get(binding).needs_capture() {
            // Idempotent per binding: a captured local is materialized as
            // EXACTLY ONE per-value CaptureCell (`populate_env`), released by a
            // single `DecrefCellRegion` at its last use (docs/impl/region/rules.md Rule 4
            // — "exactly once per activation"). `try_inline_call` re-walks an
            // inlined callee's body to discover cross-region edges, so a
            // captured local's `(var …)` Define inside a *nested* lambda (a
            // generator fiber body returned from an inlined function) can be
            // visited more than once. Minting a second cell-release region here
            // lowers to a second `DecrefCellRegion` for the one cell — a
            // double-free of the CaptureCell's region on resume
            // (region-fiber-capture-cell-resume-uaf.lisp). Reuse the cell
            // region already recorded for this binding instead.
            if let Some(regions) = self.binding_regions.get(&binding).cloned() {
                if let Some(existing) = regions
                    .into_iter()
                    .find(|r| self.cell_release_regions.contains(r))
                {
                    return Some(existing);
                }
            }
            let cell_r = self.fresh_region(self.current_region);
            self.call_result_regions.insert(cell_r);
            self.cell_release_regions.insert(cell_r);
            Some(cell_r)
        } else {
            None
        }
    }

    /// Record a cross-region edge `src → dst` at the storage site
    /// `hir_id`. Skips self-edges (src == dst).
    fn record_edge(&mut self, hir_id: HirId, src: Region, dst: Region) {
        if src != dst {
            self.cross_region_refs.push((hir_id, src, dst));
        }
    }

    /// Record the may-store edges for a declared-store native call at `site`: from
    /// each listed (stored) argument's regions to every OTHER heap argument's
    /// regions (the possible in-argument store targets), and mark the site HARD (the
    /// lowerer increfs a call-result source by value; docs/impl/region/effects.md
    /// "Hard edges"). Shared by the `Stores` and `Sends` effects: both perform this
    /// same edge/lifetime accounting (the message stays alive in the channel buffer
    /// for `Sends`). The fiber-frontier escape of a `Sends` message is escape's
    /// (`analyze_escape`'s fiber/send facet), not a fact this records.
    fn record_store_edges(&mut self, site: HirId, stored: &[usize], arg_regions: &[Vec<Region>]) {
        self.hard_edge_sites.insert(site);
        for &i in stored {
            let Some(src_rs) = arg_regions.get(i) else {
                continue;
            };
            let src_rs = src_rs.clone();
            for (j, dst_rs) in arg_regions.iter().enumerate() {
                if j == i {
                    continue;
                }
                let dst_rs = dst_rs.clone();
                for &src in &src_rs {
                    for &dst in &dst_rs {
                        self.record_edge(site, src, dst);
                    }
                }
            }
        }
    }
}

mod analyze;
mod build;
mod compensate;
mod escape;
mod format;
mod holders;
mod letrec;
mod merge;
mod ownership;
mod postdom;
mod tree;
mod walk;

pub use analyze::{analyze_regions, analyze_regions_with};
pub use format::format_regions;
// The escape→region projection (`regions::escape`), reused by the escape dump.
pub(crate) use escape::return_frontier_regions;

#[cfg(test)]
mod tests;
