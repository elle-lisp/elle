//! LIR to Bytecode emission
//!
//! Converts register-based LIR to stack-based bytecode.
//! Uses a simple stack simulation to track register values.

mod stack;

use super::types::*;
use crate::compiler::bytecode::{Bytecode, Instruction};
use crate::value::Value;
use std::collections::HashMap;
use std::rc::Rc;

/// Per-closure compilation result: bytecode, yield points, call sites.
type ClosureCompiled = (Bytecode, Vec<YieldPointInfo>, Vec<CallSiteInfo>);

/// This function's value-route release slots, ascending — the table an error exit
/// walks (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
/// still owes"). Ascending so the walk's release order is the slot order the body
/// allocated them in, identical across compiles: the releases commute (each is a
/// decref) but their cascades must not depend on a hash order.
fn sorted_release_slots(func: &LirFunction) -> Vec<u16> {
    let mut slots = func.frame_release_slots.clone();
    slots.sort_unstable();
    slots
}

/// The `DecrefRegion` half of the same table, ascending for the same reason.
fn sorted_release_regions(func: &LirFunction) -> Vec<u32> {
    let mut regions: Vec<u32> = func.frame_release_regions.iter().map(|r| r.get()).collect();
    regions.sort_unstable();
    regions
}

/// Emits bytecode from LIR
pub struct Emitter {
    /// Output bytecode
    bytecode: Bytecode,
    /// Map from Label to bytecode offset
    label_offsets: HashMap<Label, usize>,
    /// Pending jumps that need patching (instruction position, target label)
    pending_jumps: Vec<(usize, Label)>,
    /// Stack simulation: which register's value is at each stack position
    stack: Vec<Reg>,
    /// Register to stack position mapping (for finding values)
    reg_to_stack: HashMap<Reg, usize>,
    /// Symbol ID → name mapping for cross-thread portability
    symbol_names: HashMap<u32, String>,
    /// Saved stack state from yield terminators, keyed by resume label.
    /// When a block ends with Terminator::Yield, the stack state is saved here
    /// so the resume block can start with the correct simulation state.
    yield_stack_state: HashMap<Label, (Vec<Reg>, HashMap<Reg, usize>)>,
    /// Operand depth each already-emitted block started at, keyed by label.
    /// `yield_stack_state` answers the same question for a block still ahead of
    /// the cursor, but `emit_block` consumes that entry — so this is what a back
    /// edge into a loop header has left to trim against (`edge_depth`).
    block_entry_depth: HashMap<Label, usize>,
    /// Yield point metadata collected during emission.
    yield_points: Vec<YieldPointInfo>,
    /// Call site metadata collected during emission.
    call_sites: Vec<CallSiteInfo>,
    /// Whether the current function may suspend (gates call site recording).
    current_func_may_suspend: bool,
    /// Number of local variable slots in the current function.
    /// Recorded in yield points and call sites so the JIT can spill
    /// local values into the SuspendedFrame stack.
    current_func_num_locals: u16,
    /// Pre-compiled closure bytecodes for `emit_module`. Indexed by `ClosureId`.
    /// `None` when emitting a standalone function (tests, nested emit).
    compiled_closures: Option<Vec<ClosureCompiled>>,
    /// LirFunction metadata for each closure. Parallel to `compiled_closures`.
    /// Needed by MakeClosure to build ClosureTemplates.
    closure_lir_funcs: Option<Rc<[LirFunction]>>,
}

mod instr;

impl Emitter {
    pub fn new() -> Self {
        Emitter {
            bytecode: Bytecode::new(),
            label_offsets: HashMap::new(),
            pending_jumps: Vec::new(),
            stack: Vec::new(),
            reg_to_stack: HashMap::new(),
            symbol_names: HashMap::new(),
            yield_stack_state: HashMap::new(),
            block_entry_depth: HashMap::new(),
            yield_points: Vec::new(),
            call_sites: Vec::new(),
            current_func_may_suspend: false,
            current_func_num_locals: 0,
            compiled_closures: None,
            closure_lir_funcs: None,
        }
    }

    /// Create an emitter with symbol name mappings for cross-thread portability.
    pub fn new_with_symbols(symbol_names: HashMap<u32, String>) -> Self {
        Emitter {
            bytecode: Bytecode::new(),
            label_offsets: HashMap::new(),
            pending_jumps: Vec::new(),
            stack: Vec::new(),
            reg_to_stack: HashMap::new(),
            symbol_names,
            yield_stack_state: HashMap::new(),
            block_entry_depth: HashMap::new(),
            yield_points: Vec::new(),
            call_sites: Vec::new(),
            current_func_may_suspend: false,
            current_func_num_locals: 0,
            compiled_closures: None,
            closure_lir_funcs: None,
        }
    }

    /// Emit bytecode from an LIR module.
    ///
    /// Each closure is compiled independently via `emit`. The entry
    /// function's `MakeClosure` instructions reference pre-compiled
    /// closures by `ClosureId`.
    pub fn emit_module(&mut self, module: &LirModule) -> ClosureCompiled {
        // Compile closures in REVERSE order (post-order). Parents have
        // lower IDs than children (pre-order assignment), so compiling
        // in reverse ensures children are compiled before their parents.
        // This way a parent's MakeClosure can look up its child's
        // pre-compiled bytecode.
        let n = module.closures.len();
        self.closure_lir_funcs = Some(Rc::from(module.closures.as_slice()));
        // Pre-allocate with placeholders. Entries are filled in reverse
        // order; the MakeClosure handler only accesses children (higher
        // indices), which are filled before their parents.
        let mut compiled: Vec<ClosureCompiled> = (0..n)
            .map(|_| (Bytecode::new(), Vec::new(), Vec::new()))
            .collect();
        for i in (0..n).rev() {
            self.compiled_closures = Some(compiled);
            let result = self.emit(&module.closures[i]);
            compiled = self.compiled_closures.take().unwrap();
            compiled[i] = result;
        }
        // All closures compiled.
        self.compiled_closures = Some(compiled);
        let result = self.emit(&module.entry);
        self.compiled_closures = None;
        self.closure_lir_funcs = None;
        result
    }

    /// Compile all closures in a module, returning per-closure bytecodes.
    ///
    /// Like `emit_module` but returns the individual closure results
    /// instead of the entry function result. Used by the WASM backend
    /// for dual-compile (bytecode for spawn).
    pub fn emit_module_closures(&mut self, module: &LirModule) -> Vec<ClosureCompiled> {
        let n = module.closures.len();
        self.closure_lir_funcs = Some(Rc::from(module.closures.as_slice()));
        let mut compiled: Vec<ClosureCompiled> = (0..n)
            .map(|_| (Bytecode::new(), Vec::new(), Vec::new()))
            .collect();
        for i in (0..n).rev() {
            self.compiled_closures = Some(compiled);
            let result = self.emit(&module.closures[i]);
            compiled = self.compiled_closures.take().unwrap();
            compiled[i] = result;
        }
        self.compiled_closures = None;
        self.closure_lir_funcs = None;
        compiled
    }

    /// Set module context for MakeClosure resolution without
    /// pre-compiling all closures. Used by the JIT to compile a
    /// single closure that may contain MakeClosure instructions.
    pub fn set_module_context(&mut self, closures: &[LirFunction]) {
        self.closure_lir_funcs = Some(Rc::from(closures));
        // Pre-compile all closures so MakeClosure can look them up.
        // Uses reverse order (children before parents).
        let n = closures.len();
        let mut compiled: Vec<ClosureCompiled> = (0..n)
            .map(|_| (Bytecode::new(), Vec::new(), Vec::new()))
            .collect();
        for i in (0..n).rev() {
            self.compiled_closures = Some(compiled);
            let result = self.emit(&closures[i]);
            compiled = self.compiled_closures.take().unwrap();
            compiled[i] = result;
        }
        self.compiled_closures = Some(compiled);
    }

    /// Emit bytecode from a single LIR function.
    pub fn emit(&mut self, func: &LirFunction) -> ClosureCompiled {
        let mut bytecode = Bytecode::new();
        // Copy symbol names to the new bytecode for cross-thread portability
        bytecode.symbol_names = self.symbol_names.clone();
        self.bytecode = bytecode;
        self.label_offsets.clear();
        self.pending_jumps.clear();
        self.stack.clear();
        self.reg_to_stack.clear();
        self.yield_stack_state.clear();
        self.block_entry_depth.clear();
        self.yield_points.clear();
        self.call_sites.clear();
        self.current_func_may_suspend = func.signal.may_suspend();
        self.current_func_num_locals = func.num_locals;

        // Emit blocks in the order they were appended by the lowerer.
        //
        // The lowerer appends blocks by calling finish_block(), which means
        // predecessor blocks are always appended before their successors —
        // EXCEPT for merge/done blocks, which are left as `current_block`
        // and appended last (after all blocks that jump to them). This
        // guarantees that by the time the emitter processes a done/merge
        // block, all predecessors have already emitted their Jump/Branch
        // terminators and saved their stack state into yield_stack_state.
        //
        // Do NOT sort by label number. Labels are allocated in creation
        // order, not emission order. Constructs like `cond` and `match`
        // allocate the done_label first (giving it a low number) and the
        // arm blocks later (higher numbers). Sorting by label would cause
        // the done block to be emitted before its predecessors, losing the
        // stack state they carry.
        //
        // Invariant: func.blocks[0] is always the entry block (Label 0),
        // because the lowerer always starts with BasicBlock::new(Label(0))
        // and finish_block() appends it when the first branch is encountered.
        for block in &func.blocks {
            self.label_offsets
                .insert(block.label, self.bytecode.current_pos());
            self.emit_block(block, func);
        }

        // Patch jumps (relative i32 offsets)
        for (pos, label) in &self.pending_jumps {
            if let Some(&target) = self.label_offsets.get(label) {
                let offset = target as i32 - *pos as i32 - 4;
                self.bytecode.patch_jump(*pos, offset);
            }
        }

        // Carry this function's builder-idiom merge metadata into the bytecode so
        // the entry-function path (`Bytecode → Code`) mint-or-reuses merged slots —
        // the lambda path already carries it via `ClosureTemplate.merged_slots` (the
        // `MakeClosure` template build). Empty unless a merge fired.
        self.bytecode.merged_slots =
            std::rc::Rc::new(func.merged_slots.iter().map(|s| s.get()).collect());
        // Likewise the value-route release table, so the entry function's error
        // exit walks the releases its abandoned frame still owed.
        self.bytecode.frame_release_slots = std::rc::Rc::new(sorted_release_slots(func));
        self.bytecode.frame_release_regions = std::rc::Rc::new(sorted_release_regions(func));

        (
            std::mem::take(&mut self.bytecode),
            std::mem::take(&mut self.yield_points),
            std::mem::take(&mut self.call_sites),
        )
    }

    /// Emit bytecode from a nested LIR function (for closures)
    fn emit_block(&mut self, block: &BasicBlock, func: &LirFunction) {
        // Check if this block has saved stack state from a yield
        if let Some((saved_stack, saved_reg_map)) = self.yield_stack_state.remove(&block.label) {
            self.stack = saved_stack;
            self.reg_to_stack = saved_reg_map;
        } else {
            // Reset stack state at block entry
            self.stack.clear();
            self.reg_to_stack.clear();
        }

        // This block's operand depth is now fixed. Record it before the
        // instructions run: once the cursor is past a block, a back edge into it
        // has nothing else to trim against (`edge_depth`).
        self.block_entry_depth.insert(block.label, self.stack.len());

        // Pre-allocate local slots at the start of the entry block.
        //
        // The VM shares a single stack for both local variable slots
        // (addressed by StoreLocal/LoadLocal as frame_base + index) and
        // the operand stack.  Without pre-allocation, StoreLocal can
        // clobber operand values pushed by enclosing expressions (e.g.
        // the `1` in `(+ 1 (match 2 ...))`).
        //
        // By emitting num_locals Nil instructions here, we reserve
        // stack positions 0..num_locals for locals.  Operand values
        // start above the reserved area and are never clobbered.
        //
        // The simulated stack does NOT track these reserved slots —
        // all emitter operations (DupN, Pop, ensure_on_top) use
        // offsets relative to the stack top, so the constant base
        // offset is invisible to the simulation.
        if block.label == func.entry && func.num_locals > 0 {
            for _ in 0..func.num_locals {
                self.bytecode.emit(Instruction::Nil);
            }
        }

        // Emit instructions
        for spanned in &block.instructions {
            // Record source location before emitting the instruction
            self.bytecode.record_location(&spanned.span);
            self.emit_instr(&spanned.instr, func);
        }

        // Record source location for the terminator
        self.bytecode.record_location(&block.terminator.span);
        self.emit_terminator(&block.terminator.terminator);
    }

    /// The operand depth `label` is already fixed at, or `None` when this edge
    /// is the first to reach it and so gets to fix it.
    ///
    /// A block's depth is decided by whichever predecessor is emitted first —
    /// the simulation keeps that predecessor's stack and discards every later
    /// one (see `Terminator::Jump`'s `or_insert_with`). The record lives in
    /// `yield_stack_state` while the block is still ahead of the cursor, and
    /// moves to `block_entry_depth` when `emit_block` consumes it, so a back
    /// edge into an already-emitted loop header is answered too.
    fn edge_depth(&self, label: Label) -> Option<usize> {
        self.yield_stack_state
            .get(&label)
            .map(|(stack, _)| stack.len())
            .or_else(|| self.block_entry_depth.get(&label).copied())
    }

    fn emit_terminator(&mut self, term: &Terminator) {
        match term {
            Terminator::Return(reg) => {
                self.ensure_on_top(*reg);
                self.bytecode.emit(Instruction::Return);
            }

            Terminator::Jump(label) => {
                // Pop trailing orphan values so that all predecessors of a
                // merge block agree on the operand-stack depth.  Orphans are
                // created by DupN in ensure_on_top (e.g. inside the splice
                // path for `apply`).  Without this, branches that create
                // orphans leave a deeper stack than branches that don't,
                // causing wrong DupN offsets in the merge block.
                //
                // Bounded by the depth the target is already fixed at: a
                // `Terminator::Branch` edge into the same merge pops nothing, so
                // trimming past that depth would leave the two edges at
                // different depths — and the merge's successors, which inherited
                // the branch's simulation, would pop the orphan again on the
                // path that already dropped it (src/lir/AGENTS.md § "Merge
                // operand depth").
                let floor = self.edge_depth(*label).unwrap_or(0);
                self.pop_trailing_orphans_to(floor);

                // Save stack state for the target block if this is the first
                // predecessor to jump there. Multiple blocks may jump to the
                // same target (e.g., break + fallthrough, if/and/or merges).
                // We keep the FIRST saved state and ignore later ones — the
                // first predecessor is the reachable path (later predecessors
                // may be dead code after break with a wrong stack layout).
                if !self.label_offsets.contains_key(label) {
                    self.yield_stack_state
                        .entry(*label)
                        .or_insert_with(|| (self.stack.clone(), self.reg_to_stack.clone()));
                }

                self.bytecode.emit(Instruction::Jump);
                let pos = self.bytecode.current_pos();
                self.bytecode.emit_i32(0); // placeholder
                self.pending_jumps.push((pos, *label));
            }

            Terminator::Branch {
                cond,
                then_label,
                else_label,
            } => {
                self.ensure_on_top(*cond);

                // JumpIfFalse pops the condition from the stack
                self.pop();

                // Save stack state for both branches, but only if they haven't
                // been processed yet. This handles the case where blocks are
                // sorted by label and a target block might be processed before
                // the branch that jumps to it.
                if !self.label_offsets.contains_key(then_label) {
                    self.yield_stack_state
                        .insert(*then_label, (self.stack.clone(), self.reg_to_stack.clone()));
                }
                if !self.label_offsets.contains_key(else_label) {
                    self.yield_stack_state
                        .insert(*else_label, (self.stack.clone(), self.reg_to_stack.clone()));
                }

                // JumpIfFalse to else_label
                self.bytecode.emit(Instruction::JumpIfFalse);
                let else_pos = self.bytecode.current_pos();
                self.bytecode.emit_i32(0); // placeholder
                self.pending_jumps.push((else_pos, *else_label));

                // Fall through or jump to then_label
                self.bytecode.emit(Instruction::Jump);
                let then_pos = self.bytecode.current_pos();
                self.bytecode.emit_i32(0); // placeholder
                self.pending_jumps.push((then_pos, *then_label));
            }

            Terminator::Emit {
                signal,
                value,
                resume_label,
            } => {
                self.ensure_on_top(*value);
                // The whole mask is baked in: `(signal :keyword)` resolves to a
                // bit at analysis time, so nothing at runtime re-reads the
                // registry for a literal `emit`, and the operand is the only
                // place a user signal's bit (32-63) can live.
                self.bytecode.emit(Instruction::Emit);
                self.bytecode.emit_signal_bits(*signal);
                self.pop();

                let resume_ip = self.bytecode.current_pos();

                self.yield_points.push(YieldPointInfo {
                    resume_ip,
                    stack_regs: self.stack.clone(),
                    num_locals: self.current_func_num_locals,
                });

                self.yield_stack_state.insert(
                    *resume_label,
                    (self.stack.clone(), self.reg_to_stack.clone()),
                );

                self.bytecode.emit(Instruction::Jump);
                let pos = self.bytecode.current_pos();
                self.bytecode.emit_i32(0); // placeholder
                self.pending_jumps.push((pos, *resume_label));
            }

            Terminator::Unreachable => {
                // Emit nil and return as fallback
                self.bytecode.emit(Instruction::Nil);
                self.bytecode.emit(Instruction::Return);
            }
        }
    }

    /// Check if an upvalue index refers to a non-cell locally-defined variable.
    /// Returns `Some(stack_slot)` if it does, `None` otherwise.
    ///
    /// Environment layout: [captures... | params... | locals...]
    /// Stack layout: [params... | locals...] (num_locals slots pre-allocated)
    /// Conversion: stack_slot = env_index - num_captures
    fn non_cell_local_slot(index: u16, func: &LirFunction) -> Option<u16> {
        debug_assert!(
            func.num_params <= u16::MAX as usize,
            "num_params {} exceeds u16 range",
            func.num_params
        );
        let locals_start = func.num_captures + func.num_params as u16;
        if index >= locals_start {
            let local_offset = index - locals_start;
            // The mask names every local precisely, at any index: an unset slot
            // is a non-cell stack local; a set slot is a cell local reached via
            // the env. No >=64 conservatism (which forced — and leaked — cells
            // for uncaptured high locals).
            if !func.capture_locals_mask.is_set(local_offset as usize) {
                // Non-cell local: use stack slot
                Some(index - func.num_captures)
            } else {
                None // cell local: use env
            }
        } else {
            None // Capture or parameter: use env
        }
    }

    fn emit_const(&mut self, value: &LirConst, _func: &LirFunction) {
        match value {
            LirConst::Nil => {
                self.bytecode.emit(Instruction::Nil);
            }
            LirConst::EmptyList => {
                self.bytecode.emit(Instruction::EmptyList);
            }
            LirConst::Bool(true) => {
                self.bytecode.emit(Instruction::True);
            }
            LirConst::Bool(false) => {
                self.bytecode.emit(Instruction::False);
            }
            LirConst::Int(n) => {
                let idx = self.bytecode.add_constant(Value::int(*n));
                self.bytecode.emit(Instruction::LoadConst);
                self.bytecode.emit_u16(idx);
            }
            LirConst::Float(f) => {
                let idx = self.bytecode.add_constant(Value::float(*f));
                self.bytecode.emit(Instruction::LoadConst);
                self.bytecode.emit_u16(idx);
            }
            LirConst::String(_) => {
                // No producer emits `Const{LirConst::String}`. A string is a heap
                // value: in value position it lowers to a reclaimable
                // `MaterializeConst` (HirKind::String), and in a pattern it is
                // materialized-compared-freed (lir/lower/pattern.rs). The bytecode
                // constant pool has no region, so a string here would have nowhere
                // reclaimable to live. Hence: unreachable, loudly.
                unreachable!("string literals lower to MaterializeConst, not Const")
            }
            LirConst::Symbol(sym) => {
                let name = self.symbol_names.get(&sym.0).cloned().unwrap_or_default();
                let idx = self.bytecode.add_symbol(sym.0, &name);
                self.bytecode.emit(Instruction::LoadConst);
                self.bytecode.emit_u16(idx);
            }
            LirConst::Keyword(name) => {
                let idx = self.bytecode.add_constant(Value::keyword(name));
                self.bytecode.emit(Instruction::LoadConst);
                self.bytecode.emit_u16(idx);
            }
            LirConst::ClosureRef(_) => {
                panic!(
                    "bug: ClosureRef in emitter — should have been patched during reconstruction"
                )
            }
            LirConst::ValueRef(_) => {
                panic!("bug: ValueRef in emitter — should have been patched during reconstruction")
            }
        }
    }
}

impl Default for Emitter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
