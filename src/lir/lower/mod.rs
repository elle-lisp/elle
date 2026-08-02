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
/// `RegionStore` (see docs/impl/region/model.md § id-spaces).
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
/// result register and exit label. A `break` emits no region instruction of its
/// own: both the value it carries and every release its jump passes over are
/// anchored on the BLOCK by the solver, which the lowerer emits after the exit
/// label (docs/impl/region/mechanism.md § "`break` transfers its value" and
/// § "A release the break jumps over is not a release").
struct BlockLowerContext {
    block_id: BlockId,
    #[allow(dead_code)]
    result_reg: Reg,
    result_slot: u16,
    exit_label: Label,
}

/// The relocation point a frame-replacing tail call opens in its own block
/// (docs/impl/region/mechanism.md § "A release past a frame-replacing tail call
/// is not a release").
///
/// Everything the lowerer emits after a `TailCall` runs only on the NATIVE
/// fall-through — a native pushes no bytecode frame, so the dispatch loop
/// continues into that block, while a closure callee replaces the frame and
/// never arrives. A release landing there is therefore emitted where control may
/// never reach, and the frame's own reference is stranded once per call. Moving
/// that one release to just before the `TailCall` costs no count argument (it is
/// the same single release, relocated), and is legal for every region the call
/// itself cannot reach.
///
/// A branch merge inherits the points of the arms that reach it, so a point also
/// outlives its own block (§ "The relocation point outlives the block"). There
/// the release is emitted at the merge *and* replicated at each point, which is
/// sound only for a self-cancelling run — one that nil-stamps the slot it read,
/// so the copy a path reaches second no-ops.
struct TailExitHoist {
    /// Index of the `TailCall` in its block's instruction list. A hoisted
    /// release is spliced in here, ahead of the frame replacement, and the index
    /// advances so successive hoists keep their emission order.
    at: usize,
    /// The block the `TailCall` sits in.
    block: HoistBlock,
    /// The local slots and capture indices the call's operands were loaded from.
    /// A release that reloads one of these reloads the very value now sitting on
    /// the operand stack, so it IS the ownership move however ANF spelled the
    /// argument — the reading `exempt` cannot give, since ANF is free to rewrite
    /// an operand into a synthetic binding whose region the syntax walk does not
    /// connect back to the call.
    operand_locals: rustc_hash::FxHashSet<u16>,
    operand_captures: rustc_hash::FxHashSet<u16>,
    /// Regions the callee or an argument subtree names, canonicalized through
    /// the merge forest. These releases must STAY in the dead block: an
    /// argument's is the ownership move the calling convention rests on, and the
    /// callee's belongs to the activation that takes it over.
    exempt: rustc_hash::FxHashSet<crate::hir::region::Region>,
    /// Regions this call's callee closure holds through its captured environment,
    /// canonicalized through the merge forest
    /// (`TailCalleeFacts::capture_funded`). That counted edge spans the gap
    /// between a release relocated ahead of the `TailCall` and the callee's own
    /// return mint, so it is what admits a region whose only escape facet is the
    /// return one (docs/impl/region/mechanism.md § "The callee's return mint, and
    /// the edge that funds the gap"). Empty for a callee whose captures this
    /// compilation cannot see, which then funds nothing.
    capture_funded: rustc_hash::FxHashSet<crate::hir::region::Region>,
}

/// Where a relocation point lives, and with it which of the two placements
/// applies: a MOVE within the block still being filled, or a REPLICA spliced
/// into an arm that has already closed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HoistBlock {
    /// The block the lowerer is filling. Its label validates the point — a
    /// stale one names an instruction list this block no longer is.
    Current(Label),
    /// A branch arm already pushed onto `LirFunction::blocks`, by index. Blocks
    /// are only ever appended, so the index stays valid for the function's life.
    Finished(usize),
}

/// Where a value-route release reads the value whose region it means.
///
/// `allocate_slot_routed` mints binding slots from two disjoint address spaces,
/// both indexed by `u16`: an in-lambda captured binding gets an ENV index (the
/// index `LoadCapture`/`StoreCapture` address, backed by the `populate_env`
/// cell), and every other binding gets a STACK index (`LoadLocal`/`StoreLocal`).
/// Nothing about the number says which, so a bare `u16` in `region_to_slot` lets
/// an env index be read back as a stack slot — naming whichever local happens to
/// sit at that index and releasing it under its holder
/// (`tests/elle/region-def-in-lambda-capture.lisp`). Carrying the space with the
/// index makes that unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum ValueSlot {
    /// A stack-frame local. `LoadLocal { slot }` yields the value itself.
    Local(u16),
    /// An env-cell index. `LoadCapture { index }` UNWRAPS the cell and yields
    /// its content — the value whose region a release means. (The cell's own
    /// region is a separate concern, released through `LoadCaptureRaw` +
    /// `DecrefCellRegion` for a `cell_release_regions` member.)
    Env(u16),
}

impl ValueSlot {
    /// The raw index, for the sites that only need to dedupe or report it.
    pub(super) fn index(self) -> u16 {
        match self {
            ValueSlot::Local(i) | ValueSlot::Env(i) => i,
        }
    }

    /// The stack slot, or `None` for an env index. Use at sites whose emission
    /// is stack-only (`AdoptRegion`, `FreeRegionGroup`, the branch-arm
    /// compensations): skipping an env-celled region there leaves it
    /// independently reference-counted, which is each of those cuts' documented
    /// always-legal fallback.
    pub(super) fn local(self) -> Option<u16> {
        match self {
            ValueSlot::Local(i) => Some(i),
            ValueSlot::Env(_) => None,
        }
    }
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
    /// otherwise leak. `tail_callee_defers_release` routes a tail call to such a binding
    /// through the runtime's `deferred_releases` release (`vm/execute.rs`), which frees
    /// the region exactly once at the recursion's normal completion. Gating on the
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
    /// (docs/impl/region/mechanism.md § "What the exemption keeps, a channel must
    /// still run"). A member captured by a sibling has uses spanning the whole
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
    /// (docs/impl/region/mechanism.md § "The return mint is emitted exactly once").
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
    /// [`HoistBlock::Finished`] points a branch merge inherited from its arms.
    /// Set by `lower_call`'s tail arm and by the branch lowerings, cleared at
    /// every other block boundary, and saved/restored across lambda boundaries
    /// like every other per-function slot map.
    tail_exit_hoist: Vec<TailExitHoist>,
    /// Points sealed from the arms of the branch currently being lowered, which
    /// `open_branch_merge` hands to its merge block. Saved and restored around
    /// each branch (and each lambda), so a nested branch's arms never leak into
    /// the enclosing one's collection.
    arm_exit_hoists: Vec<TailExitHoist>,
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
        // The **release order** at each shared `decref_point` (docs/impl/region/rules.md
        // Rule 4): the `(earlier, later)` edges of every region that HOLDS another's pages
        // live, so the holder's release is emitted first. Two sources:
        //
        // - **Adoption.** An adopted member keeps its OWN `DecrefRegion`, a structural
        //   no-op only while the member is still `Owned` — once its owner's subtree drop
        //   reclaims it, that decref faults. So a member releases before its owner. The
        //   adopt maps hold exactly those edges: `owned_adopt_edges` (store-adopted, each
        //   store site → `(member, owner)`) and `capture_adopt_edges` (capture-adopted,
        //   each closure site → `(captured, closure)`), disjoint per member — a member is
        //   adopted by its single owner through exactly one map (region/info.rs).
        // - **Value aliasing.** A region that may be — or live inside — another is
        //   released `DecrefValueRegion`-style, resolving its runtime region by READING
        //   the value's own page, which the other's release can tear. Where the two land
        //   on one point (a discarded read, whose alias dies exactly where its container
        //   does), the alias must be ordered ahead. The same three relations the ownership
        //   cut's alias obligation closes over supply these edges, oriented
        //   `alias → source` throughout (region/adopt.md § "The lifetime obligation the
        //   root carries"): a native read's result and the container it read from
        //   (`counted_read_aliases`), an opaque call's result and its arguments
        //   (`opaque_result_aliases`), and a `Funnel`'s result and its container
        //   (`funnel_result_containers`). They compose transitively through the sort, so a
        //   read out of a CALL's result — whose recorded container is the call's
        //   placeholder, not the container the call handed back — is still ordered ahead
        //   of the container that frees the page. An opcode read mints no region of its
        //   own and contributes no edge; its borrow is covered by the container's extended
        //   lifetime instead (region/rules.md Rule 4).
        //
        // `order_releases` topologically sorts the result (holder before holdee, nested
        // subtrees innermost-first); a single flat priority class cannot express a
        // transitive member ⊂ mid ⊂ root chain, nor a region that both is adopted and
        // reads out of another. The two sets stay SEPARATE because only the first is a
        // forest: a binding whose `binding_regions` name several alternatives (a
        // re-`def`ined or branch-union binding) makes two reads each other's container, so
        // the read edges can carry a may-alias cycle no order satisfies — which the sort
        // resolves by tie-break rather than treating as the impossible state an adopt-edge
        // cycle would be.
        // Both sets are indexed by their `earlier` endpoint once here, so the per-bucket
        // sort below costs the bucket's own size rather than a scan of every edge in the
        // unit (the O(n²)-over-the-stdlib trap the two indices above exist to avoid).
        let mut adopt_owner: HashMap<crate::hir::region::Region, Vec<crate::hir::region::Region>> =
            HashMap::new();
        for &(member, owner) in info
            .owned_adopt_edges
            .values()
            .flatten()
            .chain(info.capture_adopt_edges.values().flatten())
        {
            adopt_owner.entry(member).or_default().push(owner);
        }
        let mut value_alias: HashMap<crate::hir::region::Region, Vec<crate::hir::region::Region>> =
            HashMap::new();
        for &(_site, alias, source) in info
            .counted_read_aliases
            .iter()
            .chain(info.opaque_result_aliases.iter())
            .chain(info.funnel_result_containers.iter())
        {
            value_alias.entry(alias).or_default().push(source);
        }
        for regions in decrefs_by_decref_point.values_mut() {
            Self::order_releases(regions, &adopt_owner, &value_alias, &info);
        }
        // The fn-local 1-slot containers whose content drop lands at each scope
        // node, indexed the same way and for the same reason as the two above.
        let mut cell_drops_by_demise: HashMap<HirId, Vec<Binding>> = HashMap::new();
        for (&b, c) in &info.cell_containers {
            cell_drops_by_demise.entry(c.demise).or_default().push(b);
        }
        // Deterministic emission order across runs (the map's iteration is not).
        for bindings in cell_drops_by_demise.values_mut() {
            bindings.sort_unstable_by_key(|b| b.0);
        }
        self.increfs_by_site = increfs_by_site;
        self.decrefs_by_decref_point = decrefs_by_decref_point;
        self.cell_drops_by_demise = cell_drops_by_demise;
        self.region_info = info;
        self
    }

    /// Order the releases sharing one `decref_point` (docs/impl/region/rules.md Rule 4).
    ///
    /// A topological sort of the **holder-before-holdee** edges — `adopt_owner`
    /// (`member → owner`, the single-owner Owned-subtree forest) and `value_alias`
    /// (`alias → source`, every region that may be or live inside another: a borrowing
    /// read's result, an opaque call's result, a funnel's pass-through result) — so every
    /// store/capture-adopted member's own `DecrefRegion` — a no-op only while the member
    /// is still `Owned` — and every alias's page-reading `DecrefValueRegion` are emitted
    /// before the release that frees (or subtree-drops) what they name (region/adopt.md
    /// § "The lifetime obligation the root carries"). Both nested subtrees and chained
    /// aliases resolve innermost-first by construction — a single flat priority class
    /// cannot express a transitive member-before-owner chain, nor a read whose container
    /// is a call's placeholder for the region that actually frees the page.
    ///
    /// Regions no such edge relates are tie-broken by page-read depth: a value-gated
    /// `DecrefValueRegion` that unwraps a cell to the inner value reads deepest and sorts
    /// first (class 0), then a `DecrefCellRegion` that reads the cell header and frees the
    /// cell (class 1), then a plain `DecrefRegion` that frees and reads nothing (class 2);
    /// region id breaks the final tie, so the order never depends on `HashMap` iteration
    /// (the flaky capture-cell UAF, region-capture-cell-noreassign-uaf.lisp).
    ///
    /// **The two edge sets differ in what a cycle means.** Adoption is a forest — each
    /// member has exactly one owner (the two adopt maps are disjoint per member;
    /// region/info.rs) — so a cycle there is an impossible state a debug assert flags. A
    /// read edge is only a MAY-alias: a binding whose `binding_regions` name several
    /// alternatives (a re-`def`ined or branch-union binding) makes each alternative the
    /// other's container, so two reads through it can point at each other though no
    /// single runtime container does. Such a cycle is broken by re-sorting the stalled
    /// residue on the adopt edges alone, then by tie-break — deterministic, never a
    /// release-build panic on a legal program.
    fn order_releases(
        regions: &mut Vec<crate::hir::region::Region>,
        adopt_owner: &HashMap<crate::hir::region::Region, Vec<crate::hir::region::Region>>,
        value_alias: &HashMap<crate::hir::region::Region, Vec<crate::hir::region::Region>>,
        info: &RegionInfo,
    ) {
        use crate::hir::region::Region;
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;

        // Page-read depth: lower sorts earlier. `cell_release_regions ⊆
        // call_result_regions`, so test the cell membership first.
        let class = |r: Region| -> u8 {
            if info.cell_release_regions.contains(&r) {
                1 // DecrefCellRegion: reads the cell page header, frees the cell
            } else if info.call_result_regions.contains(&r) {
                0 // DecrefValueRegion: unwraps the cell to the inner value (deepest read)
            } else {
                2 // plain DecrefRegion: frees, reads nothing
            }
        };
        // Kahn's algorithm over the edges restricted to `bucket`. A `later` region waits
        // on every in-bucket `earlier` region that holds it; `waiters` lists the waiters of
        // each earlier region (a member has one owner, but a region can be an adopted
        // member AND a read alias, so the successor set is a list). The edge sets arrive
        // PRE-INDEXED by their `earlier` endpoint, so a bucket costs its own size, not a
        // scan of every edge in the unit — the same O(n²)-over-the-stdlib trap the
        // decref/incref indices above exist to avoid. Returns the ordered prefix and the
        // residue the edges never unblocked (a cycle).
        let kahn = |bucket: &[Region],
                    edges: &[&HashMap<Region, Vec<Region>>]|
         -> (Vec<Region>, Vec<Region>) {
            let present: rustc_hash::FxHashSet<Region> = bucket.iter().copied().collect();
            let mut indeg: HashMap<Region, u32> = bucket.iter().map(|&r| (r, 0)).collect();
            let mut succ: HashMap<Region, Vec<Region>> = HashMap::new();
            for &earlier in bucket {
                for &later in edges
                    .iter()
                    .filter_map(|e| e.get(&earlier))
                    .flatten()
                    .filter(|l| present.contains(l))
                {
                    *indeg.get_mut(&later).expect("later is in this bucket") += 1;
                    succ.entry(earlier).or_default().push(later);
                }
            }
            // Min-heap by (class, region id): the deterministic tie-break among the
            // currently-unblocked regions. `Region` has no `Ord`, so key on the id and
            // reconstruct — the id is unique within a bucket (`region_data` keys regions).
            let mut ready: BinaryHeap<Reverse<(u8, u32)>> = BinaryHeap::new();
            for &r in bucket {
                if indeg[&r] == 0 {
                    ready.push(Reverse((class(r), r.0)));
                }
            }
            let mut out: Vec<Region> = Vec::with_capacity(bucket.len());
            while let Some(Reverse((_, id))) = ready.pop() {
                let r = Region(id);
                out.push(r);
                for &waiter in succ.get(&r).into_iter().flatten() {
                    let d = indeg.get_mut(&waiter).expect("waiter tracked in indeg");
                    *d -= 1;
                    if *d == 0 {
                        ready.push(Reverse((class(waiter), waiter.0)));
                    }
                }
            }
            let residue: Vec<Region> = bucket
                .iter()
                .copied()
                .filter(|r| !out.contains(r))
                .collect();
            (out, residue)
        };

        let (mut out, residue) = kahn(regions, &[adopt_owner, value_alias]);
        if !residue.is_empty() {
            // A may-alias read cycle: drop the read edges over the stalled residue and
            // order it on the adopt forest alone.
            let (rest, cyclic) = kahn(&residue, &[adopt_owner]);
            out.extend(rest);
            debug_assert!(
                cyclic.is_empty(),
                "release-order ADOPT edges cycled at a shared decref_point: {cyclic:?}"
            );
            let mut cyclic = cyclic;
            cyclic.sort_by_key(|r| (class(*r), r.0));
            out.extend(cyclic);
        }
        *regions = out;
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
        // through a single chokepoint; `tail_callee_defers_release` reads it.
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
