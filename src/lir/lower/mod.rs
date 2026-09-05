// audited: 2026-09-05
//! HIR to LIR lowering: the `Lowerer` and the state one function's lowering
//! carries. The passes themselves live in the sibling modules named below.
//!
//! docs/impl/lir.md

mod access;
mod aliases;
mod binding;
mod control;
mod emitops;
mod expr;
mod lambda;
mod naming;
mod order;
mod pattern;
pub mod rcstats;
mod regiondecref;
mod regionemit;
mod relocate;
mod splice;
mod tailcall;

use super::intrinsics::IntrinsicOp;
use super::types::*;
use crate::hir::arena::BindingArena;
use crate::hir::region::{RegionInfo, StaticRegion};
use crate::hir::{analyze_escape, Binding, BlockId, EscapeInfo, Hir, HirId, HirKind, HirPattern};
use crate::syntax::Span;
use crate::value::{Arity, SymbolId, Value};
use rustc_hash::FxHashMap;
use std::collections::HashMap;

pub use naming::new_static_region;
pub(crate) use naming::ValueSlot;
// The walk it names is a debug-only net, so the import is gated with it — an
// unconditional `use` of a `cfg`-gated item fails the release build alone.
#[cfg(debug_assertions)]
use order::assert_cells_outlive_their_readers;
pub(crate) use relocate::{BranchHoists, HoistBlock, TailExitHoist};

/// Tracks an active Loop during lowering so `Recur` can find its
/// entry label and binding slots.
struct LoopLowerContext {
    loop_label: Label,
    binding_slots: Vec<u16>,
    /// Region slot whose `DecrefRegion` fires at the recur back-edge. None if not scoped.
    region_id: Option<StaticRegion>,
}

/// Tracks an active block during lowering so `break` can find its
/// result register and exit label. A `break` emits no region instruction of its
/// own: both the value it carries and every release its jump passes over are
/// anchored on the BLOCK by the solver, which the lowerer emits after the exit
/// label (docs/impl/region/anchors.md).
struct BlockLowerContext {
    block_id: BlockId,
    #[allow(dead_code)]
    result_reg: Reg,
    result_slot: u16,
    exit_label: Label,
}

/// Lowers HIR to LIR
pub struct Lowerer<'a> {
    arena: &'a BindingArena,
    /// The owning instance's display memo, for naming a binding in an error.
    /// Lowering never resolves a name to decide anything — the only reader is
    /// the `undefined variable` message, which is a user's own spelling and so
    /// is not in the static vocabulary (docs/impl/symbol.md). `None` degrades
    /// that one message to the hash.
    symbols: Option<&'a crate::symbol::SymbolTable>,
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
    /// Bindings bound to a BORROWED SUBVIEW of a scrutinee by destructuring:
    /// a `(a & rest)` list pattern, a `(entry & rest)` head, an array element,
    /// a struct value — every binding a structural ELEMENT load (`First`/`Rest`/
    /// `Index`/`Key`) reaches reads a pointer aliased into the scrutinee's
    /// region pages with NO owning reference (the region solver only registers
    /// counted container reads for *call-site* `rest()`/`first()`/`get()`, never
    /// for pattern loads). Marking them borrowed makes
    /// `arg_leaf_is_borrowed`/`tail_arg_is_borrowed` treat a call arg naming one
    /// like any other borrowed arg: the caller mints a fresh owning reference at
    /// the call that the callee's release balances, leaving the scrutinee's own
    /// reference untouched. `Slice`/`StructRest` (array/struct `& rest`) are
    /// excluded: they mint fresh owned containers.
    ///
    /// Computed ONCE over the whole HIR at `lower()` entry by
    /// [`Self::precompute_destructure_aliases`] — before any body lowering — so
    /// a `match` compound used as a tail argument has its aliases registered
    /// before `tail_arg_is_borrowed` consults them (a call's arguments are
    /// classified before they are lowered). The decision-tree / seq / destructure
    /// walks also insert as they go; both routes fill the same set, and an
    /// insert is idempotent.
    destructure_alias_bindings: std::collections::HashSet<Binding>,
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
    /// otherwise leak. `tail_callee_defers_release` routes a tail call to such a binding
    /// through the activation's own deferred set (`ActivationDues`), which frees
    /// the region exactly once at whichever end that activation reaches
    /// (docs/impl/region/owner.md § "A deferred tail-call release has the node's
    /// life"). Gating on the
    /// tail-call body is what keeps the deferral from double-freeing a self-recursive
    /// closure whose `DecrefRegion` instead fires live (a non-tail body) — the
    /// use-after-free that gate prevents.
    stranded_self_bindings: rustc_hash::FxHashSet<Binding>,
    /// Letrec bindings of a closure-cycle merge whose MERGED arena the enclosing
    /// letrec's body TAIL-CALLS. The frame-replacing `TailCall` strands the
    /// arena's single binding-scope `DecrefRegion` as dead code, so a tail call
    /// to one of these defers the merged region's release (`tail_callee_defers_release`
    /// → `TailCallInfo::deferred`), run exactly once at the recursion's
    /// normal completion — the same channel `stranded_self_bindings` rides.
    /// Marked by `lower_letrec` from the letrec BODY's tail callees only (after
    /// the inits are lowered, so interior sibling rotations never defer), and
    /// honoured only through a non-upvalue reference (a nested closure that
    /// captures the binding must not free the arena out from under a later use
    /// in the enclosing activation).
    stranded_cycle_bindings: rustc_hash::FxHashSet<Binding>,
    /// Letrec bindings the enclosing letrec's BODY tail-calls whose OWN closure
    /// region the solver releases at that letrec's scope end — the release the
    /// frame-replacing `TailCall` strands, and the one the relocation must leave
    /// where it is because the callee is about to enter that closure
    /// (docs/impl/region/relocate.md). A member captured by a sibling has uses spanning the whole
    /// letrec, so its demise lands at the scope end rather than at the call node
    /// and `tail_callee_defers_release`'s dies-here reading never sees it. Marked
    /// by `lower_letrec` after the inits are lowered, and honoured only through a
    /// non-upvalue reference, for the reasons `stranded_cycle_bindings` is; a
    /// suppressed release (owned by the store or capture-adopt path) and a
    /// closure-cycle member (released by the merge's own channel) are excluded at
    /// the marking, so the three channels never name one region twice.
    stranded_member_bindings: rustc_hash::FxHashSet<Binding>,
    /// Parameter bindings of the current function (for per-parameter
    /// independence analysis in self-tail-calls).
    current_function_params: Option<Vec<Binding>>,
    /// Tofte-Talpin region inference results. Scope decisions use region
    /// assignments instead of syntactic escape analysis.
    region_info: RegionInfo,
    /// Authoritative escape facts (`src/hir/escape.rs`), computed once at the
    /// top of `lower`. Read by `control/call.rs::tail_callee_defers_release` for the
    /// escape half of the deferral decision (region-locality stays a region fact).
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
    /// incref is unbalanced by the self-skipping cascade; region/mechanism.md
    /// § "Self-edge elimination").
    increfs_by_site: HashMap<HirId, Vec<(crate::hir::region::Region, crate::hir::region::Region)>>,
    /// `region_data` indexed by `decref_point` HirId — regions whose demise
    /// lands at that node. Built once in `with_region_info` so
    /// `emit_decrefs_for` is an O(1) lookup (then a small per-call
    /// tail-region filter) rather than scanning all regions per node.
    decrefs_by_decref_point: HashMap<HirId, Vec<crate::hir::region::Region>>,
    /// `RegionInfo::cell_containers` indexed by demise node — the fn-local
    /// 1-slot containers whose current content is dropped when that scope
    /// exits (docs/impl/region/bindings.md § "Reassigned mutable bindings are
    /// 1-slot containers"). Empty when the unit has no such cell.
    cell_drops_by_demise: HashMap<HirId, Vec<Binding>>,
    /// Current HIR node being lowered. Set at the top of `lower_expr`.
    /// Used by `alloc_region_id()` to look up the region for allocations.
    current_hir_id: Option<HirId>,
    /// Maps Region(u32) from region inference to u16 index in the
    /// function's region_table. Lazily populated by `alloc_region_id()`.
    region_to_table: HashMap<crate::hir::region::Region, StaticRegion>,
    /// For each allocating HIR node's region, the slot of the
    /// binding that names its result. See [`ValueSlot`] for why the
    /// address space travels with the index. Populated by `lower_let`,
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
    region_to_slot: HashMap<crate::hir::region::Region, ValueSlot>,
    /// Stack slots (this function's local index space) owned by a fn-local
    /// reassigned mutable binding (`RegionInfo::reassigned_local_bindings`).
    /// Populated in `allocate_slot_routed`, reset per function like
    /// `region_to_slot`. `emit_decrefs_for` refuses a value-route decref +
    /// nil-stamp whose slot is in this set: `allocate_slot` never reuses a slot,
    /// so such a slot holds the reassigned binding's own live value for the
    /// binding's whole scope — nil-stamping it mid-scope would zero a live value
    /// (the reassigned-loop-counter clobber; region-capture-cell-loop-uaf.lisp).
    reassigned_local_slots: rustc_hash::FxHashSet<u16>,
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
    /// Tail-call HirIds whose result a `Return` mint already covers, so
    /// `lower_call`'s post-`TailCall` fall-through retain must stand down: the
    /// return mint is emitted exactly once per returned value
    /// (docs/impl/region/mechanism.md � "The return mint is emitted exactly once").
    ///
    /// The shape is ANF's canonical wrap of a tail call in a non-propagating tail
    /// position — `(let [t (f …)] (return t))`, built for a tail call nested in a
    /// `begin`/`if`/`cond`/`match` arm. There the frame HOLDS the result (the
    /// synthetic binding) and its `decref_point` balances `lower_return`'s mint,
    /// so the fall-through retain would be a second, unbalanced reference. A tail
    /// call ANF leaves unnamed (a `let`/lambda body) has no binding and no
    /// `Return`, so its fall-through retain IS the mint and is absent here.
    /// Recorded by `lower_let`, the only lowering site that sees the wrap.
    return_minted_calls: rustc_hash::FxHashSet<HirId>,
    /// The relocation points covering every path that reaches the current
    /// emission position (see [`TailExitHoist`]). Either a single
    /// [`HoistBlock::Current`] entry — a frame-replacing tail call emitted into
    /// this very block, which dominates everything after it — or the
    /// [`HoistBlock::Finished`] points a branch merge inherited, from its arms
    /// and from the position the branch was entered at. Set by `lower_call`'s
    /// tail arm and by the branch lowerings, cleared at every other block
    /// boundary, and saved/restored across lambda boundaries like every other
    /// per-function slot map.
    tail_exit_hoist: Vec<TailExitHoist>,
    /// Points sealed from the arms of the branch currently being lowered, which
    /// `open_branch_merge` hands to its merge block. Saved and restored around
    /// each branch (and each lambda), so a nested branch's arms never leak into
    /// the enclosing one's collection.
    arm_exit_hoists: Vec<TailExitHoist>,
    /// Set while `with_tail_exit_hoist` emits a release it is about to REPLICATE
    /// ahead of a branch arm's `TailCall`. Such a release must name a VALUE, so
    /// that the copy a path reaches second loads the `nil` the first stamped and
    /// no-ops; `emit_decref_for_region` reads this to take the value route for a
    /// region it would otherwise release by id (docs/impl/region/mechanism.md §
    /// "Self-cancelling is a property of the ROUTE, not of the region's class").
    /// False everywhere else, where one point covers every path and one
    /// instruction does.
    replicating_release: bool,
}

impl<'a> Lowerer<'a> {
    pub fn new(arena: &'a BindingArena) -> Self {
        Lowerer {
            arena,
            symbols: None,
            current_func: LirFunction::new(Arity::Exact(0)),
            current_block: BasicBlock::new(Label(0)),
            next_reg: 0,
            next_label: 1, // 0 is entry
            binding_to_slot: HashMap::new(),
            in_lambda: false,
            num_captures: 0,
            num_local_params: 0,
            upvalue_bindings: std::collections::HashSet::new(),
            destructure_alias_bindings: std::collections::HashSet::new(),
            current_span: Span::synthetic(),
            intrinsics: FxHashMap::default(),
            call_classification: crate::hir::CallClassification::default(),
            immutable_values: HashMap::new(),
            loop_lower_contexts: Vec::new(),
            block_lower_contexts: Vec::new(),
            pending_free_regions: Vec::new(),
            discard_slot: None,
            closures: Vec::new(),
            current_function_binding: None,
            current_self_binding: None,
            current_function_params: None,
            self_recursive_bindings: rustc_hash::FxHashSet::default(),
            stranded_self_bindings: rustc_hash::FxHashSet::default(),
            stranded_cycle_bindings: rustc_hash::FxHashSet::default(),
            stranded_member_bindings: rustc_hash::FxHashSet::default(),
            region_info: RegionInfo::empty(),
            escape_info: EscapeInfo::empty(),
            increfs_by_site: HashMap::new(),
            decrefs_by_decref_point: HashMap::new(),
            cell_drops_by_demise: HashMap::new(),
            current_hir_id: None,
            region_to_table: HashMap::new(),

            region_to_slot: HashMap::new(),
            reassigned_local_slots: rustc_hash::FxHashSet::default(),
            emitted_alloc_regions: rustc_hash::FxHashSet::default(),
            deferred_decref_points: rustc_hash::FxHashSet::default(),
            return_minted_calls: rustc_hash::FxHashSet::default(),
            tail_exit_hoist: Vec::new(),
            arm_exit_hoists: Vec::new(),
            replicating_release: false,
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

    /// Give lowering the instance's display memo, so an `undefined variable`
    /// error names the variable the user wrote.
    pub fn with_symbols(mut self, symbols: &'a crate::symbol::SymbolTable) -> Self {
        self.symbols = Some(symbols);
        self
    }

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
        // through a single chokepoint; `tail_callee_defers_release` reads it.
        self.escape_info = analyze_escape(hir, self.arena, &self.call_classification);

        // Same whole-module contract: compute the destructure-alias set once
        // over the full HIR before body lowering begins, so a `match` compound
        // used as a tail argument has its aliases registered before the call
        // site classifies that argument (arguments are classified, then
        // lowered). The decision-tree/seq/destructure walks re-insert as they
        // go; that stays idempotent and covers any pattern the walk reaches
        // that the precompute pass did not enumerate.
        self.destructure_alias_bindings.clear();
        self.precompute_destructure_aliases(hir);

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

        let module = LirModule { entry, closures };
        #[cfg(debug_assertions)]
        assert_cells_outlive_their_readers(&module);
        Ok(module)
    }
}

#[cfg(test)]
mod tests;
