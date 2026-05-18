//! HIR to LIR lowering

mod access;
mod binding;
mod control;
pub(crate) mod decision;
mod expr;
mod lambda;
mod pattern;

use super::intrinsics::IntrinsicOp;
use super::types::*;
use crate::hir::arena::BindingArena;
use crate::hir::region::RegionInfo;
use crate::hir::{Binding, BlockId, CallArg, Hir, HirId, HirKind, HirPattern};
use crate::syntax::Span;
use crate::value::{Arity, SymbolId, Value};
use rustc_hash::FxHashMap;
use std::collections::HashMap;

/// Tracks an active Loop during lowering so `Recur` can find its
/// entry label and binding slots.
struct LoopLowerContext {
    loop_label: Label,
    binding_slots: Vec<u16>,
    scope_eligible: bool,
    /// Region id for FreeRegion at recur back-edge. 0 if not scoped.
    region_id: u16,
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
    /// Pending FreeRegion region_ids to emit before tail calls.
    pending_free_regions: Vec<u16>,
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
    region_to_table: HashMap<crate::hir::region::Region, u16>,
    /// Stack of active region ids for FreeRegion emission on break.
    /// Pushed when a scope enters (FreeRegion-style), popped at scope exit.
    active_region_ids: Vec<u16>,
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
        }
    }

    /// Set intrinsic operations for operator specialization
    pub(crate) fn with_intrinsics(mut self, intrinsics: FxHashMap<SymbolId, IntrinsicOp>) -> Self {
        self.intrinsics = intrinsics;
        self
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

        let result_reg = self.lower_expr(hir)?;
        self.terminate(Terminator::Return(result_reg));
        self.finish_block();

        self.current_func.entry = Label(0);
        self.current_func.num_regs = self.next_reg;
        // Propagate signal from HIR to top-level LIR function
        self.current_func.signal = hir.signal;

        let entry = std::mem::replace(&mut self.current_func, LirFunction::new(Arity::Exact(0)));
        let closures = std::mem::take(&mut self.closures);

        Ok(LirModule {
            entry,
            closures,
        })
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
    fn emit_in_region(&mut self, instr: LirInstr, region: u16) {
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
    fn emit_alloc(&mut self, instr: LirInstr) {
        let rid = self.alloc_region_id();
        if rid == 0 {
            self.emit(instr);
        } else {
            self.emit_in_region(instr, rid);
        }
    }

    /// Look up the region for the current HIR node and return the u16
    /// index into the function's region_table. Returns 0 when there is
    /// no region assignment or the region is GLOBAL.
    fn alloc_region_id(&mut self) -> u16 {
        let hir_id = match self.current_hir_id {
            Some(id) => id,
            None => return 0,
        };
        let region = match self.region_info.alloc_region.get(&hir_id) {
            Some(r) => *r,
            None => return 0,
        };
        if region.is_global() {
            return 0;
        }
        if let Some(&table_id) = self.region_to_table.get(&region) {
            table_id
        } else {
            let table_id = self.current_func.region_table.len() as u16 + 1;
            self.current_func.region_table.push(table_id);
            self.region_to_table.insert(region, table_id);
            table_id
        }
    }

    /// Look up the u16 region table id for a scope's region.
    /// Returns 0 if the scope has no region or it's global.
    fn scope_region_id(&mut self, hir_id: HirId) -> u16 {
        let region = match self.region_info.scope_region.get(&hir_id) {
            Some(r) => *r,
            None => return 0,
        };
        if region.is_global() {
            return 0;
        }
        if let Some(&table_id) = self.region_to_table.get(&region) {
            table_id
        } else {
            let table_id = self.current_func.region_table.len() as u16 + 1;
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


    /// Emit pending FreeRegions. Called at tail-call sites where
    /// region cleanup is deferred.
    fn emit_pending_free_regions(&mut self) {
        let pending: Vec<u16> = self.pending_free_regions.clone();
        for region_id in pending {
            self.emit(LirInstr::FreeRegion { region_id });
        }
    }

    /// Check if the callee is the current function (self-tail-call).
    fn is_self_tail_call(&self, func: &Hir) -> bool {
        let binding = match &func.kind {
            HirKind::Var(b) => b,
            HirKind::DerefCell { cell } => match &cell.kind {
                HirKind::Var(b) => b,
                _ => return false,
            },
            _ => return false,
        };
        self.current_function_binding
            .as_ref()
            .is_some_and(|b| b == binding)
    }

    /// Emit DropSlot instructions for dead locals at a tail-call site.
    ///
    /// Collects all Var bindings referenced by the tail-call arguments,
    /// then emits DropSlot for each local binding (parameters AND let
    /// bindings) that:
    /// 1. Is NOT referenced by any tail-call argument
    /// 2. Has a local slot (not an upvalue/capture)
    /// 3. Is NOT captured by any closure
    fn emit_drop_slots_for_tail_call(&mut self, func: &Hir, args: &[CallArg]) {
        // Inside an active FreeRegion scope, DropSlot could cause
        // double-free when FreeRegion later frees the same slot.
        if !self.active_region_ids.is_empty() {
            return;
        }

        // Collect all Var bindings referenced by args AND the function.
        // The function closure is already in a register, but its slab
        // slot must not be freed (the register is a raw pointer copy).
        let mut referenced = std::collections::HashSet::new();
        Self::collect_var_refs(func, &mut referenced);
        for arg in args {
            Self::collect_var_refs(&arg.expr, &mut referenced);
        }

        // Emit DropSlot for unreferenced local-slot bindings.
        // ONLY for parameters — let bindings may hold values obtained
        // from collections (via get, first, etc.) that are aliased and
        // must not be freed. Parameters are safe because they receive
        // their values from the caller (fresh copies on the stack).
        let mut slots: Vec<_> = self
            .binding_to_slot
            .iter()
            .filter(|(binding, _)| {
                !referenced.contains(binding)
                    && !self.upvalue_bindings.contains(binding)
                    && !self.arena.get(**binding).is_captured
                    && self.arena.get(**binding).scope == crate::hir::arena::BindingScope::Parameter
            })
            .map(|(_, &slot)| slot)
            .collect();

        // Emit in reverse slot order (higher slots first).
        slots.sort_unstable();
        for slot in slots.into_iter().rev() {
            self.emit(LirInstr::DropSlot { slot });
        }
    }

    /// Collect all Var bindings referenced in a HIR expression.
    fn collect_var_refs(hir: &Hir, out: &mut std::collections::HashSet<Binding>) {
        match &hir.kind {
            HirKind::Var(b) => {
                out.insert(*b);
            }
            HirKind::DerefCell { cell } => {
                Self::collect_var_refs(cell, out);
            }
            _ => {
                hir.for_each_child(|child| {
                    Self::collect_var_refs(child, out);
                });
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

    /// Discard an unused value and immediately free it via DropSlot.
    /// Used for intermediate expressions in begin blocks where the
    /// discarded value is provably a fresh allocation (not aliased).
    fn discard_and_drop(&mut self, src: Reg) {
        self.discard(src);
        if let Some(slot) = self.discard_slot {
            self.emit(LirInstr::DropSlot { slot });
        }
    }

    /// Check if an expression provably produces a fresh heap allocation.
    ///
    /// Returns true only when the result is guaranteed to be a new object
    /// that no other reference points to. This is critical for DropSlot
    /// safety: mutating primitives like `put`/`push` return their first
    /// argument (an existing object), so freeing that would corrupt live
    /// references through upvalues or other bindings.
    fn expr_is_fresh_allocation(&self, hir: &Hir) -> bool {
        match &hir.kind {
            HirKind::Lambda { .. } => true,
            HirKind::Call { func, .. } => self.callee_is_fresh_allocator(func),
            _ => false,
        }
    }

    /// Check if a callee is a primitive known to always return a fresh
    /// heap allocation (never an alias of an argument).
    fn callee_is_fresh_allocator(&self, func: &Hir) -> bool {
        static FRESH_PRIMS: &[&str] = &[
            "struct",
            "struct-mut",
            "string",
            "array",
            "bytes",
            "set",
            "set-mut",
        ];

        let binding = match &func.kind {
            HirKind::Var(b) => b,
            HirKind::DerefCell { cell } => match &cell.kind {
                HirKind::Var(b) => b,
                _ => return false,
            },
            _ => return false,
        };

        // Check immutable_values directly (works for top-level primitives).
        if let Some(val) = self.immutable_values.get(binding) {
            if let Some(def) = val.as_native_def() {
                return FRESH_PRIMS.contains(&def.name);
            }
        }

        // For captured primitives (functionalize wraps them in cells),
        // check if the binding's value was seeded as a known primitive.
        // Walk the binding info: if it's is_primitive + is_immutable,
        // look up the original primitive value by checking all
        // immutable_values entries that share the same SymbolId.
        let bi = self.arena.get(*binding);
        if bi.is_immutable && !bi.is_mutated {
            let target_sym = bi.name;
            for (&other_binding, val) in &self.immutable_values {
                if self.arena.get(other_binding).name == target_sym {
                    if let Some(def) = val.as_native_def() {
                        return FRESH_PRIMS.contains(&def.name);
                    }
                }
            }
        }
        false
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
}
