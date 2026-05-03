//! HIR to LIR lowering

mod access;
mod binding;
mod control;
pub(crate) mod decision;
mod escape;
mod expr;
mod lambda;
mod pattern;

use super::intrinsics::IntrinsicOp;
use super::types::*;
use crate::hir::arena::BindingArena;
use crate::hir::region::{RegionInfo, RegionKind};
use crate::hir::{Binding, BlockId, CallArg, Hir, HirId, HirKind, HirPattern};
use crate::syntax::Span;
use crate::value::fiber::SignalBits;
use crate::value::{Arity, SymbolId, Value};
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

static GLOBAL_SCOPE_STATS: Mutex<ScopeStats> = Mutex::new(ScopeStats {
    scopes_analyzed: 0,
    scopes_qualified: 0,
    rejected_captured: 0,
    rejected_suspends: 0,
    rejected_unsafe_result: 0,
    rejected_outward_set: 0,
    rejected_break: 0,
    calls_scoped: 0,
    rotation_analyzed: 0,
    rotation_safe: 0,
});

/// Merge local scope stats into the global accumulator.
pub fn accumulate_scope_stats(stats: &ScopeStats) {
    if let Ok(mut global) = GLOBAL_SCOPE_STATS.lock() {
        global.merge(stats);
    }
}

/// Read the global scope stats.
pub fn global_scope_stats() -> ScopeStats {
    GLOBAL_SCOPE_STATS
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default()
}

/// Wrap `func`'s body with `FlipEnter`/`FlipExit` and insert `FlipSwap`
/// before every tail call. Used by Phase 4b auto-insertion
/// (gated by `config::flip_enabled()`).
///
/// The resulting LIR is semantically equivalent under the runtime's
/// existing rotation mechanism — `FlipSwap` tears down the previous
/// iteration's allocations at each tail-call boundary the same way
/// the trampoline does, and `FlipExit` tears down the trailing
/// generation when the function returns.
fn inject_flip(func: &mut LirFunction) {
    // Locate the entry block: prepend FlipEnter at the top.
    if let Some(entry_block) = func.blocks.iter_mut().find(|b| b.label == func.entry) {
        entry_block
            .instructions
            .insert(0, SpannedInstr::new(LirInstr::FlipEnter, Span::synthetic()));
    }

    for block in &mut func.blocks {
        // Insert FlipSwap immediately before every tail call.
        let mut i = 0;
        while i < block.instructions.len() {
            if matches!(
                block.instructions[i].instr,
                LirInstr::TailCall { .. } | LirInstr::TailCallArrayMut { .. }
            ) {
                block
                    .instructions
                    .insert(i, SpannedInstr::new(LirInstr::FlipSwap, Span::synthetic()));
                i += 2;
            } else {
                i += 1;
            }
        }

        // Insert FlipExit before every Return terminator. (TailCalls
        // leave the frame without a Return, so their exit is subsumed
        // by the next frame's FlipExit on its own return.)
        if matches!(block.terminator.terminator, Terminator::Return(_)) {
            block
                .instructions
                .push(SpannedInstr::new(LirInstr::FlipExit, Span::synthetic()));
        }
    }

    // While-loop flip frames: detect back-edges from the CFG and inject
    // FlipEnter/FlipSwap/FlipExit around each loop so per-iteration
    // allocations are reclaimed.
    //
    // Pattern: entry→Jump(cond), cond→Branch{body,done}, back_edge→Jump(cond).
    // Detect Branch blocks with exactly two Jump predecessors (forward entry +
    // backward back-edge), distinguished by block order.
    inject_flip_while_loops(func);
}

/// Inject per-loop FlipEnter/FlipSwap/FlipExit using the
/// `while_loops` metadata recorded during lowering. Each triple
/// `(entry, back_edge, done)` has already passed escape analysis.
fn inject_flip_while_loops(func: &mut LirFunction) {
    for &(entry_label, back_edge_label, done_label) in &func.while_loops.clone() {
        if let Some(block) = func.blocks.iter_mut().find(|b| b.label == entry_label) {
            block
                .instructions
                .push(SpannedInstr::new(LirInstr::FlipEnter, Span::synthetic()));
        }
        if let Some(block) = func.blocks.iter_mut().find(|b| b.label == back_edge_label) {
            block
                .instructions
                .push(SpannedInstr::new(LirInstr::FlipSwap, Span::synthetic()));
        }
        if let Some(block) = func.blocks.iter_mut().find(|b| b.label == done_label) {
            block
                .instructions
                .insert(0, SpannedInstr::new(LirInstr::FlipExit, Span::synthetic()));
        }
    }
}

/// Compile-time scope allocation statistics.
///
/// Tracks how many let/letrec/block scopes were analyzed for scope
/// allocation, how many qualified, and why the rest were rejected.
/// The rejection reason is the *first* failing condition (conditions
/// are checked in order and short-circuit).
#[derive(Debug, Clone, Default)]
pub struct ScopeStats {
    /// Total scopes evaluated for scope allocation
    pub scopes_analyzed: usize,
    /// Scopes that passed all conditions (RegionEnter/RegionExit emitted)
    pub scopes_qualified: usize,
    /// Scopes rejected because a binding is captured (condition 1)
    pub rejected_captured: usize,
    /// Scopes rejected because body may suspend (condition 2)
    pub rejected_suspends: usize,
    /// Scopes rejected because result is not provably immediate (condition 3)
    pub rejected_unsafe_result: usize,
    /// Scopes rejected because body contains set to outer binding (condition 4)
    pub rejected_outward_set: usize,
    /// Scopes rejected because body contains break (condition 5)
    pub rejected_break: usize,
    /// Non-tail calls wrapped in RegionEnter/RegionExitCall
    pub calls_scoped: usize,
    /// Functions analyzed for rotation safety
    pub rotation_analyzed: usize,
    /// Functions that qualified as rotation-safe
    pub rotation_safe: usize,
}

impl ScopeStats {
    /// Total rejected scopes (analyzed - qualified).
    pub fn scopes_rejected(&self) -> usize {
        self.scopes_analyzed - self.scopes_qualified
    }

    /// Merge another ScopeStats into this one (for aggregating across lowerer invocations).
    pub fn merge(&mut self, other: &ScopeStats) {
        self.scopes_analyzed += other.scopes_analyzed;
        self.scopes_qualified += other.scopes_qualified;
        self.rejected_captured += other.rejected_captured;
        self.rejected_suspends += other.rejected_suspends;
        self.rejected_unsafe_result += other.rejected_unsafe_result;
        self.rejected_outward_set += other.rejected_outward_set;
        self.rejected_break += other.rejected_break;
        self.calls_scoped += other.calls_scoped;
        self.rotation_analyzed += other.rotation_analyzed;
        self.rotation_safe += other.rotation_safe;
    }
}

impl fmt::Display for ScopeStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "scope allocation stats:")?;
        writeln!(
            f,
            "  analyzed: {}  qualified: {}  rejected: {}",
            self.scopes_analyzed,
            self.scopes_qualified,
            self.scopes_rejected()
        )?;
        if self.scopes_rejected() > 0 {
            writeln!(f, "  rejection reasons:")?;
            if self.rejected_captured > 0 {
                writeln!(f, "    captured:      {}", self.rejected_captured)?;
            }
            if self.rejected_suspends > 0 {
                writeln!(f, "    suspends:      {}", self.rejected_suspends)?;
            }
            if self.rejected_unsafe_result > 0 {
                writeln!(f, "    unsafe-result: {}", self.rejected_unsafe_result)?;
            }
            if self.rejected_outward_set > 0 {
                writeln!(f, "    outward-set:   {}", self.rejected_outward_set)?;
            }
            if self.rejected_break > 0 {
                writeln!(f, "    break:         {}", self.rejected_break)?;
            }
        }
        if self.calls_scoped > 0 {
            writeln!(f, "  call-scoped:   {}", self.calls_scoped)?;
        }
        if self.rotation_analyzed > 0 {
            writeln!(
                f,
                "  rotation:      {}/{} safe",
                self.rotation_safe, self.rotation_analyzed
            )?;
        }
        Ok(())
    }
}

/// Tracks an active Loop during lowering so `Recur` can find its
/// entry label and binding slots.
struct LoopLowerContext {
    loop_label: Label,
    binding_slots: Vec<u16>,
    scope_eligible: bool,
    /// Whether RegionRotate should also dealloc slab slots.
    dealloc_eligible: bool,
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
    /// compensating `RegionExit` instructions before jumping to the exit.
    region_depth_at_entry: u32,
    /// The `flip_depth` at the time this block was entered.
    /// `break` emits compensating `FlipExit` instructions for each
    /// flip frame entered since the block was opened.
    flip_depth_at_entry: u32,
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
    /// Primitives known to return immediates.
    /// Used by escape analysis (`result_is_safe`) to accept calls to
    /// these primitives in scope-allocated let bodies.
    immediate_primitives: FxHashSet<SymbolId>,
    mutating_primitives: FxHashSet<SymbolId>,
    /// Primitives that insert args into collections (push, put).
    arg_escaping_primitives: FxHashSet<SymbolId>,
    /// Primitives that return pre-existing values without allocating.
    non_allocating_accessors: FxHashSet<SymbolId>,
    /// Stdlib functions known to not escape heap values.
    non_escaping_stdlib: FxHashSet<SymbolId>,
    /// Binding → rotation_safe for lowered lambdas. Populated during
    /// lowering so that `body_escapes_heap_values` can check callees
    /// transitively: a call to a rotation-safe function doesn't escape.
    callee_rotation_safe: HashMap<Binding, bool>,
    /// Binding → return_safe for function definitions.
    /// A function is return-safe if its body never returns a freshly
    /// heap-allocated value (returns immediates, Vars, or results of
    /// other return-safe calls). Precomputed via fixpoint iteration.
    /// Used by `tail_arg_is_safe_extended` and `result_is_safe_extended`
    /// to see through call boundaries that `call_result_is_safe` rejects
    /// (e.g. letrec-bound functions).
    callee_return_safe: HashMap<Binding, bool>,
    /// Binding → result_is_immediate for function definitions.
    /// Precomputed via fixpoint iteration so that `call_result_is_safe`
    /// can identify user functions that always return immediates.
    callee_result_immediate: HashMap<Binding, bool>,
    /// Binding → bitmask of parameter indices that may flow to return.
    /// Precomputed via fixpoint iteration. Bit i set means param i
    /// might be returned identity-unchanged by the function.
    callee_return_params: HashMap<Binding, u64>,
    /// Binding → `Some(rest_index)` for variadic user functions (those
    /// with `&opt`/`&named`/`&keys`/`&args`). When set, call-site
    /// args at position >= rest_index all collapse into the rest param.
    /// `can_scope_allocate_call` needs this to account for the
    /// many-to-one arg-to-param mapping when deciding whether heap
    /// args might flow into the callee's return.
    callee_rest_index: HashMap<Binding, usize>,
    /// Compile-time constant values for immutable bindings (for LoadConst optimization)
    immutable_values: HashMap<Binding, Value>,
    /// Stack of active loop contexts for `Recur` lowering
    loop_lower_contexts: Vec<LoopLowerContext>,
    /// Stack of active block contexts for `break` lowering
    block_lower_contexts: Vec<BlockLowerContext>,
    /// Current nesting depth of active allocation regions.
    /// Incremented on `RegionEnter`, decremented on `RegionExit`.
    /// Used by `lower_break` to emit compensating `RegionExit`s.
    region_depth: u32,
    /// Current nesting depth of while-loop flip frames.
    /// Incremented when entering a flip-eligible while loop,
    /// decremented when leaving. Used by `lower_break` to emit
    /// compensating `FlipExit` instructions.
    flip_depth: u32,
    pending_region_exits: u32,
    /// Compile-time scope allocation statistics.
    scope_stats: ScopeStats,
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
    /// Parameter bindings of the current function (for per-parameter
    /// independence analysis in self-tail-calls).
    current_function_params: Option<Vec<Binding>>,
    /// Tofte-Talpin region inference results. Scope decisions use region
    /// assignments instead of syntactic escape analysis.
    region_info: RegionInfo,
}

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
            immediate_primitives: FxHashSet::default(),
            mutating_primitives: FxHashSet::default(),
            arg_escaping_primitives: FxHashSet::default(),
            non_allocating_accessors: FxHashSet::default(),
            non_escaping_stdlib: FxHashSet::default(),
            callee_rotation_safe: HashMap::new(),
            callee_return_safe: HashMap::new(),
            callee_result_immediate: HashMap::new(),
            callee_return_params: HashMap::new(),
            callee_rest_index: HashMap::new(),
            immutable_values: HashMap::new(),
            loop_lower_contexts: Vec::new(),
            block_lower_contexts: Vec::new(),
            region_depth: 0,
            flip_depth: 0,
            pending_region_exits: 0,
            scope_stats: ScopeStats::default(),
            discard_slot: None,
            symbol_names: HashMap::new(),
            closures: Vec::new(),
            current_function_binding: None,
            current_function_params: None,
            region_info: RegionInfo::empty(),
        }
    }

    /// Set intrinsic operations for operator specialization
    pub(crate) fn with_intrinsics(mut self, intrinsics: FxHashMap<SymbolId, IntrinsicOp>) -> Self {
        self.intrinsics = intrinsics;
        self
    }

    /// Set the whitelist of primitives known to return immediates
    pub fn with_immediate_primitives(mut self, set: FxHashSet<SymbolId>) -> Self {
        self.immediate_primitives = set;
        self
    }

    pub fn with_mutating_primitives(mut self, set: FxHashSet<SymbolId>) -> Self {
        self.mutating_primitives = set;
        self
    }

    pub fn with_arg_escaping_primitives(mut self, set: FxHashSet<SymbolId>) -> Self {
        self.arg_escaping_primitives = set;
        self
    }

    pub fn with_non_allocating_accessors(mut self, set: FxHashSet<SymbolId>) -> Self {
        self.non_allocating_accessors = set;
        self
    }

    pub fn with_non_escaping_stdlib(mut self, set: FxHashSet<SymbolId>) -> Self {
        self.non_escaping_stdlib = set;
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
        self.region_info = info;
        self
    }

    /// Check region inference for a scope node.
    fn region_scope_check(&self, hir_id: HirId) -> bool {
        matches!(
            self.region_info.scope_kind.get(&hir_id),
            Some(RegionKind::Scope)
        )
    }

    /// Check region inference for a loop node.
    #[allow(dead_code)]
    fn region_loop_check(&self, hir_id: HirId) -> bool {
        matches!(
            self.region_info.scope_kind.get(&hir_id),
            Some(RegionKind::Loop | RegionKind::Scope)
        )
    }

    /// Return compile-time scope allocation statistics.
    pub fn scope_stats(&self) -> &ScopeStats {
        &self.scope_stats
    }

    /// Lower a HIR expression to an LIR module.
    ///
    /// Returns an `LirModule` with the entry function and a flat list of
    /// closure bodies. Each closure is an independent compilation unit
    /// referenced by `ClosureId`.
    pub fn lower(&mut self, hir: &Hir) -> Result<LirModule, String> {
        self.current_func = LirFunction::new(Arity::Exact(0));
        self.current_block = BasicBlock::new(Label(0));
        self.next_reg = 0;
        self.next_label = 1;
        self.binding_to_slot.clear();
        self.discard_slot = None;
        self.closures.clear();

        // Precompute callee properties for scope allocation decisions.
        // These scan the HIR for lambda defs and record per-binding
        // properties (result_is_immediate, return_params, return_safe,
        // rotation_safe). Order matters: return_safe depends on nothing;
        // rotation_safety depends on return_safe (via tail_arg_is_safe_extended).
        self.precompute_result_immediate(hir);
        self.precompute_return_params(hir);
        self.precompute_rest_index(hir);
        self.precompute_return_safe(hir);
        self.precompute_rotation_safety(hir);

        let result_reg = self.lower_expr(hir)?;
        self.terminate(Terminator::Return(result_reg));
        self.finish_block();

        self.current_func.entry = Label(0);
        self.current_func.num_regs = self.next_reg;
        // Propagate signal from HIR to top-level LIR function
        self.current_func.signal = hir.signal;

        // Compute escape analysis flags for fiber shared-alloc decisions.
        // Covers closures created from top-level `defn` forms passed to
        // `fiber/new` as variables.
        self.current_func.result_is_immediate = self.result_is_safe(hir, &[]);
        self.current_func.has_outward_heap_set = self.body_contains_dangerous_outward_set(hir, &[]);
        self.current_func.rotation_safe = !self.body_escapes_heap_values(hir);

        let mut entry =
            std::mem::replace(&mut self.current_func, LirFunction::new(Arity::Exact(0)));
        let mut closures = std::mem::take(&mut self.closures);

        // Phase 4b: optional FlipEnter/FlipSwap/FlipExit injection. The
        // pass is a no-op unless `--flip=on` or the vm/config equivalent
        // is set. It runs after lowering so it doesn't perturb any
        // scope/rotation analysis upstream.
        if crate::config::flip_enabled() {
            inject_flip(&mut entry);
            for f in &mut closures {
                inject_flip(f);
            }
        }

        Ok(LirModule { entry, closures })
    }

    // === Helper Methods ===

    fn fresh_reg(&mut self) -> Reg {
        let r = Reg::new(self.next_reg);
        self.next_reg += 1;
        r
    }

    fn allocate_slot(&mut self, binding: Binding) -> u16 {
        // Inside a lambda, two address spaces coexist:
        //   - Env (captures + params + LBox locals): LoadCapture/StoreCapture
        //   - Stack/register locals (non-LBox let-bound): LoadLocal/StoreLocal
        //
        // Environment layout: [captures..., params..., lbox_locals..., nil_placeholders...]
        // Stack frame layout:  [params..., all_locally_defined...]
        //
        // LBox locals get ENV-relative slots (num_captures + num_locals).
        // Non-LBox locals get STACK-relative slots (num_locals).
        // Both increment num_locals to keep env placeholder slots aligned.
        let needs_capture = self.arena.get(binding).needs_capture();
        let slot = if self.in_lambda {
            // local_index is relative to locally-defined vars (after param locals)
            let local_index = self.current_func.num_locals - self.num_local_params;
            if needs_capture && local_index < 64 {
                self.current_func.capture_locals_mask |= 1 << local_index;
            }
            if needs_capture {
                // Env-relative: for LoadCapture/StoreCapture
                self.num_captures + self.current_func.num_locals
            } else {
                // Stack-relative: for LoadLocal/StoreLocal
                self.current_func.num_locals
            }
        } else {
            self.current_func.num_locals
        };
        self.current_func.num_locals += 1;
        self.binding_to_slot.insert(binding, slot);
        slot
    }

    /// Extract a compile-time constant value from an HIR node.
    /// Returns `Some(value)` for literals and references to already-known
    /// constants. Used to seed `immutable_values` so reads of immutable
    /// bindings emit `LoadConst` instead of `LoadLocal`.
    fn hir_const_value(&self, hir: &Hir) -> Option<Value> {
        match &hir.kind {
            HirKind::Int(n) => Some(Value::int(*n)),
            HirKind::Float(f) => Some(Value::float(*f)),
            HirKind::Bool(b) => Some(Value::bool(*b)),
            HirKind::Nil => Some(Value::NIL),
            HirKind::Keyword(k) => Some(Value::keyword(k)),
            HirKind::Quote(v) => Some(*v),
            // Propagate through references to known constants
            HirKind::Var(b) => self.immutable_values.get(b).copied(),
            _ => None,
        }
    }

    /// If `binding` is immutable and `init` is a compile-time constant,
    /// record it in `immutable_values` so that subsequent reads of this
    /// binding emit `LoadConst` instead of slot loads.
    fn try_seed_immutable(&mut self, binding: Binding, init: &Hir) {
        if self.arena.get(binding).is_immutable {
            if let Some(val) = self.hir_const_value(init) {
                self.immutable_values.insert(binding, val);
            }
        }
    }

    fn emit(&mut self, instr: LirInstr) {
        self.current_block
            .instructions
            .push(SpannedInstr::new(instr, self.current_span.clone()));
    }

    fn emit_const(&mut self, c: LirConst) -> Result<Reg, String> {
        let dst = self.fresh_reg();
        self.emit(LirInstr::Const { dst, value: c });
        Ok(dst)
    }

    fn emit_value_const(&mut self, value: Value) -> Result<Reg, String> {
        let dst = self.fresh_reg();
        self.emit(LirInstr::ValueConst { dst, value });
        Ok(dst)
    }

    fn terminate(&mut self, term: Terminator) {
        self.current_block.terminator = SpannedTerminator::new(term, self.current_span.clone());
    }

    fn finish_block(&mut self) {
        let block = std::mem::replace(&mut self.current_block, BasicBlock::new(Label(0)));
        self.current_func.blocks.push(block);
    }

    /// Allocate a new basic block label.
    fn fresh_label(&mut self) -> Label {
        let label = Label(self.next_label);
        self.next_label += 1;
        label
    }

    /// Finish the current block and start a new one with the given label.
    fn start_new_block(&mut self, label: Label) {
        self.finish_block();
        self.current_block = BasicBlock::new(label);
    }

    /// Emit `RegionEnter` and increment the region depth counter.
    fn emit_region_enter(&mut self) {
        self.emit(LirInstr::RegionEnter);
        self.region_depth += 1;
    }

    /// Emit `RegionExit` and decrement the region depth counter.
    fn emit_region_exit(&mut self) {
        self.emit(LirInstr::RegionExit);
        self.region_depth -= 1;
    }

    /// Emit `RegionRotate` for double-buffered loop scope rotation.
    /// Does not change region_depth — the mark count stays the same
    /// (pop prev + push new = net zero change from the 2-mark state).
    fn emit_region_rotate(&mut self) {
        self.emit(LirInstr::RegionRotate);
    }

    fn emit_region_rotate_dealloc(&mut self) {
        self.emit(LirInstr::RegionRotateDealloc);
    }

    /// Discard an unused value by storing it to a scratch slot.
    /// The emitter's auto-pop after StoreLocal cleans up the value
    /// from the operand stack. The scratch slot is lazily allocated
    /// on first use and reused for all discards in the function.
    fn discard(&mut self, src: Reg) {
        let slot = match self.discard_slot {
            Some(s) => s,
            None => {
                let s = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.discard_slot = Some(s);
                s
            }
        };
        self.emit(LirInstr::StoreLocal { slot, src });
    }

    // ── Escape analysis ────────────────────────────────────────────
    //
    // See `escape.rs` for helper functions (`result_is_safe`,
    // `body_contains_dangerous_outward_set`, `body_contains_escaping_break`,
    // `all_break_values_safe`, `all_breaks_have_safe_values`).

    /// Determine if a `let` scope's allocations can be safely released
    /// at scope exit via `RegionEnter`/`RegionExit`.
    ///
    /// Performs escape analysis on the let body to check if all bindings
    /// and intermediate values allocated within the scope can be freed
    /// when the scope exits. This enables the lowerer to emit `RegionEnter`
    /// and `RegionExit` instructions for automatic cleanup.
    ///
    /// Returns `true` when ALL six conditions hold:
    /// 1. No binding is captured by a nested lambda (captured values escape)
    /// 2. Body cannot suspend (yield/debug/polymorphic signals prevent cleanup)
    /// 3. Body result is provably an immediate (not heap-allocated)
    /// 4. Body contains no dangerous outward `set` (set to outer binding
    ///    with a value that could be heap-allocated inside the scope)
    /// 5. All breaks in body carry safe immediate values
    /// 6. Body contains no `break` targeting outer blocks (break carries
    ///    a value past RegionExit, causing use-after-free)
    ///
    /// Increments `scope_stats.scopes_analyzed` and updates rejection counters
    /// for each failed condition (short-circuits on first failure).
    #[allow(dead_code)]
    fn can_scope_allocate_let(&mut self, bindings: &[(Binding, Hir)], body: &Hir) -> bool {
        self.scope_stats.scopes_analyzed += 1;
        // Condition 1: no captures
        if bindings.iter().any(|(b, _)| self.arena.get(*b).is_captured) {
            self.scope_stats.rejected_captured += 1;
            return false;
        }

        // Condition 2: no suspension in body or binding inits.
        // Binding init expressions are evaluated inside the region, so
        // allocations made by callees during a binding init are freed by
        // RegionExit. If a binding init suspends, the caller's body may
        // later create heap objects that escape via side effects (e.g. put
        // to an external mutable struct), and RegionExit would free them
        // while they're still referenced externally.
        //
        // Exception: when the body is a PURE tail call (no preceding
        // expressions that could suspend), the tail call's signal doesn't
        // matter — RegionExit fires before the tail call executes.
        // But if the body has non-tail sub-expressions that may suspend
        // (e.g. `(begin (port/write p x) (tail-call))`), those expressions
        // run within the scope and suspension is still dangerous.
        let body_suspends = if Self::body_is_tail_call(body) {
            Self::non_tail_subexprs_may_suspend(body)
        } else {
            body.signal.may_suspend()
        };
        if body_suspends || bindings.iter().any(|(_, init)| init.signal.may_suspend()) {
            self.scope_stats.rejected_suspends += 1;
            return false;
        }

        // Build scope binding refs once — used by conditions 3 and 4
        let scope_binding_refs: Vec<(Binding, &Hir)> =
            bindings.iter().map(|(b, init)| (*b, init)).collect();

        // Condition 3: result is immediate
        if !self.result_is_safe(body, &scope_binding_refs) {
            self.scope_stats.rejected_unsafe_result += 1;
            return false;
        }

        // Condition 3b: tail call callee must not be scope-bound.
        // RegionExit fires before tail calls, so if the callee is a
        // closure allocated inside the scope, its slot is freed before
        // the tail call reads it.
        if Self::tail_call_callee_is_scope_bound(body, &scope_binding_refs) {
            self.scope_stats.rejected_unsafe_result += 1;
            return false;
        }

        // Condition 4: no dangerous outward mutation
        if self.body_contains_dangerous_outward_set(body, &scope_binding_refs) {
            self.scope_stats.rejected_outward_set += 1;
            return false;
        }

        // Condition 5: all breaks carry safe immediate values.
        if !self.all_breaks_have_safe_values(body) {
            self.scope_stats.rejected_break += 1;
            return false;
        }

        // Condition 6: no escaping break.
        if Self::hir_contains_escaping_break(body) {
            self.scope_stats.rejected_break += 1;
            return false;
        }

        self.scope_stats.scopes_qualified += 1;
        true
    }

    /// Determine if a `letrec` scope's allocations can be safely released.
    /// Identical analysis to `let` — letrec's mutual recursion and two-phase
    /// initialization don't change the escape conditions.
    #[allow(dead_code)]
    fn can_scope_allocate_letrec(&mut self, bindings: &[(Binding, Hir)], body: &Hir) -> bool {
        self.can_scope_allocate_let(bindings, body)
    }

    /// Determine if a `block` scope's allocations can be safely released.
    ///
    /// Blocks don't introduce bindings but bracket a scope of allocations.
    /// Conditions:
    /// 1. No expression in body can suspend
    /// 2. Body result is provably immediate
    /// 3. All break values targeting this block are safe immediates
    /// 4. No `set!` to non-local bindings (blocks have no own bindings)
    #[allow(dead_code)]
    fn can_scope_allocate_block(&mut self, block_id: &BlockId, body: &[Hir]) -> bool {
        self.scope_stats.scopes_analyzed += 1;
        // Condition 1: no suspension
        if body.iter().any(|e| e.signal.may_suspend()) {
            self.scope_stats.rejected_suspends += 1;
            return false;
        }

        // Collect Define bindings from the block body. Although blocks
        // don't introduce let-style bindings, they can contain def/var
        // statements that create bindings whose values are heap-allocated
        // inside the scope. These must be tracked so result_is_safe
        // doesn't treat them as pre-scope outer bindings.
        let scope_bindings: Vec<(Binding, &Hir)> = body
            .iter()
            .filter_map(|e| match &e.kind {
                HirKind::Define { binding, value } => Some((*binding, value.as_ref())),
                _ => None,
            })
            .collect();

        // B2: result is immediate (empty body → nil → safe)
        if let Some(last) = body.last() {
            if !self.result_is_safe(last, &scope_bindings) {
                self.scope_stats.rejected_unsafe_result += 1;
                return false;
            }
        }

        // Condition 3: all break values targeting this block are safe immediates.
        if !self.all_break_values_safe(body, *block_id, &scope_bindings) {
            self.scope_stats.rejected_break += 1;
            return false;
        }

        // Condition 4: no dangerous outward mutation
        if body
            .iter()
            .any(|e| self.body_contains_dangerous_outward_set(e, &scope_bindings))
        {
            self.scope_stats.rejected_outward_set += 1;
            return false;
        }

        self.scope_stats.scopes_qualified += 1;
        true
    }

    /// Determine if a non-tail call's temporaries can be freed after the
    /// call returns via `RegionEnter`/`RegionExit` around the call.
    ///
    /// Safe when ALL conditions hold:
    /// 1. Callee is a known function that returns an immediate
    /// 2. Callee is rotation-safe (doesn't escape heap values)
    /// 3. Call doesn't suspend (conservative: no yielding anywhere)
    /// 4. At least one argument may heap-allocate (otherwise no benefit)
    /// 5. No spliced arguments (splice path builds an array; more complex)
    fn can_scope_allocate_call(
        &self,
        func: &Hir,
        args: &[CallArg],
        _call_signals: SignalBits,
    ) -> bool {
        // Must be a variable reference to a known function
        let HirKind::Var(binding) = &func.kind else {
            return false;
        };

        // Condition 1: no spliced args
        if args.iter().any(|a| a.spliced) {
            return false;
        }

        // Condition 2: callee's return value won't alias a freed arg.
        let callee_rp = self
            .callee_return_params
            .get(binding)
            .copied()
            .unwrap_or(!0);
        let rest_index = self.callee_rest_index.get(binding).copied();
        for (i, arg) in args.iter().enumerate() {
            let param_slot = match rest_index {
                Some(r) if i >= r => r,
                _ => i,
            };
            if param_slot < 64
                && (callee_rp & (1u64 << param_slot)) != 0
                && !self.result_is_safe(&arg.expr, &[])
            {
                return false;
            }
        }

        // Condition 3: callee doesn't escape heap values.
        let rotation_safe = self
            .callee_rotation_safe
            .get(binding)
            .copied()
            .unwrap_or(false);
        if !rotation_safe {
            return false;
        }

        // Condition 4: argument evaluation must not suspend.
        if args.iter().any(|a| a.expr.signal.may_suspend()) {
            return false;
        }

        // Condition 5: at least one arg may heap-allocate
        // (if all args are immediates, the region has nothing to reclaim)
        args.iter().any(|a| !self.result_is_safe(&a.expr, &[]))
    }

    /// Precompute `callee_result_immediate` for all function definitions.
    ///
    /// Fixpoint iteration: seed all functions as "returns immediate",
    /// then iterate `result_is_safe(body, &[])` until stable. A function
    /// whose body calls another function that returns a non-immediate
    /// will converge to non-immediate.
    fn precompute_result_immediate(&mut self, hir: &Hir) {
        let mut defs: Vec<(Binding, &Hir)> = Vec::new();
        Self::collect_lambda_defs(hir, &mut defs);
        if defs.is_empty() {
            return;
        }

        // Seed: all functions optimistically return immediates.
        for &(binding, _) in &defs {
            self.callee_result_immediate.insert(binding, true);
        }

        // Iterate until stable.
        loop {
            let mut changed = false;
            for &(binding, body) in &defs {
                let is_imm = self.body_result_is_immediate(body);
                let was_imm = self.callee_result_immediate[&binding];
                if was_imm && !is_imm {
                    self.callee_result_immediate.insert(binding, false);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// Precompute `callee_return_safe` for all function definitions.
    ///
    /// A function is return-safe if its body's result is provably non-heap-
    /// allocated, considering calls to other return-safe functions as safe.
    /// Uses `result_is_safe_extended` (which trusts `callee_return_safe`)
    /// in a fixpoint iteration: seed all functions as return-safe, then
    /// iterate until stable. Only transitions safe→unsafe (monotone).
    ///
    /// This enables `tail_arg_is_safe_extended` to see through call
    /// boundaries that `call_result_is_safe` conservatively rejects
    /// (e.g. letrec-bound functions like nqueens' `search`).
    fn precompute_return_safe(&mut self, hir: &Hir) {
        let mut defs: Vec<(Binding, &Hir)> = Vec::new();
        Self::collect_lambda_defs(hir, &mut defs);
        if defs.is_empty() {
            return;
        }

        // Seed: all functions optimistically return-safe.
        for &(binding, _) in &defs {
            self.callee_return_safe.insert(binding, true);
        }

        // Iterate until stable.
        loop {
            let mut changed = false;
            for &(binding, body) in &defs {
                let is_safe = self.result_is_safe_extended(body, &[]);
                let was_safe = self.callee_return_safe[&binding];
                if was_safe && !is_safe {
                    self.callee_return_safe.insert(binding, false);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// Precompute `callee_rest_index` for all user functions. The rest
    /// param index is `params.len() - 1` when `rest_param` is `Some`
    /// (the rest-collecting binding is the last entry in `params`).
    fn precompute_rest_index(&mut self, hir: &Hir) {
        Self::collect_rest_indices(hir, &mut self.callee_rest_index);
    }

    fn collect_rest_indices(hir: &Hir, out: &mut HashMap<Binding, usize>) {
        match &hir.kind {
            HirKind::Define { binding, value } => {
                if let HirKind::Lambda {
                    params, rest_param, ..
                } = &value.kind
                {
                    if rest_param.is_some() && !params.is_empty() {
                        out.insert(*binding, params.len() - 1);
                    }
                }
                Self::collect_rest_indices(value, out);
            }
            HirKind::Begin(exprs) => {
                for e in exprs {
                    Self::collect_rest_indices(e, out);
                }
            }
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                for (binding, init) in bindings {
                    if let HirKind::Lambda {
                        params, rest_param, ..
                    } = &init.kind
                    {
                        if rest_param.is_some() && !params.is_empty() {
                            out.insert(*binding, params.len() - 1);
                        }
                    }
                    Self::collect_rest_indices(init, out);
                }
                Self::collect_rest_indices(body, out);
            }
            HirKind::Lambda { body, .. } => {
                Self::collect_rest_indices(body, out);
            }
            // Recurse into all structural nodes to find nested lambda defs
            HirKind::While { cond, body } => {
                Self::collect_rest_indices(cond, out);
                Self::collect_rest_indices(body, out);
            }
            HirKind::Loop { bindings, body } => {
                for (_, init) in bindings {
                    Self::collect_rest_indices(init, out);
                }
                Self::collect_rest_indices(body, out);
            }
            HirKind::Recur { args } => {
                for a in args {
                    Self::collect_rest_indices(a, out);
                }
            }
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                Self::collect_rest_indices(cond, out);
                Self::collect_rest_indices(then_branch, out);
                Self::collect_rest_indices(else_branch, out);
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                for (c, b) in clauses {
                    Self::collect_rest_indices(c, out);
                    Self::collect_rest_indices(b, out);
                }
                if let Some(e) = else_branch {
                    Self::collect_rest_indices(e, out);
                }
            }
            HirKind::Block { body, .. } => {
                for e in body {
                    Self::collect_rest_indices(e, out);
                }
            }
            HirKind::Break { value, .. } => Self::collect_rest_indices(value, out),
            HirKind::Match { value, arms } => {
                Self::collect_rest_indices(value, out);
                for (_, guard, body) in arms {
                    if let Some(g) = guard {
                        Self::collect_rest_indices(g, out);
                    }
                    Self::collect_rest_indices(body, out);
                }
            }
            HirKind::Call { func, args, .. } => {
                Self::collect_rest_indices(func, out);
                for a in args {
                    Self::collect_rest_indices(&a.expr, out);
                }
            }
            HirKind::Assign { value, .. } => Self::collect_rest_indices(value, out),
            HirKind::And(exprs) | HirKind::Or(exprs) => {
                for e in exprs {
                    Self::collect_rest_indices(e, out);
                }
            }
            HirKind::Emit { value, .. } => Self::collect_rest_indices(value, out),
            HirKind::Destructure { value, .. } => Self::collect_rest_indices(value, out),
            HirKind::Eval { expr, env } => {
                Self::collect_rest_indices(expr, out);
                Self::collect_rest_indices(env, out);
            }
            HirKind::Parameterize { bindings, body } => {
                for (k, v) in bindings {
                    Self::collect_rest_indices(k, out);
                    Self::collect_rest_indices(v, out);
                }
                Self::collect_rest_indices(body, out);
            }
            HirKind::MakeCell { value } => Self::collect_rest_indices(value, out),
            HirKind::DerefCell { cell } => Self::collect_rest_indices(cell, out),
            HirKind::SetCell { cell, value } => {
                Self::collect_rest_indices(cell, out);
                Self::collect_rest_indices(value, out);
            }
            HirKind::Intrinsic { args, .. } => {
                for a in args {
                    Self::collect_rest_indices(a, out);
                }
            }
            // Leaves: Var, literals, Quote, Error
            HirKind::Nil
            | HirKind::EmptyList
            | HirKind::Bool(_)
            | HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::String(_)
            | HirKind::Keyword(_)
            | HirKind::Var(_)
            | HirKind::Quote(_)
            | HirKind::Error => {}
        }
    }

    /// Precompute `callee_return_params` for all function definitions.
    ///
    /// For each function, compute a bitmask of parameter indices that may
    /// flow to the return position. Fixpoint iteration: seed all functions
    /// with empty bitmask (optimistic — no params returned), then widen
    /// until stable.
    fn precompute_return_params(&mut self, hir: &Hir) {
        let mut defs: Vec<(Binding, Vec<Binding>, &Hir)> = Vec::new();
        Self::collect_lambda_defs_with_params(hir, &mut defs);
        if defs.is_empty() {
            return;
        }

        // Seed: no params flow to return.
        for &(binding, _, _) in &defs {
            self.callee_return_params.insert(binding, 0);
        }

        // Iterate until stable.
        loop {
            let mut changed = false;
            for &(binding, ref params, body) in &defs {
                let mask = self.compute_return_params(body, params);
                let old = self.callee_return_params[&binding];
                if mask != old {
                    self.callee_return_params.insert(binding, mask | old);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// Compute return-params bitmask for a HIR expression.
    ///
    /// Returns a u64 bitmask where bit i is set if parameter i (from
    /// `params`) may flow to the return position of this expression.
    fn compute_return_params(&self, hir: &Hir, params: &[Binding]) -> u64 {
        match &hir.kind {
            // A variable reference: if it's one of our params, set its bit
            HirKind::Var(binding) => {
                if let Some(idx) = params.iter().position(|p| p == binding) {
                    if idx < 64 {
                        1u64 << idx
                    } else {
                        0
                    }
                } else {
                    0
                }
            }

            // Literals never return a parameter
            HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Bool(_)
            | HirKind::Nil
            | HirKind::Keyword(_)
            | HirKind::EmptyList
            | HirKind::String(_)
            | HirKind::Lambda { .. }
            | HirKind::Quote(_) => 0,

            // Control flow: union of all result positions
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.compute_return_params(then_branch, params)
                    | self.compute_return_params(else_branch, params)
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                let mut mask = 0u64;
                for (_, body) in clauses {
                    mask |= self.compute_return_params(body, params);
                }
                if let Some(b) = else_branch {
                    mask |= self.compute_return_params(b, params);
                }
                mask
            }
            HirKind::Begin(exprs) => exprs
                .last()
                .map(|e| self.compute_return_params(e, params))
                .unwrap_or(0),
            HirKind::And(exprs) | HirKind::Or(exprs) => {
                let mut mask = 0u64;
                for e in exprs {
                    mask |= self.compute_return_params(e, params);
                }
                mask
            }
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => {
                self.compute_return_params(body, params)
            }
            HirKind::Block { body, .. } => body
                .last()
                .map(|e| self.compute_return_params(e, params))
                .unwrap_or(0),
            HirKind::Match { arms, .. } => {
                let mut mask = 0u64;
                for (_, _, body) in arms {
                    mask |= self.compute_return_params(body, params);
                }
                mask
            }
            HirKind::While { .. } | HirKind::Loop { .. } => 0, // returns nil
            HirKind::Recur { .. } => 0,                        // jumps, never returns a value

            // Call: map callee's return_params through our args.
            //
            // This applies to both tail and non-tail calls: a non-tail
            // call in return position of this function (e.g. `(defn f [y]
            // (array :first y))` where `(array ...)` is the whole body)
            // still forwards its args into the result. The flag only
            // affects the trampoline, not the data-flow.
            //
            // Default for unknown callees is `!0` (assume every arg can
            // flow into the return), matching `can_scope_allocate_call`.
            // User-defined functions are seeded in `callee_return_params`
            // and refined by fixpoint iteration. Primitives aren't in
            // the map — `unwrap_or(!0)` treats them conservatively: a
            // container-like primitive (`array`, `struct`, `cons`) DOES
            // embed its args in the result; an intrinsic like `+` does
            // not. Both cases are safe under `!0`: the caller's scope
            // analysis may reject some scope-allocations that would
            // actually be fine, but it won't drop a value the callee
            // handed back inside its return.
            HirKind::Call { func, args, .. } => {
                let callee_rp = if let HirKind::Var(b) = &func.kind {
                    self.callee_return_params.get(b).copied().unwrap_or(!0)
                } else {
                    // Non-Var callee (e.g. `((get sched :pump))`):
                    // we can't identify it, so assume every arg flows
                    // to the return.
                    !0
                };
                let mut mask = 0u64;
                for (j, arg) in args.iter().enumerate() {
                    if j < 64 && (callee_rp & (1u64 << j)) != 0 {
                        if let HirKind::Var(b) = &arg.expr.kind {
                            if let Some(k) = params.iter().position(|p| p == b) {
                                if k < 64 {
                                    mask |= 1u64 << k;
                                }
                            }
                        }
                        // Non-Var arg (like (search ...)) doesn't map
                        // to any of our params — no bits set.
                    }
                }
                mask
            }

            // Parameterize: result is body's result
            HirKind::Parameterize { body, .. } => self.compute_return_params(body, params),

            // Cell ops, Assign, Define, Eval, Break, Emit, Destructure
            // — not return positions or covered by other analysis.
            HirKind::MakeCell { .. }
            | HirKind::DerefCell { .. }
            | HirKind::SetCell { .. }
            | HirKind::Assign { .. }
            | HirKind::Define { .. }
            | HirKind::Emit { .. }
            | HirKind::Destructure { .. }
            | HirKind::Eval { .. }
            | HirKind::Break { .. }
            | HirKind::Error => 0,

            // Intrinsics: result is not one of our params
            HirKind::Intrinsic { .. } => 0,
        }
    }

    /// Collect top-level function definitions with their parameter bindings.
    fn collect_lambda_defs_with_params<'b>(
        hir: &'b Hir,
        out: &mut Vec<(Binding, Vec<Binding>, &'b Hir)>,
    ) {
        match &hir.kind {
            HirKind::Define { binding, value } => {
                if let HirKind::Lambda { params, body, .. } = &value.kind {
                    out.push((*binding, params.clone(), body));
                }
                Self::collect_lambda_defs_with_params(value, out);
            }
            HirKind::Begin(exprs) => {
                for e in exprs {
                    Self::collect_lambda_defs_with_params(e, out);
                }
            }
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                for (binding, init) in bindings {
                    if let HirKind::Lambda {
                        params,
                        body: lbody,
                        ..
                    } = &init.kind
                    {
                        out.push((*binding, params.clone(), lbody));
                    }
                    Self::collect_lambda_defs_with_params(init, out);
                }
                Self::collect_lambda_defs_with_params(body, out);
            }
            // Recurse into lambda bodies to find nested defs (e.g. closures
            // inside function bodies). Don't push the Lambda itself — that's
            // handled by Define/Let when the Lambda is bound to a name.
            HirKind::Lambda { body, .. } => {
                Self::collect_lambda_defs_with_params(body, out);
            }
            // Recurse into all structural nodes to find nested lambda defs
            HirKind::While { cond, body } => {
                Self::collect_lambda_defs_with_params(cond, out);
                Self::collect_lambda_defs_with_params(body, out);
            }
            HirKind::Loop { bindings, body } => {
                for (binding, init) in bindings {
                    if let HirKind::Lambda {
                        params,
                        body: lbody,
                        ..
                    } = &init.kind
                    {
                        out.push((*binding, params.clone(), lbody));
                    }
                    Self::collect_lambda_defs_with_params(init, out);
                }
                Self::collect_lambda_defs_with_params(body, out);
            }
            HirKind::Recur { args } => {
                for a in args {
                    Self::collect_lambda_defs_with_params(a, out);
                }
            }
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                Self::collect_lambda_defs_with_params(cond, out);
                Self::collect_lambda_defs_with_params(then_branch, out);
                Self::collect_lambda_defs_with_params(else_branch, out);
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                for (c, b) in clauses {
                    Self::collect_lambda_defs_with_params(c, out);
                    Self::collect_lambda_defs_with_params(b, out);
                }
                if let Some(e) = else_branch {
                    Self::collect_lambda_defs_with_params(e, out);
                }
            }
            HirKind::Block { body, .. } => {
                for e in body {
                    Self::collect_lambda_defs_with_params(e, out);
                }
            }
            HirKind::Break { value, .. } => Self::collect_lambda_defs_with_params(value, out),
            HirKind::Match { value, arms } => {
                Self::collect_lambda_defs_with_params(value, out);
                for (_, guard, body) in arms {
                    if let Some(g) = guard {
                        Self::collect_lambda_defs_with_params(g, out);
                    }
                    Self::collect_lambda_defs_with_params(body, out);
                }
            }
            HirKind::Call { func, args, .. } => {
                Self::collect_lambda_defs_with_params(func, out);
                for a in args {
                    Self::collect_lambda_defs_with_params(&a.expr, out);
                }
            }
            HirKind::Assign { value, .. } => Self::collect_lambda_defs_with_params(value, out),
            HirKind::And(exprs) | HirKind::Or(exprs) => {
                for e in exprs {
                    Self::collect_lambda_defs_with_params(e, out);
                }
            }
            HirKind::Emit { value, .. } => Self::collect_lambda_defs_with_params(value, out),
            HirKind::Destructure { value, .. } => Self::collect_lambda_defs_with_params(value, out),
            HirKind::Eval { expr, env } => {
                Self::collect_lambda_defs_with_params(expr, out);
                Self::collect_lambda_defs_with_params(env, out);
            }
            HirKind::Parameterize { bindings, body } => {
                for (k, v) in bindings {
                    Self::collect_lambda_defs_with_params(k, out);
                    Self::collect_lambda_defs_with_params(v, out);
                }
                Self::collect_lambda_defs_with_params(body, out);
            }
            HirKind::MakeCell { value } => Self::collect_lambda_defs_with_params(value, out),
            HirKind::DerefCell { cell } => Self::collect_lambda_defs_with_params(cell, out),
            HirKind::SetCell { cell, value } => {
                Self::collect_lambda_defs_with_params(cell, out);
                Self::collect_lambda_defs_with_params(value, out);
            }
            HirKind::Intrinsic { args, .. } => {
                for a in args {
                    Self::collect_lambda_defs_with_params(a, out);
                }
            }
            // Leaves: Var, literals, Quote, Error
            HirKind::Nil
            | HirKind::EmptyList
            | HirKind::Bool(_)
            | HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::String(_)
            | HirKind::Keyword(_)
            | HirKind::Var(_)
            | HirKind::Quote(_)
            | HirKind::Error => {}
        }
    }

    /// Check if a function body always returns an immediate value.
    ///
    /// Unlike `result_is_safe` (which checks if a value is safe to
    /// return from a scope-allocated let), this checks the actual
    /// return type. For tail calls, it checks whether the CALLEE
    /// returns an immediate (via `call_result_is_safe`), not just
    /// whether the args avoid scope bindings.
    fn body_result_is_immediate(&self, hir: &Hir) -> bool {
        match &hir.kind {
            // Literals: all immediates
            HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Bool(_)
            | HirKind::Nil
            | HirKind::Keyword(_)
            | HirKind::EmptyList => true,

            // Var: a parameter or captured variable — could be anything.
            // We cannot know the caller's argument types, so conservatively
            // return false. Only constant-like values (literals) are safe.
            HirKind::Var(_) => false,

            // Control flow: recurse into all result positions
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.body_result_is_immediate(then_branch)
                    && self.body_result_is_immediate(else_branch)
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses
                    .iter()
                    .all(|(_, body)| self.body_result_is_immediate(body))
                    && else_branch
                        .as_ref()
                        .is_none_or(|b| self.body_result_is_immediate(b))
            }
            HirKind::Begin(exprs) => exprs
                .last()
                .is_some_and(|e| self.body_result_is_immediate(e)),
            HirKind::And(exprs) | HirKind::Or(exprs) => {
                exprs.iter().all(|e| self.body_result_is_immediate(e))
            }
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => {
                self.body_result_is_immediate(body)
            }
            HirKind::Block { body, .. } => body
                .last()
                .is_some_and(|e| self.body_result_is_immediate(e)),
            HirKind::Match { arms, .. } => arms
                .iter()
                .all(|(_, _, body)| self.body_result_is_immediate(body)),
            HirKind::While { .. } | HirKind::Recur { .. } => true, // returns nil
            HirKind::Loop { body, .. } => self.body_result_is_immediate(body),

            // ALL calls (tail or not): check if callee returns immediate
            HirKind::Call { func, args, .. } => self.call_result_is_safe(func, args),

            // Cell ops: MakeCell creates a heap cell, DerefCell/SetCell
            // may return heap values — conservatively not immediate.
            HirKind::MakeCell { .. } | HirKind::DerefCell { .. } | HirKind::SetCell { .. } => false,

            // Heap-allocating or unknown: Lambda, String, Quote, etc.
            HirKind::Lambda { .. }
            | HirKind::String(_)
            | HirKind::Quote(_)
            | HirKind::Assign { .. }
            | HirKind::Define { .. }
            | HirKind::Destructure { .. }
            | HirKind::Emit { .. }
            | HirKind::Eval { .. }
            | HirKind::Break { .. }
            | HirKind::Parameterize { .. }
            | HirKind::Intrinsic { .. }
            | HirKind::Error => false,
        }
    }

    /// Precompute `callee_rotation_safe` for all function definitions.
    ///
    /// Walks the HIR to find all `Define` bindings with lambda values,
    /// then iterates `body_escapes_heap_values` until the map stabilizes.
    /// This handles mutual recursion: initially all functions are assumed
    /// safe, and each pass may flip some to unsafe. Converges because
    /// the only transition is safe→unsafe (monotone).
    fn precompute_rotation_safety(&mut self, hir: &Hir) {
        // Collect all (binding, params, lambda_body) pairs from the HIR.
        let mut defs: Vec<(Binding, Vec<Binding>, &Hir)> = Vec::new();
        Self::collect_lambda_defs_with_params(hir, &mut defs);
        if defs.is_empty() {
            return;
        }

        // Seed: all functions optimistically safe.
        for &(binding, _, _) in &defs {
            self.callee_rotation_safe.insert(binding, true);
        }

        // Iterate until stable.
        loop {
            let mut changed = false;
            for &(binding, ref params, body) in &defs {
                // Set context so body_escapes_heap_values can detect
                // self-tail-calls and apply per-parameter analysis.
                self.current_function_binding = Some(binding);
                self.current_function_params = Some(params.clone());
                let escapes = self.body_escapes_heap_values(body);
                let was_safe = self.callee_rotation_safe[&binding];
                if was_safe && escapes {
                    self.callee_rotation_safe.insert(binding, false);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        // Clear context
        self.current_function_binding = None;
        self.current_function_params = None;

        // Record stats
        self.scope_stats.rotation_analyzed = defs.len();
        self.scope_stats.rotation_safe = self.callee_rotation_safe.values().filter(|&&v| v).count();
    }

    /// Collect all `(binding, lambda_body)` pairs from Define nodes.
    fn collect_lambda_defs<'b>(hir: &'b Hir, out: &mut Vec<(Binding, &'b Hir)>) {
        match &hir.kind {
            HirKind::Define { binding, value } => {
                if let HirKind::Lambda { body, .. } = &value.kind {
                    out.push((*binding, body));
                }
                Self::collect_lambda_defs(value, out);
            }
            HirKind::Begin(exprs) => {
                for e in exprs {
                    Self::collect_lambda_defs(e, out);
                }
            }
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                for (binding, init) in bindings {
                    if let HirKind::Lambda { body: lbody, .. } = &init.kind {
                        out.push((*binding, lbody));
                    }
                    Self::collect_lambda_defs(init, out);
                }
                Self::collect_lambda_defs(body, out);
            }
            HirKind::Lambda { body, .. } => {
                Self::collect_lambda_defs(body, out);
            }
            // Recurse into all structural nodes to find nested lambda defs
            HirKind::While { cond, body } => {
                Self::collect_lambda_defs(cond, out);
                Self::collect_lambda_defs(body, out);
            }
            HirKind::Loop { bindings, body } => {
                for (binding, init) in bindings {
                    if let HirKind::Lambda { body: lbody, .. } = &init.kind {
                        out.push((*binding, lbody));
                    }
                    Self::collect_lambda_defs(init, out);
                }
                Self::collect_lambda_defs(body, out);
            }
            HirKind::Recur { args } => {
                for a in args {
                    Self::collect_lambda_defs(a, out);
                }
            }
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                Self::collect_lambda_defs(cond, out);
                Self::collect_lambda_defs(then_branch, out);
                Self::collect_lambda_defs(else_branch, out);
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                for (c, b) in clauses {
                    Self::collect_lambda_defs(c, out);
                    Self::collect_lambda_defs(b, out);
                }
                if let Some(e) = else_branch {
                    Self::collect_lambda_defs(e, out);
                }
            }
            HirKind::Block { body, .. } => {
                for e in body {
                    Self::collect_lambda_defs(e, out);
                }
            }
            HirKind::Break { value, .. } => Self::collect_lambda_defs(value, out),
            HirKind::Match { value, arms } => {
                Self::collect_lambda_defs(value, out);
                for (_, guard, body) in arms {
                    if let Some(g) = guard {
                        Self::collect_lambda_defs(g, out);
                    }
                    Self::collect_lambda_defs(body, out);
                }
            }
            HirKind::Call { func, args, .. } => {
                Self::collect_lambda_defs(func, out);
                for a in args {
                    Self::collect_lambda_defs(&a.expr, out);
                }
            }
            HirKind::Assign { value, .. } => Self::collect_lambda_defs(value, out),
            HirKind::And(exprs) | HirKind::Or(exprs) => {
                for e in exprs {
                    Self::collect_lambda_defs(e, out);
                }
            }
            HirKind::Emit { value, .. } => Self::collect_lambda_defs(value, out),
            HirKind::Destructure { value, .. } => Self::collect_lambda_defs(value, out),
            HirKind::Eval { expr, env } => {
                Self::collect_lambda_defs(expr, out);
                Self::collect_lambda_defs(env, out);
            }
            HirKind::Parameterize { bindings, body } => {
                for (k, v) in bindings {
                    Self::collect_lambda_defs(k, out);
                    Self::collect_lambda_defs(v, out);
                }
                Self::collect_lambda_defs(body, out);
            }
            HirKind::MakeCell { value } => Self::collect_lambda_defs(value, out),
            HirKind::DerefCell { cell } => Self::collect_lambda_defs(cell, out),
            HirKind::SetCell { cell, value } => {
                Self::collect_lambda_defs(cell, out);
                Self::collect_lambda_defs(value, out);
            }
            HirKind::Intrinsic { args, .. } => {
                for a in args {
                    Self::collect_lambda_defs(a, out);
                }
            }
            // Leaves: Var, literals, Quote, Error
            HirKind::Nil
            | HirKind::EmptyList
            | HirKind::Bool(_)
            | HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::String(_)
            | HirKind::Keyword(_)
            | HirKind::Var(_)
            | HirKind::Quote(_)
            | HirKind::Error => {}
        }
    }

    /// Check if a HIR body is a tail call (or control flow where all result
    /// positions are tail calls). Used to relax the suspension check: a
    /// tail call replaces the frame, so its signal doesn't affect the
    /// enclosing scope's lifetime.
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
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => {
                Self::body_is_tail_call(body)
            }
            HirKind::Match { arms, .. } => arms
                .iter()
                .all(|(_, _, body)| Self::body_is_tail_call(body)),
            _ => false,
        }
    }

    /// Check if the body's tail call callee is a scope-bound binding.
    /// If so, RegionExit before the tail call would free the callee's slot.
    fn tail_call_callee_is_scope_bound(hir: &Hir, scope_bindings: &[(Binding, &Hir)]) -> bool {
        match &hir.kind {
            HirKind::Call {
                is_tail: true,
                func,
                ..
            } => {
                if let HirKind::Var(b) = &func.kind {
                    scope_bindings.iter().any(|(sb, _)| sb == b)
                } else if let HirKind::DerefCell { cell } = &func.kind {
                    if let HirKind::Var(b) = &cell.kind {
                        scope_bindings.iter().any(|(sb, _)| sb == b)
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                Self::tail_call_callee_is_scope_bound(then_branch, scope_bindings)
                    || Self::tail_call_callee_is_scope_bound(else_branch, scope_bindings)
            }
            HirKind::Begin(exprs) => exprs
                .last()
                .is_some_and(|e| Self::tail_call_callee_is_scope_bound(e, scope_bindings)),
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => {
                Self::tail_call_callee_is_scope_bound(body, scope_bindings)
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses
                    .iter()
                    .any(|(_, body)| Self::tail_call_callee_is_scope_bound(body, scope_bindings))
                    || else_branch
                        .as_ref()
                        .is_some_and(|b| Self::tail_call_callee_is_scope_bound(b, scope_bindings))
            }
            HirKind::Match { arms, .. } => arms
                .iter()
                .any(|(_, _, body)| Self::tail_call_callee_is_scope_bound(body, scope_bindings)),
            _ => false,
        }
    }

    /// Check if non-tail sub-expressions within a tail-call body may suspend.
    ///
    /// When `body_is_tail_call` returns true, the tail call's own signal
    /// is irrelevant (RegionExit fires before it). But preceding expressions
    /// in the body (e.g. side effects before the tail call in a `begin`)
    /// still execute within the scope and their suspension is dangerous.
    ///
    /// Returns true if any non-tail sub-expression may suspend.
    #[allow(dead_code)]
    fn non_tail_subexprs_may_suspend(hir: &Hir) -> bool {
        match &hir.kind {
            // A bare tail call has no preceding expressions.
            HirKind::Call { is_tail: true, .. } => false,
            // If/Cond: the condition runs before branches.
            HirKind::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                cond.signal.may_suspend()
                    || Self::non_tail_subexprs_may_suspend(then_branch)
                    || Self::non_tail_subexprs_may_suspend(else_branch)
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses
                    .iter()
                    .any(|(c, b)| c.signal.may_suspend() || Self::non_tail_subexprs_may_suspend(b))
                    || else_branch
                        .as_ref()
                        .is_some_and(|b| Self::non_tail_subexprs_may_suspend(b))
            }
            // Begin: all expressions except the last are non-tail.
            HirKind::Begin(exprs) => {
                let non_tail = &exprs[..exprs.len().saturating_sub(1)];
                non_tail.iter().any(|e| e.signal.may_suspend())
                    || exprs
                        .last()
                        .is_some_and(Self::non_tail_subexprs_may_suspend)
            }
            // Let/Letrec: init expressions are non-tail; recurse into body.
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                bindings.iter().any(|(_, init)| init.signal.may_suspend())
                    || Self::non_tail_subexprs_may_suspend(body)
            }
            // Match: the scrutinee is non-tail; recurse into arm bodies.
            HirKind::Match { value, arms } => {
                value.signal.may_suspend()
                    || arms
                        .iter()
                        .any(|(_, _, body)| Self::non_tail_subexprs_may_suspend(body))
            }
            // Anything else that body_is_tail_call returned true for:
            // conservatively say it may suspend.
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::Span;

    fn make_span() -> Span {
        Span::new(0, 0, 1, 1)
    }

    #[test]
    fn test_lower_int() {
        let arena = crate::hir::BindingArena::new();
        let mut lowerer = Lowerer::new(&arena);
        let hir = Hir::silent(HirKind::Int(42), make_span());
        let func = lowerer.lower(&hir).unwrap();
        assert!(!func.entry.blocks.is_empty());
    }

    #[test]
    fn test_lower_if() {
        let arena = crate::hir::BindingArena::new();
        let mut lowerer = Lowerer::new(&arena);
        let hir = Hir::silent(
            HirKind::If {
                cond: Box::new(Hir::silent(HirKind::Bool(true), make_span())),
                then_branch: Box::new(Hir::silent(HirKind::Int(1), make_span())),
                else_branch: Box::new(Hir::silent(HirKind::Int(2), make_span())),
            },
            make_span(),
        );
        let func = lowerer.lower(&hir).unwrap();
        // If now creates multiple blocks: entry, then, else, merge
        assert_eq!(func.entry.blocks.len(), 4);
        // Entry block should have a Branch terminator
        assert!(matches!(
            func.entry.blocks[0].terminator.terminator,
            Terminator::Branch { .. }
        ));
    }

    #[test]
    fn test_lower_begin() {
        let arena = crate::hir::BindingArena::new();
        let mut lowerer = Lowerer::new(&arena);
        let hir = Hir::silent(
            HirKind::Begin(vec![
                Hir::silent(HirKind::Int(1), make_span()),
                Hir::silent(HirKind::Int(2), make_span()),
            ]),
            make_span(),
        );
        let func = lowerer.lower(&hir).unwrap();
        assert!(!func.entry.blocks.is_empty());
    }
}
