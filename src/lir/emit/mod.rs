// audited: 2026-10-06
// docs/impl/bytecode.md
//! Converts register-based LIR to stack-based bytecode, simulating the operand
//! stack to find where each register's value sits. `edge` emits the block
//! terminators, `stack` holds the simulation, `instr` the instructions.

mod edge;
mod stack;

use super::code::{BlockRef, ConstRef, FrozenModule, InstrRef, LirOwned, LirView};
use super::types::*;
use crate::compiler::bytecode::{Bytecode, Instruction};
use crate::value::Value;
use std::collections::HashMap;
use std::rc::Rc;

/// Per-closure compilation result: bytecode, yield points, call sites.
pub type ClosureCompiled = (Bytecode, Vec<YieldPointInfo>, Vec<CallSiteInfo>);

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
    /// The simulation a block still ahead of the cursor starts from, keyed by
    /// its label. A `Jump` saves it for its target, a `Branch` for both
    /// targets, and an `Emit` for its resume block.
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
    /// The frozen LIR of each closure. Parallel to `compiled_closures`.
    /// Needed by MakeClosure to write the closure's payload.
    closure_lir_funcs: Option<Rc<[LirOwned]>>,
    /// The code region this emission writes its payloads into.
    arena: crate::value::CodeArena,
    /// The header over each closure's payload, once a `MakeClosure` has
    /// written it, so a lambda two sites build is one code object.
    lambda_headers: HashMap<ClosureId, Value>,
}

mod instr;

impl Emitter {
    /// An emitter writing the code objects it builds into `arena`.
    pub fn new(arena: crate::value::CodeArena) -> Self {
        Emitter {
            arena,
            bytecode: Bytecode::new(),
            label_offsets: HashMap::new(),
            pending_jumps: Vec::new(),
            stack: Vec::new(),
            reg_to_stack: HashMap::new(),
            yield_stack_state: HashMap::new(),
            block_entry_depth: HashMap::new(),
            yield_points: Vec::new(),
            call_sites: Vec::new(),
            current_func_may_suspend: false,
            current_func_num_locals: 0,
            compiled_closures: None,
            closure_lir_funcs: None,
            lambda_headers: HashMap::new(),
        }
    }

    /// The header over closure `id`'s payload in the unit's code region,
    /// writing the payload the first time a `MakeClosure` names it.
    ///
    /// Panics outside module emission: the closure's compiled bytecode and its
    /// frozen LIR come from `emit_module`'s pass, so code holding a
    /// `MakeClosure` is emitted through it.
    fn lambda_header(&mut self, id: ClosureId, num_captures: usize) -> Value {
        if let Some(&header) = self.lambda_headers.get(&id) {
            return header;
        }
        let compiled = self
            .compiled_closures
            .as_ref()
            .expect("MakeClosure without compiled_closures context")
            .get(id.0 as usize)
            .expect("MakeClosure: invalid ClosureId")
            .clone();
        let func = &self
            .closure_lir_funcs
            .as_ref()
            .expect("MakeClosure without closure_lir_funcs context")[id.0 as usize];
        let parts = crate::value::PayloadParts::lambda(func, num_captures, compiled);
        let header = self.arena.header(self.arena.write(parts));
        self.lambda_headers.insert(id, header);
        header
    }

    /// Emit bytecode from an LIR module.
    ///
    /// Each closure is compiled independently via `emit`. The entry
    /// function's `MakeClosure` instructions reference pre-compiled
    /// closures by `ClosureId`.
    pub fn emit_module(&mut self, module: &FrozenModule) -> ClosureCompiled {
        self.compile_closures(module);
        let result = self.emit(&module.entry.view());
        self.compiled_closures = None;
        self.closure_lir_funcs = None;
        result
    }

    /// Emit `module` as [`emit_module`](Self::emit_module) does, and answer the
    /// header over every closure's payload beside the entry's result, indexed
    /// by `ClosureId`. The WASM backend's dual compile reads these: its host
    /// builds each closure's code object from the closure's own payload
    /// (docs/impl/wasm.md).
    pub fn emit_module_with_lambdas(
        &mut self,
        module: &FrozenModule,
    ) -> (ClosureCompiled, Vec<Value>) {
        self.compile_closures(module);
        let result = self.emit(&module.entry.view());
        // A closure no `MakeClosure` named is written here, with the capture
        // count its own LIR records.
        let lambdas = (0..module.closures.len())
            .map(|i| {
                let captures = module.closures[i].view().num_captures() as usize;
                self.lambda_header(ClosureId(i as u32), captures)
            })
            .collect();
        self.compiled_closures = None;
        self.closure_lir_funcs = None;
        (result, lambdas)
    }

    /// Emit every closure of `module` and keep the results, so the entry's
    /// `MakeClosure`s find them.
    fn compile_closures(&mut self, module: &FrozenModule) {
        // Compile closures in REVERSE order (post-order). Parents have
        // lower IDs than children (pre-order assignment), so compiling
        // in reverse ensures children are compiled before their parents.
        // This way a parent's MakeClosure can look up its child's
        // pre-compiled bytecode.
        let n = module.closures.len();
        self.closure_lir_funcs = Some(Rc::from(module.closures.as_slice()));
        self.lambda_headers.clear();
        // Pre-allocate with placeholders. Entries are filled in reverse
        // order; the MakeClosure handler only accesses children (higher
        // indices), which are filled before their parents.
        let mut compiled: Vec<ClosureCompiled> = (0..n)
            .map(|_| (Bytecode::new(), Vec::new(), Vec::new()))
            .collect();
        for i in (0..n).rev() {
            self.compiled_closures = Some(compiled);
            let result = self.emit(&module.closures[i].view());
            compiled = self.compiled_closures.take().unwrap();
            compiled[i] = result;
        }
        // All closures compiled.
        self.compiled_closures = Some(compiled);
    }

    /// Emit bytecode from a single LIR function.
    pub fn emit(&mut self, func: &LirView<'_>) -> ClosureCompiled {
        self.bytecode = Bytecode::new();
        self.label_offsets.clear();
        self.pending_jumps.clear();
        self.stack.clear();
        self.reg_to_stack.clear();
        self.yield_stack_state.clear();
        self.block_entry_depth.clear();
        self.yield_points.clear();
        self.call_sites.clear();
        self.current_func_may_suspend = func.signal().may_suspend();
        self.current_func_num_locals = func.num_locals();

        // Emit blocks in the order they were appended by the lowerer.
        //
        // The lowerer appends blocks by calling finish_block(), which means
        // predecessor blocks are always appended before their successors —
        // EXCEPT for merge/done blocks, which are left as `current_block`
        // and appended last (after all blocks that jump to them). This
        // guarantees that by the time the emitter processes a done/merge
        // block, all predecessors have already emitted their Jump/Branch
        // terminators and saved their stack state into yield_stack_state.
        // Freezing keeps the order.
        //
        // Do NOT sort by label number. Labels are allocated in creation
        // order, not emission order. Constructs like `cond` and `match`
        // allocate the done_label first (giving it a low number) and the
        // arm blocks later (higher numbers). Sorting by label would cause
        // the done block to be emitted before its predecessors, losing the
        // stack state they carry.
        //
        // Invariant: the first block is always the entry block (Label 0),
        // because the lowerer always starts with BasicBlock::new(Label(0))
        // and finish_block() appends it when the first branch is encountered.
        for block in func.blocks() {
            self.label_offsets
                .insert(block.label(), self.bytecode.current_pos());
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
        // the entry function's payload mint-or-reuses merged slots — a lambda's
        // payload reads them off its frozen function. Empty unless a merge fired.
        // Freezing records both these and the release tables ascending.
        self.bytecode.merged_slots = func.merged_slots().iter().map(|s| s.get()).collect();
        // Likewise the value-route release table, so the entry function's error
        // exit walks the releases its abandoned frame still owed.
        self.bytecode.frame_release_slots = func.frame_release_slots().to_vec();
        self.bytecode.frame_release_regions = func
            .frame_release_regions()
            .iter()
            .map(|r| r.get())
            .collect();

        // A frozen function's values have no owner of their own: the constant
        // pool keeps them alive (src/lir/AGENTS.md invariant 7).
        debug_assert!(
            func.values()
                .iter()
                .all(|v| self.bytecode.constants.contains(v)),
            "every ValueConst value is in the constant pool"
        );

        (
            std::mem::take(&mut self.bytecode),
            std::mem::take(&mut self.yield_points),
            std::mem::take(&mut self.call_sites),
        )
    }

    /// Emit one basic block: its instructions in order, then its terminator.
    fn emit_block(&mut self, block: BlockRef<'_>, func: &LirView<'_>) {
        let label = block.label();
        // Check if this block has saved stack state from a yield
        if let Some((saved_stack, saved_reg_map)) = self.yield_stack_state.remove(&label) {
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
        self.block_entry_depth.insert(label, self.stack.len());

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
        if label == func.entry() && func.num_locals() > 0 {
            for _ in 0..func.num_locals() {
                self.bytecode.emit(Instruction::Nil);
            }
        }

        // Emit instructions
        for node in block.nodes() {
            // Record source location before emitting the instruction
            self.bytecode.record_location(&node.span());
            self.emit_instr(&node.instr(), func);
        }

        // Record source location for the terminator
        self.bytecode.record_location(&block.terminator_span());
        self.emit_terminator(&block.terminator());
    }

    /// Check if an upvalue index refers to a non-cell locally-defined variable.
    /// Returns `Some(stack_slot)` if it does, `None` otherwise.
    ///
    /// Environment layout: [captures... | params... | locals...]
    /// Stack layout: [params... | locals...] (num_locals slots pre-allocated)
    /// Conversion: stack_slot = env_index - num_captures
    fn non_cell_local_slot(index: u16, func: &LirView<'_>) -> Option<u16> {
        debug_assert!(
            func.num_params() <= u16::MAX as usize,
            "num_params {} exceeds u16 range",
            func.num_params()
        );
        let locals_start = func.num_captures() + func.num_params() as u16;
        if index >= locals_start {
            let local_offset = index - locals_start;
            // The mask names every local precisely, at any index: an unset slot
            // is a non-cell stack local; a set slot is a cell local reached via
            // the env. No >=64 conservatism (which forced — and leaked — cells
            // for uncaptured high locals).
            if !func.capture_locals_mask().is_set(local_offset as usize) {
                // Non-cell local: use stack slot
                Some(index - func.num_captures())
            } else {
                None // cell local: use env
            }
        } else {
            None // Capture or parameter: use env
        }
    }

    fn emit_const(&mut self, value: ConstRef) {
        match value {
            ConstRef::Nil => {
                self.bytecode.emit(Instruction::Nil);
            }
            ConstRef::EmptyList => {
                self.bytecode.emit(Instruction::EmptyList);
            }
            ConstRef::Bool(true) => {
                self.bytecode.emit(Instruction::True);
            }
            ConstRef::Bool(false) => {
                self.bytecode.emit(Instruction::False);
            }
            ConstRef::Int(n) => {
                let idx = self.bytecode.add_constant(Value::int(n));
                self.bytecode.emit(Instruction::LoadConst);
                self.bytecode.emit_u16(idx);
            }
            ConstRef::Float(f) => {
                let idx = self.bytecode.add_constant(Value::float(f));
                self.bytecode.emit(Instruction::LoadConst);
                self.bytecode.emit_u16(idx);
            }
            ConstRef::Symbol(sym) => {
                let idx = self.bytecode.add_constant(Value::symbol(sym));
                self.bytecode.emit(Instruction::LoadConst);
                self.bytecode.emit_u16(idx);
            }
            ConstRef::Keyword(hash) => {
                let idx = self.bytecode.add_constant(Value::keyword_from_hash(hash));
                self.bytecode.emit(Instruction::LoadConst);
                self.bytecode.emit_u16(idx);
            }
        }
    }
}

impl Emitter {
    /// The code region this emission writes into.
    pub fn arena(&self) -> crate::value::CodeArena {
        self.arena
    }
}

#[cfg(test)]
mod tests;
