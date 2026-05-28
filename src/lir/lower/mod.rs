//! HIR to LIR lowering

mod access;
mod binding;
mod control;
pub(crate) mod decision;
mod expr;
mod lambda;
mod pattern;

use std::sync::atomic::{AtomicU16, Ordering};

use super::intrinsics::IntrinsicOp;
use super::types::*;
use crate::hir::arena::BindingArena;
use crate::hir::region::{RegionId, RegionInfo};
use crate::hir::{Binding, BlockId, Hir, HirId, HirKind, HirPattern};

/// Global region ID counter. IDs 0 (invalid) and 1 (immortal) are reserved.
/// Used by the lowerer for solver-assigned regions and by the compilation
/// pipeline for transient compile-time regions.
static NEXT_REGION_ID: AtomicU16 = AtomicU16::new(2);

/// Mint a fresh globally-unique runtime region ID.
pub fn fresh_region_id() -> RegionId {
    let id = NEXT_REGION_ID.fetch_add(1, Ordering::Relaxed);
    assert!(id >= 2, "region ID counter wrapped or hit reserved range");
    id
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
    /// Region id whose `DecrefRegion` fires at the recur back-edge. None if not scoped.
    region_id: Option<RegionId>,
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
    /// Compile-time constant values for immutable bindings (for LoadConst optimization)
    immutable_values: HashMap<Binding, Value>,
    /// Stack of active loop contexts for `Recur` lowering
    loop_lower_contexts: Vec<LoopLowerContext>,
    /// Stack of active block contexts for `break` lowering
    block_lower_contexts: Vec<BlockLowerContext>,
    /// Current nesting depth of active allocation regions.
    /// Pending `DecrefRegion` region_ids to emit before tail calls.
    pending_free_regions: Vec<RegionId>,
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
    /// Current HIR node being lowered. Set at the top of `lower_expr`.
    /// Used by `alloc_region_id()` to look up the region for allocations.
    current_hir_id: Option<HirId>,
    /// Maps Region(u32) from region inference to u16 index in the
    /// function's region_table. Lazily populated by `alloc_region_id()`.
    region_to_table: HashMap<crate::hir::region::Region, RegionId>,
    /// Stack of active region ids for `DecrefRegion` emission on break.
    /// Pushed when a scope enters, popped at scope exit.
    active_region_ids: Vec<RegionId>,
    /// Stack of currently-active lambda HirIds. `lower_lambda_expr`
    /// pushes its HirId before lowering the body and pops on exit.
    /// `emit_decrefs_for` consults the top entry to look up the active
    /// lambda's tail-region set in `region_info.lambda_tail_regions`
    /// and suppress `DecrefRegion` for any region that flows out as
    /// the function's return value (impl step 14 — return as escape).
    current_lambda_stack: Vec<HirId>,
    /// For call result regions whose value lives in a known local
    /// slot (let-bound Call init), maps the region to that slot.
    /// `emit_decrefs_for` uses this to emit `LoadLocal slot` +
    /// `ReleaseValueRegion` at the call's `free_at`, dynamically
    /// decref'ing the runtime region of the actual returned value
    /// (impl step 14).
    call_region_slot: HashMap<crate::hir::region::Region, u16>,
    /// RegionIds the lowerer has stamped onto at least one
    /// instruction via `emit_in_region` (i.e., regions the runtime
    /// will actually have a slot for after `alloc_in_region`).
    /// Used by `emit_decref_region` to suppress phantom DecrefRegion
    /// emissions — the analysis may yield a `free_at` for a region
    /// whose alloc never landed in the bytecode (legitimately, for
    /// `call_result_regions` going through `ReleaseValueRegion`
    /// instead; less legitimately, when the regions walk assigned a
    /// region to a node the lowerer is transparent for). Emitting
    /// `DecrefRegion(r)` for an unstamped r would decrement an RC
    /// the runtime never raised.
    emitted_alloc_regions: rustc_hash::FxHashSet<RegionId>,
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
            immutable_values: HashMap::new(),
            loop_lower_contexts: Vec::new(),
            block_lower_contexts: Vec::new(),
            pending_free_regions: Vec::new(),
            discard_slot: None,
            symbol_names: HashMap::new(),
            closures: Vec::new(),
            current_function_binding: None,
            current_function_params: None,
            region_info: RegionInfo::empty(),
            current_hir_id: None,
            region_to_table: HashMap::new(),
            active_region_ids: Vec::new(),
            current_lambda_stack: Vec::new(),
            call_region_slot: HashMap::new(),
            emitted_alloc_regions: rustc_hash::FxHashSet::default(),
        }
    }

    /// Set all primitive property sets from a PrimitiveClassification.
    pub fn with_primitive_classification(
        mut self,
        pc: crate::lir::intrinsics::PrimitiveClassification,
    ) -> Self {
        self.intrinsics = pc.intrinsics;
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
        self.current_func = LirFunction::new(Arity::Exact(0));
        self.current_block = BasicBlock::new(Label(0));
        self.next_reg = 0;
        self.next_label = 1;
        self.binding_to_slot.clear();
        self.discard_slot = None;
        self.closures.clear();

        // Treat the top-level expression as an implicit function
        // body for tail-region suppression — the entry function
        // returns its result, so tail regions must transfer to the
        // caller (impl step 14).
        self.current_lambda_stack.push(hir.id);
        let result_reg = self.lower_expr(hir)?;
        self.current_lambda_stack.pop();
        self.terminate(Terminator::Return(result_reg));
        self.finish_block();

        self.current_func.entry = Label(0);
        self.current_func.num_regs = self.next_reg;
        // Propagate signal from HIR to top-level LIR function
        self.current_func.signal = hir.signal;

        let entry = std::mem::replace(&mut self.current_func, LirFunction::new(Arity::Exact(0)));
        let closures = std::mem::take(&mut self.closures);

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
        if !needs_capture || !self.in_lambda {
            // Initialize slot to NIL so LoadLocal finds a valid value.
            if let Ok(nil_reg) = self.emit_const(LirConst::Nil) {
                self.emit(LirInstr::StoreLocal { slot, src: nil_reg });
            }
        }
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

    /// Emit an instruction with a specific region id.
    /// Used for heap-allocating instructions that belong to a non-default region.
    fn emit_in_region(&mut self, instr: LirInstr, region: RegionId) {
        self.emitted_alloc_regions.insert(region);
        self.current_block
            .instructions
            .push(SpannedInstr::with_region(
                instr,
                self.current_span.clone(),
                region,
            ));
    }

    /// Emit a heap-allocating instruction, stamping it with the region
    /// assigned by region inference for the current HIR node.
    ///
    /// Panics if no region is assigned — every allocation must have a
    /// region from the solver.
    fn emit_alloc(&mut self, instr: LirInstr) {
        let rid = self.alloc_region_id().unwrap_or_else(|| {
            panic!(
                "emit_alloc: no region for hir_id {:?} — solver must assign a region to every allocation",
                self.current_hir_id
            )
        });
        if crate::config::get().has_trace("rc") {
            // Correlates runtime [trace:rc] alloc_in_region(R) events back
            // to a HirId and source span. Pair with grep on the region id
            // to find the full lifecycle (incref/decref/FREE/cascade), or
            // grep on the payload address from a deref-mismatch panic.
            eprintln!(
                "[trace:rc:emit] emit_alloc hir_id={:?} region={} span={}",
                self.current_hir_id, rid, self.current_span
            );
        }
        self.emit_in_region(instr, rid);
    }

    /// Look up the region for the current HIR node and return the u16
    /// index into the function's region_table.
    fn alloc_region_id(&mut self) -> Option<RegionId> {
        let hir_id = self.current_hir_id?;
        let region = *self.region_info.alloc_region.get(&hir_id)?;
        let table_id = self.region_table_id(region);
        Some(table_id)
    }

    /// Look up the u16 region table id for a scope's region.
    /// Returns `None` if the scope has no region.
    fn scope_region_id(&mut self, hir_id: HirId) -> Option<RegionId> {
        let region = *self.region_info.scope_region.get(&hir_id)?;
        let table_id = self.region_table_id(region);
        Some(table_id)
    }

    /// Map a solver Region to a u16 bytecode region table id.
    ///
    /// IDs are globally unique (via atomic counter) so that different
    /// compilation units never collide at runtime — `FreeRegion(N)` from
    /// one unit must not free objects stamped by another unit.
    fn region_table_id(&mut self, region: crate::hir::region::Region) -> RegionId {
        if let Some(&table_id) = self.region_to_table.get(&region) {
            table_id
        } else {
            let table_id = fresh_region_id();
            self.current_func.region_table.push(table_id);
            self.region_to_table.insert(region, table_id);
            table_id
        }
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

    /// Emit a store for a named binding.
    fn emit_binding_store(&mut self, slot: u16, src: Reg) {
        self.emit(LirInstr::StoreLocal { slot, src });
    }

    /// After emitting a non-tail Call (or Call-like) instruction whose
    /// result lives in `dst`, allocate a release slot, stash the value
    /// into it, reload it into a fresh register, and record the slot
    /// against the call's region in `call_region_slot`.
    ///
    /// This makes `emit_decrefs_for` emit `LoadLocal slot +
    /// ReleaseValueRegion` uniformly at the call's `free_at` for both
    /// bound (`(let [x (foo)] ...)`) and unbound (`(use (foo))`)
    /// Calls. Without the slot the unbound case fell through to a
    /// no-op and the call's result region leaked until fiber teardown.
    ///
    /// The release slot is allocated even for tail-region Calls
    /// (whose Release is suppressed by `emit_decrefs_for` via
    /// `lambda_tail_regions`): allocating an extra stack-local
    /// indirection is cheap, and the simpler "always allocate" rule
    /// avoids branching on tail-region detection here.
    fn wrap_call_with_release_slot(&mut self, dst: Reg) -> Reg {
        let slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;
        self.emit(LirInstr::StoreLocal { slot, src: dst });
        let reload = self.fresh_reg();
        self.emit(LirInstr::LoadLocal { dst: reload, slot });
        if let Some(hir_id) = self.current_hir_id {
            if let Some(&call_r) = self.region_info.alloc_region.get(&hir_id) {
                self.call_region_slot.insert(call_r, slot);
            }
        }
        reload
    }

    /// Emit IncrefRegion for any cross-region references at this HIR node.
    fn emit_increfs_for(&mut self, hir_id: HirId) {
        let refs: Vec<_> = self
            .region_info
            .cross_region_refs
            .iter()
            .filter(|(site, _, _)| *site == hir_id)
            .map(|&(_, src, _)| src)
            .collect();
        for src in refs {
            let src_id = self.region_table_id(src);
            self.emit(LirInstr::IncrefRegion { region_id: src_id });
        }
    }

    /// Emit region-demise instructions at this node's `free_at`:
    ///
    /// - Tail regions of the currently-active lambda: skip (ownership
    ///   transferred to the caller via `Return` — impl step 14).
    /// - Call-result regions with a known binding slot: emit
    ///   `LoadLocal slot` + `ReleaseValueRegion` so the decref uses
    ///   the *runtime* region of the actual returned value, not the
    ///   compile-time `call_r` placeholder.
    /// - Call-result regions without a known slot: skip. The runtime
    ///   region is unknown to the lowerer at this point; the alloc
    ///   leaks until fiber teardown. This is conservative but sound.
    /// - All other regions: emit `DecrefRegion(rid)` — the
    ///   compile-time region ID matches the runtime region (alloc
    ///   opcodes used the compile-time ID as the bytecode operand).
    fn emit_decrefs_for(&mut self, hir_id: HirId) {
        let tail_regions: Vec<crate::hir::region::Region> = self
            .current_lambda_stack
            .last()
            .and_then(|lambda_id| self.region_info.lambda_tail_regions.get(lambda_id))
            .cloned()
            .unwrap_or_default();

        let regions: Vec<crate::hir::region::Region> = self
            .region_info
            .region_data
            .iter()
            .filter(|(r, d)| d.free_at == hir_id && !tail_regions.contains(r))
            .map(|(r, _)| *r)
            .collect();
        for r in regions {
            if self.region_info.call_result_regions.contains(&r) {
                if let Some(&slot) = self.call_region_slot.get(&r) {
                    // Load the value from its slot and release by
                    // runtime region. The slot still holds a dangling
                    // Value after this but is never read again
                    // (free_at is the last use). The expected region
                    // id gates the decref so passthrough calls (whose
                    // result lives in a region this call did not
                    // allocate) skip.
                    let expected = self.region_table_id(r);
                    let val_reg = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal { dst: val_reg, slot });
                    self.emit(LirInstr::ReleaseValueRegion {
                        src: val_reg,
                        expected_region_id: expected,
                    });
                    if crate::config::get().has_trace("rc") {
                        // The hir_id here is the `free_at` HirId — where
                        // the regions analysis placed the release. Pair
                        // with [trace:rc:emit] emit_alloc on the same
                        // region id to spot the alloc-then-release-at-
                        // same-HirId pattern (the bug class fixed by
                        // propagating parent_consumes through Or/And/
                        // If branches/Let body/Begin tail in
                        // src/hir/liveness.rs).
                        eprintln!(
                            "[trace:rc:emit] emit_release_value_region hir_id={:?} region={} span={}",
                            hir_id, expected, self.current_span
                        );
                    }
                }
                // Unbound Call result: skip (leak until fiber teardown).
                continue;
            }
            let rid = self.region_table_id(r);
            self.emit_decref_region(rid);
        }
    }

    /// Emit `DecrefRegion` for a region's compiler-owned reference
    /// (the initial RC=1 that the compiler dropped at the region's
    /// `free_at` HirId).
    ///
    /// Cross-region refs are decremented by cascade in `do_free` at
    /// runtime, not by additional compiler-emitted `DecrefRegion`
    /// instructions. Compiler-emitted `IncrefRegion` (from
    /// `emit_increfs_for`) handles the incref side; cascade handles
    /// the decref side.
    fn emit_decref_region(&mut self, region_id: RegionId) {
        // Suppress phantom DecrefRegion: if the lowerer never stamped
        // an instruction with this region id via `emit_in_region`,
        // then the runtime has no entry for it and the DecrefRegion
        // would target the wrong region (or fail an assertion in
        // debug builds). Phantom regions can arise from regions-walk
        // assignments to nodes whose lowering layer is transparent
        // (DerefCell, MakeCell) or from analysis gaps that the
        // ongoing audit hasn't yet closed. Begin/Letrec phantoms (the
        // if-phi-merge case) were fixed at the analysis layer in
        // src/hir/regions.rs by gating alloc_here on the lowerer's
        // emit predicate; other classes (Match without captured pattern
        // bindings, etc.) remain to be audited — the guard stays as
        // defense in depth until that audit is complete.
        if !self.emitted_alloc_regions.contains(&region_id) {
            return;
        }
        self.emit(LirInstr::DecrefRegion { region_id });
    }

    /// Emit pending `DecrefRegion` instructions. Called at tail-call
    /// sites where region cleanup is deferred. Deduplicates to avoid
    /// double-decrementing regions shared between nested scopes.
    fn emit_pending_free_regions(&mut self) {
        let pending: Vec<u16> = self.pending_free_regions.clone();
        let mut seen = std::collections::HashSet::new();
        for region_id in pending {
            if seen.insert(region_id) {
                self.emit_decref_region(region_id);
            }
        }
    }

    /// Discard an unused value by storing it to a scratch slot.
    /// StoreLocal does not incref, so no refcount tracking needed.
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

    // ── Region-lifecycle emission tests (drive impl steps 9 & 13) ────

    fn compile_to_lir(source: &str) -> crate::lir::LirModule {
        use crate::hir::functionalize::functionalize;
        use crate::hir::tailcall::mark_tail_calls;
        use crate::hir::Analyzer;
        use crate::primitives::register_primitives;
        use crate::reader::read_syntax;
        use crate::symbol::SymbolTable;
        use crate::syntax::Expander;
        use crate::vm::VM;

        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        let meta = register_primitives(&mut vm, &mut symbols);
        let wrapped = format!(
            "(letrec [cond_var (fn () nil) f (fn (& args) args) g (fn (& args) args)] {})",
            source
        );
        let syntax = read_syntax(&wrapped, "<test>").expect("parse");
        let mut expander = Expander::new();
        let expanded = expander
            .expand(syntax, &mut symbols, &mut vm)
            .expect("expand");
        let mut arena = crate::hir::BindingArena::new();
        let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
        analyzer.bind_primitives(&meta);
        let mut analysis = analyzer.analyze(&expanded).expect("analyze");
        let prim_values = analyzer.primitive_values().clone();
        drop(analyzer);
        mark_tail_calls(&mut analysis.hir);
        functionalize(&mut analysis.hir, &mut arena);
        crate::hir::anf::anf_lift(&mut analysis.hir, &mut arena);

        let pc = crate::lir::intrinsics::PrimitiveClassification::new(&symbols, &meta);
        let region_info = crate::hir::analyze_regions_with(
            &analysis.hir,
            &arena,
            pc.call_classification.clone(),
        );
        let mut lowerer = Lowerer::new(&arena)
            .with_primitive_classification(pc)
            .with_primitive_values(prim_values)
            .with_symbol_names(symbols.all_names())
            .with_region_info(region_info);
        lowerer.lower(&analysis.hir).expect("lower")
    }

    fn count_decref_regions(module: &crate::lir::LirModule) -> usize {
        fn count_in_func(func: &LirFunction) -> usize {
            func.blocks
                .iter()
                .flat_map(|b| b.instructions.iter())
                .filter(|i| matches!(i.instr, LirInstr::DecrefRegion { .. }))
                .count()
        }
        count_in_func(&module.entry)
            + module.closures.iter().map(count_in_func).sum::<usize>()
    }

    fn count_release_value_regions(module: &crate::lir::LirModule) -> usize {
        fn count_in_func(func: &LirFunction) -> usize {
            func.blocks
                .iter()
                .flat_map(|b| b.instructions.iter())
                .filter(|i| matches!(i.instr, LirInstr::ReleaseValueRegion { .. }))
                .count()
        }
        count_in_func(&module.entry)
            + module.closures.iter().map(count_in_func).sum::<usize>()
    }

    #[test]
    fn decref_region_emitted_for_one_alloc_let() {
        // Under unique-per-alloc the lowerer emits one `DecrefRegion`
        // per region at each region's `free_at` HirId. The walk also
        // registers regions for `Let`/`Letrec`/`Begin`/`Match`/`Call`
        // nodes (for capture-cell and per-call bookkeeping), so the
        // total count is more than just the one user-visible allocation.
        // Assert there's at least one DecrefRegion — i.e. the new
        // emission path is wired (we'd see zero if `emit_decrefs_for`
        // weren't called).
        let module = compile_to_lir("(fn () (let [x (string \"a\")] x))");
        assert!(
            count_decref_regions(&module) >= 1,
            "expected at least one DecrefRegion to be emitted by emit_decrefs_for",
        );
    }

    #[test]
    fn decref_region_emitted_for_emit_yield() {
        // `(fn () (let [x (string "a")] (emit :yield x)))` — the yielded
        // value's region is decref'd at the Emit's HirId (the value's
        // last use); the runtime incref in `handle_emit` (impl step 14)
        // keeps the region alive past the matching DecrefRegion at the
        // resume site.
        let module = compile_to_lir("(fn () (let [x (string \"a\")] (emit :yield x)))");
        assert!(
            count_decref_regions(&module) >= 1,
            "expected at least one DecrefRegion for the emit-yielded value",
        );
    }

    #[test]
    fn release_emitted_for_unbound_call_result() {
        // An unbound Call result — `(f "a")` whose result flows
        // directly into Begin's discard position — must have a
        // ReleaseValueRegion emitted at its free_at. Without this,
        // the call's result region survives until fiber teardown
        // (linear leak in loops). `lower_call` allocates a release
        // slot for every Call so emit_decrefs_for can emit
        // `LoadLocal slot` + `ReleaseValueRegion` uniformly for
        // both bound and unbound Calls.
        let module = compile_to_lir("(fn () (begin (f \"a\" \"b\") nil))");
        assert!(
            count_release_value_regions(&module) >= 1,
            "expected at least one ReleaseValueRegion for the unbound (f ...) result",
        );
    }

    #[test]
    fn release_emitted_for_let_bound_call_result() {
        // Sanity check: the existing let-bound Call result path
        // also produces a ReleaseValueRegion. This guards against
        // a regression where removing the redundant call_region_slot
        // recording in lower_let breaks the bound case.
        let module = compile_to_lir("(fn () (let [x (f \"a\" \"b\")] nil))");
        assert!(
            count_release_value_regions(&module) >= 1,
            "expected at least one ReleaseValueRegion for the let-bound (f ...) result",
        );
    }

    #[test]
    fn release_emitted_for_eval_result() {
        // `(fn () (begin (eval 1) nil))` — the Eval's result is
        // discarded. Eval's result region is a placeholder in the
        // outer compilation (the actual value lives in the inner
        // compilation's region). The regions walk registers Eval's
        // placeholder in `call_result_regions`, mirroring Call, and
        // `lower_eval` wraps the result with
        // `wrap_call_with_release_slot`. `emit_decrefs_for` then
        // emits `LoadLocal slot + ReleaseValueRegion(expected)` at
        // the Eval's free_at; the runtime gate skips the decref when
        // `region_of(value)` doesn't match the placeholder — safe by
        // construction.
        //
        // Without this wiring (pre-fix), the walk's `alloc_here` for
        // Eval's HirId would land in the else branch of
        // `emit_decrefs_for`, which emits raw `DecrefRegion(rid)` for
        // a region the runtime never allocated into — counter
        // underflow or conflation with a neighbouring region id.
        let module = compile_to_lir("(fn () (begin (eval 1) nil))");
        assert!(
            count_release_value_regions(&module) >= 1,
            "expected at least one ReleaseValueRegion for the (eval ...) result",
        );
    }

    #[test]
    #[ignore = "merging enabled at impl step 16"]
    fn decref_region_emitted_once_for_merged_pair() {
        // `(let [x (string "a") y (string "b")] (g x y))` has two
        // allocations with identical free_at and no cross-region
        // edges, so the merge pass collapses them into one region.
        // The lowerer emits exactly one `DecrefRegion` for the
        // merged group.
        let module =
            compile_to_lir("(let [x (string \"a\") y (string \"b\")] (g x y))");
        assert_eq!(
            count_decref_regions(&module),
            1,
            "merged x and y should share one DecrefRegion",
        );
    }
}
