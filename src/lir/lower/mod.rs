//! HIR to LIR lowering

mod access;
mod binding;
mod control;
mod emitops;
mod expr;
mod lambda;
mod pattern;
pub mod rcstats;

use std::sync::atomic::{AtomicU32, Ordering};

use super::intrinsics::IntrinsicOp;
use super::types::*;
use crate::hir::arena::BindingArena;
use crate::hir::region::{RegionInfo, StaticRegion};
use crate::hir::{analyze_escape, Binding, BlockId, EscapeInfo, Hir, HirId, HirKind, HirPattern};

/// Global region ID counter. IDs 0 (invalid) and 1 are reserved; minting starts at 2.
/// Used by the lowerer for solver-assigned regions and by the compilation
/// pipeline for transient compile-time regions.
static NEXT_STATIC_REGION: AtomicU32 = AtomicU32::new(2);

/// Short, stable name for an allocating `LirInstr` variant — used only
/// in the `--trace=rc:emit` lines to disambiguate which kind of alloc
/// was stamped on a phantom region's HirId.
fn instr_kind_name(instr: &LirInstr) -> &'static str {
    match instr {
        LirInstr::MakeClosure { .. } => "MakeClosure",
        LirInstr::MakeCaptureCell { .. } => "MakeCaptureCell",
        LirInstr::MakeArrayMut { .. } => "MakeArrayMut",
        LirInstr::List { .. } => "List",
        LirInstr::Call { .. } => "Call",
        LirInstr::SuspendingCall { .. } => "SuspendingCall",
        LirInstr::TailCall { .. } => "TailCall",
        LirInstr::CallArrayMut { .. } => "CallArrayMut",
        LirInstr::TailCallArrayMut { .. } => "TailCallArrayMut",
        LirInstr::Freeze { .. } => "Freeze",
        LirInstr::Thaw { .. } => "Thaw",
        _ => "other",
    }
}

/// Mint a fresh **static** region id — a compile-time, globally-unique slot
/// number baked into bytecode. A static id is a per-function slot, NOT a live
/// region: each activation remaps it to a freshly-minted `new_runtime_region`
/// via its `activation_region_map`. Never index a static id into the
/// `RegionStore` (see docs/impl/region-model.md § id-spaces).
pub fn new_static_region() -> StaticRegion {
    let id = NEXT_STATIC_REGION.fetch_add(1, Ordering::Relaxed);
    assert!(
        id >= 2,
        "static region id counter wrapped or hit reserved range"
    );
    StaticRegion::new(id).expect("static region id counter is >= 2, hence nonzero")
}
use crate::syntax::Span;
use crate::value::{Arity, SymbolId, Value};
use rustc_hash::FxHashMap;
use std::collections::HashMap;

/// Tracks an active Loop during lowering so `Recur` can find its
/// entry label and binding slots.
struct LoopLowerContext {
    loop_label: Label,
    binding_slots: Vec<u16>,
    /// Region slot whose `DecrefRegion` fires at the recur back-edge. None if not scoped.
    region_id: Option<StaticRegion>,
}

/// Tracks an active block during lowering so `break` can find its
/// result register and exit label.
struct BlockLowerContext {
    block_id: BlockId,
    #[allow(dead_code)]
    result_reg: Reg,
    result_slot: u16,
    exit_label: Label,
    /// The `region_depth` at the time this block was entered.
    /// `break` emits `(current_region_depth - region_depth_at_entry)`
    /// compensating `DecrefRegion` instructions before jumping to the exit.
    region_depth_at_entry: u32,
}

/// Lowers HIR to LIR
pub struct Lowerer<'a> {
    arena: &'a BindingArena,
    /// Current function being built
    current_func: LirFunction,
    /// Current block being built
    current_block: BasicBlock,
    /// Next register ID
    next_reg: u32,
    /// Next label ID
    next_label: u32,
    /// Mapping from Binding to local slot
    binding_to_slot: HashMap<Binding, u16>,
    /// Whether we're currently lowering a lambda (closure)
    in_lambda: bool,
    /// Number of captured variables (for lambda context)
    num_captures: u16,
    /// Number of parameters allocated as locals (non-LBox, non-captured params).
    /// Used by allocate_slot to compute capture_locals_mask offsets.
    num_local_params: u16,
    /// Set of bindings that are upvalues (captures/parameters in lambda)
    /// These use LoadCapture/StoreCapture, not LoadLocal/StoreLocal
    upvalue_bindings: std::collections::HashSet<Binding>,
    /// Current span for emitted instructions
    current_span: Span,
    /// Intrinsic operations for operator specialization.
    /// Maps global SymbolId to specialized LIR instruction.
    intrinsics: FxHashMap<SymbolId, IntrinsicOp>,
    /// Declared native `RegionEffect`s + intrinsic-op set, from the
    /// `PrimitiveClassification`. Read by `analyze_escape` (the store facet's
    /// native-call seeding); empty until `with_primitive_classification`.
    call_classification: crate::hir::CallClassification,
    /// Compile-time constant values for immutable bindings (for LoadConst optimization)
    immutable_values: HashMap<Binding, Value>,
    /// Stack of active loop contexts for `Recur` lowering
    loop_lower_contexts: Vec<LoopLowerContext>,
    /// Stack of active block contexts for `break` lowering
    block_lower_contexts: Vec<BlockLowerContext>,
    /// Current nesting depth of active allocation regions.
    /// Pending `DecrefRegion` region slots to emit before tail calls.
    pending_free_regions: Vec<StaticRegion>,
    /// Scratch slot for discarding unused intermediate values.
    /// Lazily allocated on first use. Reused across all discards
    /// within the same function, so only one extra local slot.
    discard_slot: Option<u16>,
    /// Symbol ID → name mapping for error messages.
    symbol_names: HashMap<u32, String>,
    /// Flat list of closure bodies. `MakeClosure` instructions reference
    /// closures by `ClosureId` (index into this list). Built depth-first
    /// during lowering.
    closures: Vec<LirFunction>,
    /// Binding of the current function being analyzed (for self-tail-call
    /// detection in escape analysis and drop insertion).
    current_function_binding: Option<Binding>,
    /// The self-recursive binding of the lambda body currently being lowered:
    /// the binding this lambda captures as `CaptureKind::Recursive` (a same-binding
    /// self-edge). A reference to it in **value** position resolves to the executing
    /// closure via `LoadSelf`; in **call** position it re-enters the current code+env
    /// (a self-call re-dispatch) — never a cell load, since the self-edge is cell-free.
    /// Read only from THIS lambda's own captures — a nested lambda that captures
    /// the same binding does so as a sibling `Capture`/`Local`, not `Recursive`,
    /// so it never sets this — and saved/restored across lambda boundaries like
    /// `current_function_binding` (`lower_lambda_body`). `None` outside a
    /// self-recursive lambda body.
    current_self_binding: Option<Binding>,
    /// Bindings whose initializer lambda references them across the lambda boundary
    /// (a `CaptureKind::Recursive` self-edge) — i.e. self-recursive local functions.
    /// A self-recursive closure is cell-free (the self-edge does not mark it captured),
    /// but it is still a per-call allocation: its region lives through the whole
    /// recursion (the self-reference borrows the executing closure), so its scope-end
    /// release is stranded as dead code past the frame-replacing recursive `TailCall`.
    /// It is consumed by `lower_letrec`/`lower_define` to derive
    /// `stranded_self_bindings`. Recorded when the closure is built
    /// (`lower_lambda_expr`).
    self_recursive_bindings: rustc_hash::FxHashSet<Binding>,
    /// The subset of `self_recursive_bindings` whose defining body is a **tail call**,
    /// so the closure's scope-end `DecrefRegion` is emitted as dead code past that
    /// frame-replacing `TailCall` and never runs — the per-call closure region would
    /// otherwise leak. `tail_callee_adopts` routes a tail call to such a binding
    /// through the runtime's `adopted_closures` release (`vm/execute.rs`), which frees
    /// the region exactly once at the recursion's normal completion. Gating on the
    /// tail-call body is what keeps the adopt from double-freeing a self-recursive
    /// closure whose `DecrefRegion` instead fires live (a non-tail body) — the
    /// use-after-free that gate prevents.
    stranded_self_bindings: rustc_hash::FxHashSet<Binding>,
    /// Letrec bindings of a closure-cycle merge whose MERGED arena the enclosing
    /// letrec's body TAIL-CALLS. The frame-replacing `TailCall` strands the
    /// arena's single binding-scope `DecrefRegion` as dead code, so a tail call
    /// to one of these adopts the merged region (`tail_callee_adopts` →
    /// `TailCallInfo::adopt_region`), released exactly once at the recursion's
    /// normal completion — the same channel `stranded_self_bindings` rides.
    /// Marked by `lower_letrec` from the letrec BODY's tail callees only (after
    /// the inits are lowered, so interior sibling rotations never adopt), and
    /// honoured only through a non-upvalue reference (a nested closure that
    /// captures the binding must not adopt the arena out from under a later use
    /// in the enclosing activation).
    stranded_cycle_bindings: rustc_hash::FxHashSet<Binding>,
    /// Closure regions of self-recursive **`def`** bindings whose scope-end
    /// `DecrefRegion` must be SUPPRESSED. A `letrec`-bound self-recursive closure has
    /// its release land at the letrec scope end — dead code past the body's
    /// frame-replacing `TailCall`, supplied once by the runtime adopt. A `def`-bound
    /// one instead has its closure region demise at the binding's last use — the
    /// func-load of the `(loop …)` recursive call — which the lowerer would emit as a
    /// LIVE `DecrefRegion` immediately BEFORE that call, freeing the closure out from
    /// under its own re-entry (the executing-closure re-dispatch then reads a recycled
    /// page). Suppressing that decref and adopting at the tail call instead (the
    /// binding is also `stranded_self_bindings`) reproduces the `letrec` path's runtime
    /// accounting: the region is freed exactly once, by the adopt at the recursion's
    /// normal completion.
    suppressed_self_regions: rustc_hash::FxHashSet<crate::hir::region::Region>,
    /// Parameter bindings of the current function (for per-parameter
    /// independence analysis in self-tail-calls).
    current_function_params: Option<Vec<Binding>>,
    /// Tofte-Talpin region inference results. Scope decisions use region
    /// assignments instead of syntactic escape analysis.
    region_info: RegionInfo,
    /// Authoritative escape facts (`src/hir/escape.rs`), computed once at the
    /// top of `lower`. Read by `control/call.rs::tail_callee_adopts` for the
    /// escape half of the adopt decision (region-locality stays a region fact).
    /// NOT read by `tail_arg_is_borrowed` (a structural ownership-location test —
    /// escape over-marks owned tail-args and double-frees across fiber resume; see
    /// `control.rs`) nor by the `lower_return` mint (unconditional since the move
    /// convention was removed).
    escape_info: EscapeInfo,
    /// `cross_region_refs` indexed by their site HirId — the `(source, target)`
    /// region pairs to `IncrefRegion` at that node. Built once in
    /// `with_region_info` so `emit_increfs_for` is an O(1) lookup rather than a
    /// linear scan of every cross-region ref per HIR node (an O(n²) over stdlib).
    /// The `target` rides alongside the `source` (the lone region the incref
    /// actually names) so `emit_increfs_for` can classify a post-merge
    /// intra-region self-edge — `is_merge_self_edge(source, target)` — and DROP it
    /// (transform 2: a merged `source→target` store edge is intra-region, so its
    /// incref is unbalanced by the self-skipping cascade; region-rules.md
    /// § "Self-edge elimination").
    increfs_by_site: HashMap<HirId, Vec<(crate::hir::region::Region, crate::hir::region::Region)>>,
    /// `region_data` indexed by `decref_point` HirId — regions whose demise
    /// lands at that node. Built once in `with_region_info` so
    /// `emit_decrefs_for` is an O(1) lookup (then a small per-call
    /// tail-region filter) rather than scanning all regions per node.
    decrefs_by_decref_point: HashMap<HirId, Vec<crate::hir::region::Region>>,
    /// Current HIR node being lowered. Set at the top of `lower_expr`.
    /// Used by `alloc_region_id()` to look up the region for allocations.
    current_hir_id: Option<HirId>,
    /// Maps Region(u32) from region inference to u16 index in the
    /// function's region_table. Lazily populated by `alloc_region_id()`.
    region_to_table: HashMap<crate::hir::region::Region, StaticRegion>,
    /// Stack of active region slots for `DecrefRegion` emission on break.
    /// Pushed when a scope enters, popped at scope exit.
    active_region_ids: Vec<StaticRegion>,
    /// For each allocating HIR node's region, the slot of the
    /// binding that names its result. Populated by `lower_let`,
    /// `lower_letrec`, `lower_define`, and other binding sites by
    /// reading `region_info.alloc_region.get(&init.id)` after the
    /// slot is allocated. Saved/restored across lambda boundaries
    /// (see `lower_lambda_body`).
    ///
    /// `emit_decrefs_for` consults this map for `call_result_regions`:
    /// it emits `LoadLocal slot` + `DecrefValueRegion` so the
    /// release uses the *runtime* region of the actual returned value,
    /// not the compile-time placeholder. The expected region id gates
    /// the decref so passthrough calls (whose result lives in a
    /// different region) skip.
    ///
    /// After the ANF lift (`src/hir/anf.rs`) every allocating
    /// expression in a consumer position is bound to a synthetic
    /// `Let`, so the binding-slot path covers the Call result directly —
    /// no separate stash-and-reload slot at the Call site.
    region_to_slot: HashMap<crate::hir::region::Region, u16>,
    /// Static region slots the lowerer has stamped onto at least one
    /// instruction via `emit_in_region` (i.e., regions the runtime
    /// will actually have a slot for after `alloc_in_region`).
    /// Used by `emit_decref_region` to suppress phantom DecrefRegion
    /// emissions — the analysis may yield a `decref_point` for a region
    /// whose alloc never landed in the bytecode (legitimately, for
    /// `call_result_regions` going through `DecrefValueRegion`
    /// instead; less legitimately, when the regions walk assigned a
    /// region to a node the lowerer is transparent for). Emitting
    /// `DecrefRegion(r)` for an unstamped r would decrement an RC
    /// the runtime never raised.
    emitted_alloc_regions: rustc_hash::FxHashSet<StaticRegion>,
    /// HirIds whose trailing `emit_decrefs_for` (in `lower_expr`) is
    /// suppressed because the caller will emit it itself at a better
    /// point. `lower_let` uses this for a binding's init: the init's
    /// region `decref_point` is the init's own HirId when the binding is
    /// unused, but `lower_expr`'s automatic decref fires *before*
    /// `lower_let` stores the init value into the slot it reloads — so the
    /// decref would hit the slot's stamped `nil` and the real value would
    /// leak. `lower_let` defers it, stores, then emits the decref against
    /// the now-populated slot.
    deferred_decref_points: rustc_hash::FxHashSet<HirId>,
}

mod regiondecref;
mod regionemit;

impl<'a> Lowerer<'a> {
    pub fn new(arena: &'a BindingArena) -> Self {
        Lowerer {
            arena,
            current_func: LirFunction::new(Arity::Exact(0)),
            current_block: BasicBlock::new(Label(0)),
            next_reg: 0,
            next_label: 1, // 0 is entry
            binding_to_slot: HashMap::new(),
            in_lambda: false,
            num_captures: 0,
            num_local_params: 0,
            upvalue_bindings: std::collections::HashSet::new(),
            current_span: Span::synthetic(),
            intrinsics: FxHashMap::default(),
            call_classification: crate::hir::CallClassification::default(),
            immutable_values: HashMap::new(),
            loop_lower_contexts: Vec::new(),
            block_lower_contexts: Vec::new(),
            pending_free_regions: Vec::new(),
            discard_slot: None,
            symbol_names: HashMap::new(),
            closures: Vec::new(),
            current_function_binding: None,
            current_self_binding: None,
            current_function_params: None,
            self_recursive_bindings: rustc_hash::FxHashSet::default(),
            stranded_self_bindings: rustc_hash::FxHashSet::default(),
            stranded_cycle_bindings: rustc_hash::FxHashSet::default(),
            suppressed_self_regions: rustc_hash::FxHashSet::default(),
            region_info: RegionInfo::empty(),
            escape_info: EscapeInfo::empty(),
            increfs_by_site: HashMap::new(),
            decrefs_by_decref_point: HashMap::new(),
            current_hir_id: None,
            region_to_table: HashMap::new(),
            active_region_ids: Vec::new(),

            region_to_slot: HashMap::new(),
            emitted_alloc_regions: rustc_hash::FxHashSet::default(),
            deferred_decref_points: rustc_hash::FxHashSet::default(),
        }
    }

    /// Set all primitive property sets from a PrimitiveClassification.
    pub fn with_primitive_classification(
        mut self,
        pc: crate::lir::intrinsics::PrimitiveClassification,
    ) -> Self {
        self.intrinsics = pc.intrinsics;
        self.call_classification = pc.call_classification;
        self
    }

    /// Set symbol names for error messages.
    pub fn with_symbol_names(mut self, names: HashMap<u32, String>) -> Self {
        self.symbol_names = names;
        self
    }

    /// Seed `immutable_values` with primitive binding→value pairs.
    ///
    /// Primitive bindings are `BindingScope::Local` with `mark_immutable()`.
    /// The lowerer never allocates slots for them — instead, `lower_var`
    /// checks `immutable_values` first and emits `LoadConst` for any
    /// binding with a known constant value.
    pub fn with_primitive_values(mut self, values: HashMap<Binding, Value>) -> Self {
        self.immutable_values.extend(values);
        self
    }

    /// Set Tofte-Talpin region inference results.
    pub fn with_region_info(mut self, info: RegionInfo) -> Self {
        // Pre-index the two collections that `emit_increfs_for` /
        // `emit_decrefs_for` consult per HIR node, so each lookup is O(1)
        // instead of a linear scan (which made lowering O(n²) over a
        // large compilation unit like the stdlib).
        let mut increfs_by_site: HashMap<
            HirId,
            Vec<(crate::hir::region::Region, crate::hir::region::Region)>,
        > = HashMap::new();
        for &(site, src, dst) in &info.cross_region_refs {
            increfs_by_site.entry(site).or_default().push((src, dst));
        }
        let mut decrefs_by_decref_point: HashMap<HirId, Vec<crate::hir::region::Region>> =
            HashMap::new();
        for (&r, d) in &info.region_data {
            decrefs_by_decref_point
                .entry(d.decref_point)
                .or_default()
                .push(r);
        }
        // A store-adopted member (`owned_adopt_edges`: each store site maps to
        // `(member, owner)` interior edges) keeps its OWN decref, which is a structural
        // no-op only while the member is still `Owned` — i.e. only before its owner's
        // subtree drop. So at a shared decref_point it must sort BEFORE every release
        // that can free the member's owner there (the owner's holder-binding release AND
        // the discarded pass-through result of a `%array-push`/`%put`, which returns its
        // container — both value-gated below). Ordering an `Owned` no-op ahead of the
        // page-reading releases is safe: it reads and frees nothing; it then no-ops, and
        // the owner's later release subtree-drops the member exactly once. Without this
        // the readers-before-freers order below sorted the member's plain `DecrefRegion`
        // LAST, so the owner's release reclaimed the member before its own decref, which
        // then faulted on the freed region (the emit-order half of the lifetime
        // obligation — docs/impl/region-model.md § "The lifetime obligation the root
        // carries"; region-array-push-pair-loop-uaf.lisp).
        let store_adopted_members: rustc_hash::FxHashSet<crate::hir::region::Region> = info
            .owned_adopt_edges
            .values()
            .flatten()
            .map(|&(member, _owner)| member)
            .collect();
        // Release order at a shared decref_point is deterministic and
        // dependency-safe (docs/impl/region-rules.md Rule 4): a store-adopted member's
        // `Owned` no-op sorts FIRST (class 0), then releases that READ pages come before
        // releases that FREE them. Class 1 — `DecrefValueRegion` (loads a slot and
        // derefs the value, unwrapping a capture cell); class 2 — `DecrefCellRegion`
        // (reads the cell's page header via `region_of`); class 3 — plain `DecrefRegion`
        // (frees, reads nothing). Region id breaks ties so the order never depends on
        // hash iteration (`region_data` is a HashMap; its iteration order is random per
        // instance — the unsorted order was the flaky capture-cell UAF,
        // region-capture-cell-noreassign-uaf.lisp).
        for regions in decrefs_by_decref_point.values_mut() {
            regions.sort_by_key(|r| {
                let class = if store_adopted_members.contains(r) {
                    0
                } else if info.cell_release_regions.contains(r) {
                    2
                } else if info.call_result_regions.contains(r) {
                    1
                } else {
                    3
                };
                (class, r.0)
            });
        }
        self.increfs_by_site = increfs_by_site;
        self.decrefs_by_decref_point = decrefs_by_decref_point;
        self.region_info = info;
        self
    }

    /// Check if a scope has local allocations (reclaimable).
    fn region_scope_check(&self, hir_id: HirId) -> bool {
        self.region_info.scope_has_local_allocs(hir_id)
    }

    /// Check if a loop has local allocations (rotation-eligible).
    fn region_loop_check(&self, hir_id: HirId) -> bool {
        self.region_info.scope_has_local_allocs(hir_id)
    }

    /// Lower a HIR expression to an LIR module.
    ///
    /// Returns an `LirModule` with the entry function and a flat list of
    /// closure bodies. Each closure is an independent compilation unit
    /// referenced by `ClosureId`.
    pub fn lower(&mut self, hir: &Hir) -> Result<LirModule, String> {
        // Escape analysis is whole-module, like region inference — compute it
        // once over the full canonical HIR before lowering recurses into
        // closures. Computing it here keeps the pass on every real lowering path
        // through a single chokepoint; `tail_callee_adopts` reads it.
        self.escape_info = analyze_escape(hir, self.arena, &self.call_classification);

        self.current_func = LirFunction::new(Arity::Exact(0));
        self.current_block = BasicBlock::new(Label(0));
        self.next_reg = 0;
        self.next_label = 1;
        self.binding_to_slot.clear();
        self.discard_slot = None;
        self.closures.clear();

        let result_reg = self.lower_expr(hir)?;
        self.terminate(Terminator::Return(result_reg));
        self.finish_block();

        self.current_func.entry = Label(0);
        self.current_func.num_regs = self.next_reg;
        // Propagate signal from HIR to top-level LIR function
        self.current_func.signal = hir.signal;

        // Record the entry function's merged slots — the root slots a builder-idiom
        // merge shares, for runtime mint-or-reuse (see `record_merged_slots`). Empty
        // unless a merge fired.
        self.record_merged_slots();

        let entry = std::mem::replace(&mut self.current_func, LirFunction::new(Arity::Exact(0)));
        let closures = std::mem::take(&mut self.closures);

        Ok(LirModule { entry, closures })
    }

    /// Check if a HIR body is a tail call (or control flow where all result
    /// positions are tail calls). Used to relax the suspension check: a
    /// tail call replaces the frame, so its signal doesn't affect the
    /// enclosing scope's lifetime.
    ///
    /// After the ANF lift, a tail call previously of the form `(f x)`
    /// becomes `(let [t (f x)] t)`. Recognise this single-binding shape
    /// where the body is `Var(b)` and check the init for tail-callness
    /// — `mark_tail_calls` runs before ANF, so `is_tail` is preserved
    /// on the wrapped Call.
    fn body_is_tail_call(hir: &Hir) -> bool {
        match &hir.kind {
            HirKind::Call { is_tail: true, .. } => true,
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => Self::body_is_tail_call(then_branch) && Self::body_is_tail_call(else_branch),
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses
                    .iter()
                    .all(|(_, body)| Self::body_is_tail_call(body))
                    && else_branch
                        .as_ref()
                        .is_some_and(|b| Self::body_is_tail_call(b))
            }
            HirKind::Begin(exprs) => exprs.last().is_some_and(Self::body_is_tail_call),
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                // ANF wrap shape: `(let [b e] (var b))` is tail-equivalent
                // to `e`.
                if bindings.len() == 1 {
                    let (b, init) = (&bindings[0].0, &bindings[0].1);
                    if matches!(&body.kind, HirKind::Var(v) if v == b)
                        && Self::body_is_tail_call(init)
                    {
                        return true;
                    }
                }
                Self::body_is_tail_call(body)
            }
            HirKind::Match { arms, .. } => arms
                .iter()
                .all(|(_, _, body)| Self::body_is_tail_call(body)),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests;
