//! LIR to Bytecode emission
//!
//! Converts register-based LIR to stack-based bytecode.
//! Uses a simple stack simulation to track register values.

use super::types::*;
use crate::compiler::bytecode::{Bytecode, Instruction};
use crate::value::{Closure, Value};
use std::collections::HashMap;
use std::rc::Rc;

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
        }
    }

    /// Emit bytecode from a LIR function
    pub fn emit(
        &mut self,
        func: &LirFunction,
    ) -> (Bytecode, Vec<YieldPointInfo>, Vec<CallSiteInfo>) {
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

        // First pass: record label offsets (simplified - emit all blocks in order)
        // Second pass handled inline since we emit sequentially

        // Sort blocks by label for deterministic output
        let mut blocks: Vec<_> = func.blocks.iter().collect();
        blocks.sort_by_key(|b| b.label.0);

        for block in &blocks {
            self.label_offsets
                .insert(block.label, self.bytecode.current_pos());
            self.emit_block(block, func);
        }

        // Patch jumps (relative offsets)
        for (pos, label) in &self.pending_jumps {
            if let Some(&target) = self.label_offsets.get(label) {
                let offset = (target as i32 - *pos as i32 - 2) as i16;
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
    fn emit_nested_function(
        &mut self,
        func: &LirFunction,
    ) -> (Bytecode, Vec<YieldPointInfo>, Vec<CallSiteInfo>) {
        // Save current state
        let saved_bytecode = std::mem::take(&mut self.bytecode);
        let saved_label_offsets = std::mem::take(&mut self.label_offsets);
        let saved_pending_jumps = std::mem::take(&mut self.pending_jumps);
        let saved_stack = std::mem::take(&mut self.stack);
        let saved_reg_to_stack = std::mem::take(&mut self.reg_to_stack);
        let saved_yield_stack_state = std::mem::take(&mut self.yield_stack_state);
        let saved_yield_points = std::mem::take(&mut self.yield_points);
        let saved_call_sites = std::mem::take(&mut self.call_sites);
        let saved_may_suspend = self.current_func_may_suspend;

        // Emit the nested function
        let result = self.emit(func);

        // Restore state
        self.bytecode = saved_bytecode;
        self.label_offsets = saved_label_offsets;
        self.pending_jumps = saved_pending_jumps;
        self.stack = saved_stack;
        self.reg_to_stack = saved_reg_to_stack;
        self.yield_stack_state = saved_yield_stack_state;
        self.yield_points = saved_yield_points;
        self.call_sites = saved_call_sites;
        self.current_func_may_suspend = saved_may_suspend;

        result
    }

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
                    self.bytecode.emit_byte(*index as u8);
                }
                self.push_reg(*dst);
            }

            LirInstr::LoadCaptureRaw { dst, index } => {
                // Load without unwrapping cells - used for forwarding captures
                self.bytecode.emit(Instruction::LoadUpvalueRaw);
                self.bytecode.emit_byte(0); // depth (currently unused)
                self.bytecode.emit_byte(*index as u8);
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
                    self.bytecode.emit_byte(*index as u8);
                }
                // Both StoreLocal and StoreUpvalue pop-then-push-back.
                // Auto-pop: consume the pushed-back value.
                self.bytecode.emit(Instruction::Pop);
                self.pop();
            }

            LirInstr::MakeClosure {
                dst,
                func,
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

                // Recursively emit the nested function
                let (nested_bytecode, nested_yield_points, nested_call_sites) =
                    self.emit_nested_function(func);
                let mut nested_lir = func.as_ref().clone();
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
                    lbox_params_mask: func.lbox_params_mask,
                    lbox_locals_mask: func.lbox_locals_mask,
                    symbol_names: Rc::new(nested_bytecode.symbol_names),
                    location_map: Rc::new(nested_bytecode.location_map),
                    jit_code: None,
                    lir_function: Some(Rc::new(nested_lir)),
                    doc: func.doc,
                    syntax: func.syntax.clone(),
                    vararg_kind: func.vararg_kind.clone(),
                    name: func.name.clone().map(|s| Rc::from(s.as_str())),
                };
                let closure = Closure {
                    template: Rc::new(template),
                    env: Rc::new(vec![]),
                };

                // Add closure template to constants
                let const_idx = self.bytecode.add_constant(Value::closure(closure));

                // Emit MakeClosure instruction
                self.bytecode.emit(Instruction::MakeClosure);
                self.bytecode.emit_u16(const_idx);
                self.bytecode.emit_byte(captures.len() as u8);

                // Pop captures, push closure
                for _ in captures {
                    self.pop();
                }
                self.push_reg(*dst);
            }

            LirInstr::Call { dst, func, args } => {
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
                self.bytecode.emit_byte(args.len() as u8);
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
                self.bytecode.emit_byte(args.len() as u8);
            }

            LirInstr::Cons { dst, head, tail } => {
                self.ensure_on_top(*tail);
                self.ensure_on_top(*head);
                self.bytecode.emit(Instruction::Cons);
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

            LirInstr::Car { dst, pair } => {
                self.ensure_on_top(*pair);
                self.bytecode.emit(Instruction::Car);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::Cdr { dst, pair } => {
                self.ensure_on_top(*pair);
                self.bytecode.emit(Instruction::Cdr);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::CarDestructure { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::CarDestructure);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::CdrDestructure { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::CdrDestructure);
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

            LirInstr::TableGetOrNil { dst, src, key } => {
                self.ensure_on_top(*src);
                let key_value = match key {
                    LirConst::Keyword(name) => Value::keyword(name),
                    LirConst::String(s) => Value::string(s.clone()),
                    LirConst::Int(n) => Value::int(*n),
                    LirConst::Symbol(sym) => Value::symbol(sym.0),
                    LirConst::Bool(b) => Value::bool(*b),
                    LirConst::Nil => Value::NIL,
                    _ => panic!("TableGetOrNil: unsupported key type"),
                };
                let const_idx = self.bytecode.add_constant(key_value);
                self.bytecode.emit(Instruction::TableGetOrNil);
                self.bytecode.emit_u16(const_idx);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::TableGetDestructure { dst, src, key } => {
                self.ensure_on_top(*src);
                let key_value = match key {
                    LirConst::Keyword(name) => Value::keyword(name),
                    LirConst::String(s) => Value::string(s.clone()),
                    LirConst::Int(n) => Value::int(*n),
                    LirConst::Symbol(sym) => Value::symbol(sym.0),
                    LirConst::Bool(b) => Value::bool(*b),
                    LirConst::Nil => Value::NIL,
                    _ => panic!("TableGetDestructure: unsupported key type"),
                };
                let const_idx = self.bytecode.add_constant(key_value);
                self.bytecode.emit(Instruction::TableGetDestructure);
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
            LirInstr::CarOrNil { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::CarOrNil);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::CdrOrNil { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::CdrOrNil);
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

            LirInstr::IsTable { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsTable);
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

            LirInstr::MakeLBox { dst, value } => {
                self.ensure_on_top(*value);
                self.bytecode.emit(Instruction::MakeLBox);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::LoadLBox { dst, cell } => {
                self.ensure_on_top(*cell);
                self.bytecode.emit(Instruction::UnlBox);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::StoreLBox { cell, value } => {
                self.ensure_on_top(*cell);
                self.ensure_on_top(*value);
                self.bytecode.emit(Instruction::UpdateLBox);
                // UpdateLBox pops value, pops cell, pushes value back.
                // Unlike other stores, UpdateLBox pushes the value back.
                // We do NOT auto-pop here because lower_set needs the value.
                self.pop(); // value (consumed by UpdateLBox, re-pushed)
                self.pop(); // cell (consumed by UpdateLBox)
                            // Value is now on the stack (pushed back by UpdateLBox).
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
                self.pop(); // args
                self.pop(); // func
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

            LirInstr::CheckSignalBound { src, allowed_bits } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::CheckSignalBound);
                // Emit u32 as two u16s (low half first, then high half)
                self.bytecode.emit_u16(*allowed_bits as u16);
                self.bytecode.emit_u16((*allowed_bits >> 16) as u16);
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
                // Save stack state for the target block, but only if it hasn't
                // been processed yet. This is used for control flow merges
                // (e.g., if/and/or) where multiple blocks jump to the same target.
                // If the target block has already been processed (because blocks
                // are sorted by label), we don't overwrite the saved state.
                if !self.label_offsets.contains_key(label) {
                    self.yield_stack_state
                        .insert(*label, (self.stack.clone(), self.reg_to_stack.clone()));
                }

                self.bytecode.emit(Instruction::Jump);
                let pos = self.bytecode.current_pos();
                self.bytecode.emit_i16(0); // placeholder
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
                self.bytecode.emit_i16(0); // placeholder
                self.pending_jumps.push((else_pos, *else_label));

                // Fall through or jump to then_label
                self.bytecode.emit(Instruction::Jump);
                let then_pos = self.bytecode.current_pos();
                self.bytecode.emit_i16(0); // placeholder
                self.pending_jumps.push((then_pos, *then_label));
            }

            Terminator::Yield {
                value,
                resume_label,
            } => {
                self.ensure_on_top(*value);
                self.bytecode.emit(Instruction::Yield);
                // Pop the yielded value from the simulated stack
                self.pop();

                // The resume IP is the current bytecode position (right after
                // the Yield opcode byte). This is what the interpreter stores
                // in SuspendedFrame.ip.
                let resume_ip = self.bytecode.current_pos();

                // Record yield point metadata for JIT.
                // num_locals is needed so the JIT can spill local variable
                // values into the SuspendedFrame stack, matching the
                // interpreter's layout: [locals..., operands...].
                self.yield_points.push(YieldPointInfo {
                    resume_ip,
                    stack_regs: self.stack.clone(),
                    num_locals: self.current_func_num_locals,
                });

                // Save stack state for the resume block.
                // The resume block will start with this stack state,
                // plus the resume value on top (added by LoadResumeValue).
                self.yield_stack_state.insert(
                    *resume_label,
                    (self.stack.clone(), self.reg_to_stack.clone()),
                );

                // Emit a jump to the resume block.
                // This is necessary because blocks are sorted by label number,
                // so the resume block may not be immediately after the yield.
                // When the coroutine is resumed, the VM continues from the IP
                // after the yield, which is this jump instruction.
                self.bytecode.emit(Instruction::Jump);
                let pos = self.bytecode.current_pos();
                self.bytecode.emit_i16(0); // placeholder
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
            if local_offset < 64 && (func.lbox_locals_mask & (1 << local_offset)) == 0 {
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
        }
    }

    // Stack management helpers

    fn push_reg(&mut self, reg: Reg) {
        let pos = self.stack.len();
        self.stack.push(reg);
        self.reg_to_stack.insert(reg, pos);
    }

    fn pop(&mut self) {
        if let Some(reg) = self.stack.pop() {
            self.reg_to_stack.remove(&reg);
        }
    }

    fn ensure_on_top(&mut self, reg: Reg) {
        if let Some(&pos) = self.reg_to_stack.get(&reg) {
            let stack_top = self.stack.len().saturating_sub(1);
            if pos != stack_top {
                // Value is not on top - duplicate it to the top using DupN
                let offset = stack_top - pos;
                self.bytecode.emit(Instruction::DupN);
                self.bytecode.emit_byte(offset as u8);
                // Track the duplicated value
                self.stack.push(reg);
                // Update reg_to_stack to point to the new top position
                self.reg_to_stack.insert(reg, self.stack.len() - 1);
            }
            // else: already on top, nothing to do
        } else {
            // Register not tracked - this can happen after control flow merges
            // where the stack state is uncertain. Assume the value is already
            // on top of the stack (this is the case for if/and/or expressions
            // where each branch leaves its result on top).
            // This is a fallback for compatibility; ideally the LIR would
            // use phi nodes or a single result register for control flow.
        }
    }

    /// Ensure two registers are the top two stack elements (lhs below rhs).
    ///
    /// Binary operations (BinOp, Compare) consume both operands. Unlike
    /// `ensure_on_top` which duplicates via DupN (leaving originals as
    /// orphans), this checks whether the operands are already in position
    /// and only falls back to DupN when they aren't.
    fn ensure_binary_on_top(&mut self, lhs: Reg, rhs: Reg) {
        let stack_len = self.stack.len();
        if stack_len >= 2 {
            let lhs_pos = self.reg_to_stack.get(&lhs).copied();
            let rhs_pos = self.reg_to_stack.get(&rhs).copied();
            if lhs_pos == Some(stack_len - 2) && rhs_pos == Some(stack_len - 1) {
                // Already in place — nothing to emit
                return;
            }
        }
        // Fall back to ensure_on_top for each operand.
        // This handles the uncommon case (e.g., after control flow merges).
        // NOTE: The DupN fallback leaves original values as orphans on the
        // actual VM stack. This is a pre-existing limitation of ensure_on_top.
        // From intrinsic lowering, operands are always freshly lowered and
        // already in position, so this path is not reached in practice.
        self.ensure_on_top(lhs);
        self.ensure_on_top(rhs);
    }
}

impl Default for Emitter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
