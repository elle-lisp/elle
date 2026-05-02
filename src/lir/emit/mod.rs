//! LIR to Bytecode emission
//!
//! Converts register-based LIR to stack-based bytecode.
//! Uses a simple stack simulation to track register values.

mod stack;

use super::types::*;
use crate::compiler::bytecode::{Bytecode, Instruction};
use crate::value::fiber::SignalBits;
use crate::value::{Closure, Value};
use std::collections::HashMap;
use std::rc::Rc;

/// Per-closure compilation result: bytecode, yield points, call sites.
type ClosureCompiled = (Bytecode, Vec<YieldPointInfo>, Vec<CallSiteInfo>);

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

    fn emit_instr(&mut self, instr: &LirInstr, func: &LirFunction) {
        match instr {
            LirInstr::Const { dst, value } => {
                self.emit_const(value, func);
                self.push_reg(*dst);
            }

            LirInstr::ValueConst { dst, value } => {
                let const_idx = self.bytecode.add_constant(*value);
                self.bytecode.emit(Instruction::LoadConst);
                self.bytecode.emit_u16(const_idx);
                self.push_reg(*dst);
            }

            LirInstr::LoadLocal { dst, slot } => {
                self.bytecode.emit(Instruction::LoadLocal);
                self.bytecode.emit_u16(*slot);
                self.push_reg(*dst);
            }

            LirInstr::StoreLocal { slot, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::StoreLocal);
                self.bytecode.emit_u16(*slot);
                // StoreLocal pops the value, stores it, and pushes it back.
                // Auto-pop: consume the pushed-back value so stores are pure
                // side effects from the LIR's perspective.
                self.bytecode.emit(Instruction::Pop);
                self.pop();
            }

            LirInstr::LoadCapture { dst, index } => {
                if let Some(stack_slot) = Self::non_cell_local_slot(*index, func) {
                    // Non-cell locally-defined variable: use stack
                    self.bytecode.emit(Instruction::LoadLocal);
                    self.bytecode.emit_u16(stack_slot);
                } else {
                    self.bytecode.emit(Instruction::LoadUpvalue);
                    self.bytecode.emit_byte(0); // depth (currently unused)
                    self.bytecode.emit_u16(*index);
                }
                self.push_reg(*dst);
            }

            LirInstr::LoadCaptureRaw { dst, index } => {
                // Load without unwrapping cells - used for forwarding captures
                self.bytecode.emit(Instruction::LoadUpvalueRaw);
                self.bytecode.emit_byte(0); // depth (currently unused)
                self.bytecode.emit_u16(*index);
                self.push_reg(*dst);
            }

            LirInstr::StoreCapture { index, src } => {
                self.ensure_on_top(*src);
                if let Some(stack_slot) = Self::non_cell_local_slot(*index, func) {
                    // Non-cell locally-defined variable: use stack
                    self.bytecode.emit(Instruction::StoreLocal);
                    self.bytecode.emit_u16(stack_slot);
                } else {
                    self.bytecode.emit(Instruction::StoreUpvalue);
                    self.bytecode.emit_byte(0); // depth (currently unused)
                    self.bytecode.emit_u16(*index);
                }
                // Both StoreLocal and StoreUpvalue pop-then-push-back.
                // Auto-pop: consume the pushed-back value.
                self.bytecode.emit(Instruction::Pop);
                self.pop();
            }

            LirInstr::MakeClosure {
                dst,
                closure_id,
                captures,
            } => {
                // Check if captures are already in order on top of stack
                let stack_len = self.stack.len();
                let mut all_in_place = stack_len >= captures.len();
                if all_in_place {
                    let base = stack_len - captures.len();
                    for (i, cap) in captures.iter().enumerate() {
                        if self.reg_to_stack.get(cap) != Some(&(base + i)) {
                            all_in_place = false;
                            break;
                        }
                    }
                }

                if !all_in_place {
                    // Captures not in place - need to arrange them
                    for cap in captures {
                        self.ensure_on_top(*cap);
                    }
                }

                // Look up the pre-compiled closure by ClosureId.
                // In emit_module mode, closures are pre-compiled.
                // In standalone emit mode (tests), this panics — callers
                // must use emit_module for code with MakeClosure.
                let compiled = self
                    .compiled_closures
                    .as_ref()
                    .expect("MakeClosure without compiled_closures context")
                    .get(closure_id.0 as usize)
                    .expect("MakeClosure: invalid ClosureId");
                let (nested_bytecode, nested_yield_points, nested_call_sites) = compiled.clone();

                // Look up the LirFunction from the module for metadata.
                // We need the LirFunction for the ClosureTemplate (arity,
                // signal, lbox masks, etc). The compiled_closures Vec is
                // parallel to the module's closures Vec.
                let func = &self
                    .closure_lir_funcs
                    .as_ref()
                    .expect("MakeClosure without closure_lir_funcs context")
                    [closure_id.0 as usize];

                let mut nested_lir = func.clone();
                nested_lir.yield_points = nested_yield_points;
                nested_lir.call_sites = nested_call_sites;

                // Create closure template
                let template = crate::value::ClosureTemplate {
                    bytecode: Rc::new(nested_bytecode.instructions),
                    arity: func.arity,
                    num_locals: func.num_locals as usize,
                    num_captures: captures.len(),
                    num_params: func.num_params,
                    constants: Rc::new(nested_bytecode.constants),
                    signal: func.signal,
                    capture_params_mask: func.capture_params_mask,
                    capture_locals_mask: func.capture_locals_mask,
                    symbol_names: Rc::new(nested_bytecode.symbol_names),
                    location_map: Rc::new(nested_bytecode.location_map),
                    lir_function: Some(Rc::new(nested_lir)),
                    doc: func.doc,
                    syntax: func.syntax.clone(),
                    vararg_kind: func.vararg_kind.clone(),
                    name: func.name.clone().map(|s| Rc::from(s.as_str())),
                    result_is_immediate: func.result_is_immediate,
                    has_outward_heap_set: func.has_outward_heap_set,
                    wasm_func_idx: None,
                    spirv: std::cell::OnceCell::new(),

                    rotation_safe: func.rotation_safe,
                };
                let closure = Closure {
                    template: Rc::new(template),
                    env: crate::value::inline_slice::InlineSlice::empty(),
                    squelch_mask: SignalBits::EMPTY,
                };

                // Add closure template to constants
                let const_idx = self.bytecode.add_constant(Value::closure(closure));

                // Emit MakeClosure instruction
                self.bytecode.emit(Instruction::MakeClosure);
                self.bytecode.emit_u16(const_idx);
                self.bytecode.emit_u16(captures.len() as u16);

                // Pop captures, push closure
                for _ in captures {
                    self.pop();
                }
                self.push_reg(*dst);
            }

            LirInstr::Call { dst, func, args } | LirInstr::SuspendingCall { dst, func, args } => {
                // Call expects: [arg1, arg2, ..., argN, func] on stack
                // Check if values are already in the correct positions at the top of the stack
                let total_values = args.len() + 1; // args + func
                let stack_len = self.stack.len();

                // Check if all values are already in place
                let mut all_in_place = stack_len >= total_values;
                if all_in_place {
                    let base = stack_len - total_values;
                    for (i, arg) in args.iter().enumerate() {
                        if self.reg_to_stack.get(arg) != Some(&(base + i)) {
                            all_in_place = false;
                            break;
                        }
                    }
                    if all_in_place && self.reg_to_stack.get(func) != Some(&(base + args.len())) {
                        all_in_place = false;
                    }
                }

                if !all_in_place {
                    // Values are not in place, need to duplicate them to the top
                    for arg in args {
                        self.ensure_on_top(*arg);
                    }
                    self.ensure_on_top(*func);
                }

                self.bytecode.emit(Instruction::Call);
                self.bytecode.emit_u16(args.len() as u16);
                let call_resume_ip = self.bytecode.current_pos();

                // Pop func and args from simulated stack
                self.pop(); // func
                for _ in args {
                    self.pop();
                }

                // Record call site metadata AFTER popping func/args, BEFORE
                // pushing result. This matches the interpreter's stack state
                // when yield propagates through a call: the Call instruction
                // has consumed its operands, the callee yielded, and the
                // interpreter saves the remaining stack.
                if self.current_func_may_suspend {
                    self.call_sites.push(CallSiteInfo {
                        resume_ip: call_resume_ip,
                        stack_regs: self.stack.clone(),
                        num_locals: self.current_func_num_locals,
                    });
                }

                self.push_reg(*dst);
            }

            LirInstr::TailCall { func, args } => {
                // Check if values are already in the correct positions at the top of the stack
                let total_values = args.len() + 1; // args + func
                let stack_len = self.stack.len();

                let mut all_in_place = stack_len >= total_values;
                if all_in_place {
                    let base = stack_len - total_values;
                    for (i, arg) in args.iter().enumerate() {
                        if self.reg_to_stack.get(arg) != Some(&(base + i)) {
                            all_in_place = false;
                            break;
                        }
                    }
                    if all_in_place && self.reg_to_stack.get(func) != Some(&(base + args.len())) {
                        all_in_place = false;
                    }
                }

                if !all_in_place {
                    for arg in args {
                        self.ensure_on_top(*arg);
                    }
                    self.ensure_on_top(*func);
                }
                self.bytecode.emit(Instruction::TailCall);
                self.bytecode.emit_u16(args.len() as u16);
            }

            LirInstr::List { dst, head, tail } => {
                // VM pops rest (top) then first (below), calls pair(first, rest).
                // Push head first (it becomes below = first), then tail (top = rest).
                self.ensure_on_top(*head);
                self.ensure_on_top(*tail);
                self.bytecode.emit(Instruction::Pair);
                self.pop();
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::MakeArrayMut { dst, elements } => {
                for elem in elements {
                    self.ensure_on_top(*elem);
                }
                self.bytecode.emit(Instruction::MakeArrayMut);
                self.bytecode.emit_byte(elements.len() as u8);
                for _ in elements {
                    self.pop();
                }
                self.push_reg(*dst);
            }

            LirInstr::First { dst, pair } => {
                self.ensure_on_top(*pair);
                self.bytecode.emit(Instruction::First);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::Rest { dst, pair } => {
                self.ensure_on_top(*pair);
                self.bytecode.emit(Instruction::Rest);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::FirstDestructure { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::FirstDestructure);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::RestDestructure { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::RestDestructure);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::ArrayMutRefDestructure { dst, src, index } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::ArrayMutRefDestructure);
                self.bytecode.emit_u16(*index);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::ArrayMutSliceFrom { dst, src, index } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::ArrayMutSliceFrom);
                self.bytecode.emit_u16(*index);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::StructGetOrNil { dst, src, key } => {
                self.ensure_on_top(*src);
                let key_value = match key {
                    LirConst::Keyword(name) => Value::keyword(name),
                    LirConst::String(s) => Value::string(s.clone()),
                    LirConst::Int(n) => Value::int(*n),
                    LirConst::Symbol(sym) => Value::symbol(sym.0),
                    LirConst::Bool(b) => Value::bool(*b),
                    LirConst::Nil => Value::NIL,
                    _ => panic!("StructGetOrNil: unsupported key type"),
                };
                let const_idx = self.bytecode.add_constant(key_value);
                self.bytecode.emit(Instruction::StructGetOrNil);
                self.bytecode.emit_u16(const_idx);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::StructGetDestructure { dst, src, key } => {
                self.ensure_on_top(*src);
                let key_value = match key {
                    LirConst::Keyword(name) => Value::keyword(name),
                    LirConst::String(s) => Value::string(s.clone()),
                    LirConst::Int(n) => Value::int(*n),
                    LirConst::Symbol(sym) => Value::symbol(sym.0),
                    LirConst::Bool(b) => Value::bool(*b),
                    LirConst::Nil => Value::NIL,
                    _ => panic!("StructGetDestructure: unsupported key type"),
                };
                let const_idx = self.bytecode.add_constant(key_value);
                self.bytecode.emit(Instruction::StructGetDestructure);
                self.bytecode.emit_u16(const_idx);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::StructRest {
                dst,
                src,
                exclude_keys,
            } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::StructRest);
                self.bytecode.emit_u16(exclude_keys.len() as u16);
                for key in exclude_keys {
                    let key_value = match key {
                        LirConst::Keyword(name) => Value::keyword(name),
                        LirConst::Symbol(sid) => Value::symbol(sid.0),
                        _ => panic!("StructRest: unsupported key type {:?}", key),
                    };
                    let const_idx = self.bytecode.add_constant(key_value);
                    self.bytecode.emit_u16(const_idx);
                }
                self.pop();
                self.push_reg(*dst);
            }

            // Silent destructuring (parameter context: absent optional params → nil)
            LirInstr::FirstOrNil { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::FirstOrNil);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::RestOrNil { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::RestOrNil);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::ArrayMutRefOrNil { dst, src, index } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::ArrayMutRefOrNil);
                self.bytecode.emit_u16(*index);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::BinOp { dst, op, lhs, rhs } => {
                // Check if lhs and rhs are already the top two stack elements
                // (lhs at top-1, rhs at top). This is the common case from the
                // lowerer and avoids DupN which would leave orphaned values.
                self.ensure_binary_on_top(*lhs, *rhs);
                let instr = match op {
                    BinOp::Add => Instruction::Add,
                    BinOp::Sub => Instruction::Sub,
                    BinOp::Mul => Instruction::Mul,
                    BinOp::Div => Instruction::Div,
                    BinOp::Rem => Instruction::Rem,
                    BinOp::BitAnd => Instruction::BitAnd,
                    BinOp::BitOr => Instruction::BitOr,
                    BinOp::BitXor => Instruction::BitXor,
                    BinOp::Shl => Instruction::Shl,
                    BinOp::Shr => Instruction::Shr,
                };
                self.bytecode.emit(instr);
                self.pop();
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::Compare { dst, op, lhs, rhs } => {
                // Check if lhs and rhs are already the top two stack elements
                self.ensure_binary_on_top(*lhs, *rhs);
                let instr = match op {
                    CmpOp::Eq => Instruction::Eq,
                    CmpOp::Lt => Instruction::Lt,
                    CmpOp::Gt => Instruction::Gt,
                    CmpOp::Le => Instruction::Le,
                    CmpOp::Ge => Instruction::Ge,
                    CmpOp::Ne => Instruction::Eq, // Will need Not after
                };
                self.bytecode.emit(instr);
                if matches!(op, CmpOp::Ne) {
                    self.bytecode.emit(Instruction::Not);
                }
                self.pop();
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::UnaryOp { dst, op, src } => {
                self.ensure_on_top(*src);
                match op {
                    UnaryOp::Not => self.bytecode.emit(Instruction::Not),
                    UnaryOp::Neg => {
                        // Negate by multiplying by -1.
                        // Stack has src on top; push -1, then Mul.
                        let neg1_idx = self.bytecode.add_constant(Value::int(-1));
                        self.bytecode.emit(Instruction::LoadConst);
                        self.bytecode.emit_u16(neg1_idx);
                        self.bytecode.emit(Instruction::Mul);
                    }
                    UnaryOp::BitNot => {
                        self.bytecode.emit(Instruction::BitNot);
                    }
                }
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::Convert { dst, op, src } => {
                self.ensure_on_top(*src);
                match op {
                    ConvOp::IntToFloat => self.bytecode.emit(Instruction::IntToFloat),
                    ConvOp::FloatToInt => self.bytecode.emit(Instruction::FloatToInt),
                }
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::IsNil { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsNil);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::IsPair { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsPair);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::IsArray { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsArray);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::IsArrayMut { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsArrayMut);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::IsStruct { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsStruct);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::IsStructMut { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsStructMut);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::IsSet { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsSet);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::IsSetMut { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsSetMut);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::ArrayMutLen { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::ArrayMutLen);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::MakeCaptureCell { dst, value } => {
                self.ensure_on_top(*value);
                self.bytecode.emit(Instruction::MakeCapture);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::LoadCaptureCell { dst, cell } => {
                self.ensure_on_top(*cell);
                self.bytecode.emit(Instruction::UnwrapCapture);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::StoreCaptureCell { cell, value } => {
                self.ensure_on_top(*cell);
                self.ensure_on_top(*value);
                self.bytecode.emit(Instruction::UpdateCapture);
                // UpdateCapture pops value, pops cell, pushes value back.
                // Unlike other stores, UpdateCapture pushes the value back.
                // We do NOT auto-pop here because lower_set needs the value.
                self.pop(); // value (consumed by UpdateCapture, re-pushed)
                self.pop(); // cell (consumed by UpdateCapture)
                            // Value is now on the stack (pushed back by UpdateCapture).
                self.push_reg(*value);
            }

            LirInstr::LoadResumeValue { dst } => {
                // The resume value is already on the operand stack
                // (pushed by the VM's resume_continuation).
                // The stack simulation already has the pre-yield state.
                // Just register the resume value.
                self.push_reg(*dst);
            }

            LirInstr::Eval { dst, expr, env } => {
                // Stack order: env on bottom, expr on top
                // (VM pops expr first, then env)
                self.ensure_on_top(*env);
                self.ensure_on_top(*expr);
                self.bytecode.emit(Instruction::Eval);
                // Eval pops 2 values and pushes 1 result
                self.pop(); // expr
                self.pop(); // env
                self.push_reg(*dst);
            }

            LirInstr::ArrayMutExtend { dst, array, source } => {
                // Stack: [array, source] → [extended_array]
                self.ensure_binary_on_top(*array, *source);
                self.bytecode.emit(Instruction::ArrayMutExtend);
                self.pop(); // source
                self.pop(); // array
                self.push_reg(*dst);
            }

            LirInstr::ArrayMutPush { dst, array, value } => {
                // Stack: [array, value] → [extended_array]
                self.ensure_binary_on_top(*array, *value);
                self.bytecode.emit(Instruction::ArrayMutPush);
                self.pop(); // value
                self.pop(); // array
                self.push_reg(*dst);
            }

            LirInstr::CallArrayMut { dst, func, args } => {
                // Stack: [func, args_array] → [result]
                self.ensure_binary_on_top(*func, *args);
                self.bytecode.emit(Instruction::CallArrayMut);
                let call_resume_ip = self.bytecode.current_pos();
                self.pop(); // args
                self.pop(); // func

                if self.current_func_may_suspend {
                    self.call_sites.push(CallSiteInfo {
                        resume_ip: call_resume_ip,
                        stack_regs: self.stack.clone(),
                        num_locals: self.current_func_num_locals,
                    });
                }

                self.push_reg(*dst);
            }

            LirInstr::TailCallArrayMut { func, args } => {
                // Stack: [func, args_array] → (tail call, no push)
                self.ensure_binary_on_top(*func, *args);
                self.bytecode.emit(Instruction::TailCallArrayMut);
                self.pop(); // args
                self.pop(); // func
            }

            LirInstr::RegionEnter => {
                self.bytecode.emit(Instruction::RegionEnter);
                // No stack effect
            }

            LirInstr::RegionExit => {
                self.bytecode.emit(Instruction::RegionExit);
                // No stack effect
            }

            LirInstr::RegionExitCall => {
                self.bytecode.emit(Instruction::RegionExitCall);
                // No stack effect
            }

            LirInstr::OutboxEnter => {
                self.bytecode.emit(Instruction::OutboxEnter);
                // No stack effect
            }

            LirInstr::OutboxExit => {
                self.bytecode.emit(Instruction::OutboxExit);
                // No stack effect
            }

            LirInstr::FlipEnter => {
                self.bytecode.emit(Instruction::FlipEnter);
            }
            LirInstr::FlipSwap => {
                self.bytecode.emit(Instruction::FlipSwap);
            }
            LirInstr::FlipExit => {
                self.bytecode.emit(Instruction::FlipExit);
            }

            LirInstr::PushParamFrame { pairs } => {
                // Push all param/value pairs onto the stack
                for (param, value) in pairs {
                    self.ensure_on_top(*param);
                    self.ensure_on_top(*value);
                }
                self.bytecode.emit(Instruction::PushParamFrame);
                self.bytecode.emit_byte(pairs.len() as u8);
                // All pairs consumed from stack
                for _ in pairs {
                    self.pop(); // value
                    self.pop(); // param
                }
            }

            LirInstr::PopParamFrame => {
                self.bytecode.emit(Instruction::PopParamFrame);
                // No stack effect
            }

            // === New type predicates ===
            LirInstr::IsEmpty { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsEmptyList);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsBool { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsBool);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsInt { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsInt);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsFloat { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsFloat);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsString { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsString);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsKeyword { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsKeyword);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsSymbolCheck { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsSymbol);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsBytes { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsBytes);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsBox { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsBox);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsClosure { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsClosure);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsFiber { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsFiber);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::TypeOf { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::TypeOf);
                self.pop();
                self.push_reg(*dst);
            }

            // === Data access ===
            LirInstr::Length { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::Length);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::Get { dst, obj, key } => {
                self.ensure_binary_on_top(*obj, *key);
                self.bytecode.emit(Instruction::IntrGet);
                self.pop(); // key
                self.pop(); // obj
                self.push_reg(*dst);
            }
            LirInstr::Put { dst, obj, key, val } => {
                self.ensure_on_top(*obj);
                self.ensure_on_top(*key);
                self.ensure_on_top(*val);
                self.bytecode.emit(Instruction::IntrPut);
                self.pop(); // val
                self.pop(); // key
                self.pop(); // obj
                self.push_reg(*dst);
            }
            LirInstr::Del { dst, obj, key } => {
                self.ensure_binary_on_top(*obj, *key);
                self.bytecode.emit(Instruction::IntrDel);
                self.pop(); // key
                self.pop(); // obj
                self.push_reg(*dst);
            }
            LirInstr::Has { dst, obj, key } => {
                self.ensure_binary_on_top(*obj, *key);
                self.bytecode.emit(Instruction::IntrHas);
                self.pop(); // key
                self.pop(); // obj
                self.push_reg(*dst);
            }
            LirInstr::IntrPush { dst, array, value } => {
                self.ensure_binary_on_top(*array, *value);
                self.bytecode.emit(Instruction::IntrPush);
                self.pop(); // value
                self.pop(); // array
                self.push_reg(*dst);
            }
            LirInstr::Pop { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IntrPop);
                self.pop();
                self.push_reg(*dst);
            }

            // === Mutability ===
            LirInstr::Freeze { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IntrFreeze);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::Thaw { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IntrThaw);
                self.pop();
                self.push_reg(*dst);
            }

            // === Identity ===
            LirInstr::Identical { dst, lhs, rhs } => {
                self.ensure_binary_on_top(*lhs, *rhs);
                self.bytecode.emit(Instruction::Identical);
                self.pop(); // rhs
                self.pop(); // lhs
                self.push_reg(*dst);
            }

            LirInstr::CheckSignalBound { src, allowed_bits } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::CheckSignalBound);
                // Emit SignalBits raw value as four u16s (least-significant first)
                let raw = allowed_bits.raw();
                self.bytecode.emit_u16(raw as u16);
                self.bytecode.emit_u16((raw >> 16) as u16);
                self.bytecode.emit_u16((raw >> 32) as u16);
                self.bytecode.emit_u16((raw >> 48) as u16);
                // Value consumed by the check
                self.pop();
            }
        }
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
                self.pop_trailing_orphans();

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
                // Emit instruction with signal bits as u16 operand.
                // Only bits 0-15 (built-in) are encoded here; user-defined
                // signals (bits 32-63) are resolved at runtime via the
                // signal registry, not baked into bytecode.
                self.bytecode.emit(Instruction::Emit);
                self.bytecode.emit_u16(signal.raw() as u16);
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
            // Beyond bit 63, the mask can't represent the local — be conservative
            // and treat it as a cell local (use env/StoreUpvalue).
            if local_offset < 64 && (func.capture_locals_mask & (1 << local_offset)) == 0 {
                // Non-cell local: use stack slot
                Some(index - func.num_captures)
            } else {
                None // cell local (or beyond mask range): use env
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
            LirConst::String(s) => {
                let idx = self.bytecode.add_constant(Value::string(s.clone()));
                self.bytecode.emit(Instruction::LoadConst);
                self.bytecode.emit_u16(idx);
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
