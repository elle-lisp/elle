//! LIR to Bytecode emission
//!
//! Converts register-based LIR to stack-based bytecode.
//! Uses a simple stack simulation to track register values.

use super::types::*;
use crate::compiler::bytecode::{Bytecode, Instruction};
use crate::value::{Arity, Closure, Value};
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
    /// Pending handler jumps that need patching (instruction position, target label)
    /// These use absolute offsets instead of relative offsets
    pending_handler_jumps: Vec<(usize, Label)>,
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
}

impl Emitter {
    pub fn new() -> Self {
        Emitter {
            bytecode: Bytecode::new(),
            label_offsets: HashMap::new(),
            pending_jumps: Vec::new(),
            pending_handler_jumps: Vec::new(),
            stack: Vec::new(),
            reg_to_stack: HashMap::new(),
            symbol_names: HashMap::new(),
            yield_stack_state: HashMap::new(),
        }
    }

    /// Create an emitter with symbol name mappings for cross-thread portability.
    pub fn new_with_symbols(symbol_names: HashMap<u32, String>) -> Self {
        Emitter {
            bytecode: Bytecode::new(),
            label_offsets: HashMap::new(),
            pending_jumps: Vec::new(),
            pending_handler_jumps: Vec::new(),
            stack: Vec::new(),
            reg_to_stack: HashMap::new(),
            symbol_names,
            yield_stack_state: HashMap::new(),
        }
    }

    /// Emit bytecode from a LIR function
    pub fn emit(&mut self, func: &LirFunction) -> Bytecode {
        let mut bytecode = Bytecode::new();
        // Copy symbol names to the new bytecode for cross-thread portability
        bytecode.symbol_names = self.symbol_names.clone();
        self.bytecode = bytecode;
        self.label_offsets.clear();
        self.pending_jumps.clear();
        self.pending_handler_jumps.clear();
        self.stack.clear();
        self.reg_to_stack.clear();
        self.yield_stack_state.clear();

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

        // Patch handler jumps (absolute offsets)
        for (pos, label) in &self.pending_handler_jumps {
            if let Some(&target) = self.label_offsets.get(label) {
                let offset = target as i16;
                self.bytecode.patch_jump(*pos, offset);
            }
        }

        std::mem::take(&mut self.bytecode)
    }

    /// Emit bytecode from a nested LIR function (for closures)
    fn emit_nested_function(&mut self, func: &LirFunction) -> Bytecode {
        // Save current state
        let saved_bytecode = std::mem::take(&mut self.bytecode);
        let saved_label_offsets = std::mem::take(&mut self.label_offsets);
        let saved_pending_jumps = std::mem::take(&mut self.pending_jumps);
        let saved_pending_handler_jumps = std::mem::take(&mut self.pending_handler_jumps);
        let saved_stack = std::mem::take(&mut self.stack);
        let saved_reg_to_stack = std::mem::take(&mut self.reg_to_stack);
        let saved_yield_stack_state = std::mem::take(&mut self.yield_stack_state);

        // Emit the nested function
        let result = self.emit(func);

        // Restore state
        self.bytecode = saved_bytecode;
        self.label_offsets = saved_label_offsets;
        self.pending_jumps = saved_pending_jumps;
        self.pending_handler_jumps = saved_pending_handler_jumps;
        self.stack = saved_stack;
        self.reg_to_stack = saved_reg_to_stack;
        self.yield_stack_state = saved_yield_stack_state;

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
                self.bytecode.emit_byte(0); // depth 0 for now
                self.bytecode.emit_byte(*slot as u8);
                self.push_reg(*dst);
            }

            LirInstr::StoreLocal { slot, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::StoreLocal);
                self.bytecode.emit_byte(0); // depth 0
                self.bytecode.emit_byte(*slot as u8);
                // StoreLocal pops the value, stores it, and pushes it back
                // So the stack simulation stays the same (value is still on top)
            }

            LirInstr::LoadCapture { dst, index } => {
                self.bytecode.emit(Instruction::LoadUpvalue);
                self.bytecode.emit_byte(0); // depth (currently unused)
                self.bytecode.emit_byte(*index as u8);
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
                self.bytecode.emit(Instruction::StoreUpvalue);
                self.bytecode.emit_byte(0); // depth (currently unused)
                self.bytecode.emit_byte(*index as u8);
                // StoreCapture: VM pops value, stores in cell, pushes it back.
                // Net stack effect is 0, so don't adjust simulated stack.
            }

            LirInstr::LoadGlobal { dst, sym } => {
                // Add symbol to constants with name for cross-thread portability
                let name = self.symbol_names.get(&sym.0).cloned().unwrap_or_default();
                let const_idx = self.bytecode.add_symbol(sym.0, &name);
                // LoadGlobal reads the symbol index directly from bytecode
                self.bytecode.emit(Instruction::LoadGlobal);
                self.bytecode.emit_u16(const_idx);
                self.push_reg(*dst);
            }

            LirInstr::StoreGlobal { sym, src } => {
                self.ensure_on_top(*src);
                // Add symbol to constants with name for cross-thread portability
                let name = self.symbol_names.get(&sym.0).cloned().unwrap_or_default();
                let const_idx = self.bytecode.add_symbol(sym.0, &name);
                // StoreGlobal reads the symbol index directly from bytecode
                // Stack should have the value on top
                self.bytecode.emit(Instruction::StoreGlobal);
                self.bytecode.emit_u16(const_idx);
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
                let nested_bytecode = self.emit_nested_function(func);

                // Create closure template
                let closure = Closure {
                    bytecode: Rc::new(nested_bytecode.instructions),
                    arity: Arity::Exact(func.arity as usize),
                    env: Rc::new(vec![]), // Empty - captures added at runtime
                    num_locals: func.num_locals as usize,
                    num_captures: captures.len(),
                    constants: Rc::new(nested_bytecode.constants),
                    effect: func.effect.clone(),
                    cell_params_mask: func.cell_params_mask,
                    symbol_names: Rc::new(nested_bytecode.symbol_names),
                    location_map: Rc::new(nested_bytecode.location_map),
                    jit_code: None,
                    lir_function: Some(Rc::new(func.as_ref().clone())),
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
                // Pop func and args, push result
                self.pop(); // func
                for _ in args {
                    self.pop();
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

            LirInstr::MakeVector { dst, elements } => {
                for elem in elements {
                    self.ensure_on_top(*elem);
                }
                self.bytecode.emit(Instruction::MakeVector);
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

            LirInstr::BinOp { dst, op, lhs, rhs } => {
                self.ensure_on_top(*lhs);
                self.ensure_on_top(*rhs);
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
                self.ensure_on_top(*lhs);
                self.ensure_on_top(*rhs);
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
                        // Emit: push 0, swap, sub
                        // Actually, we need to emit 0 - src
                        // Stack has src on top, we need 0 on top then sub
                        let zero_idx = self.bytecode.add_constant(Value::int(0));
                        self.bytecode.emit(Instruction::LoadConst);
                        self.bytecode.emit_u16(zero_idx);
                        self.bytecode.emit(Instruction::Sub);
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

            LirInstr::MakeCell { dst, value } => {
                self.ensure_on_top(*value);
                self.bytecode.emit(Instruction::MakeCell);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::LoadCell { dst, cell } => {
                self.ensure_on_top(*cell);
                self.bytecode.emit(Instruction::UnwrapCell);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::StoreCell { cell, value } => {
                self.ensure_on_top(*cell);
                self.ensure_on_top(*value);
                self.bytecode.emit(Instruction::UpdateCell);
                self.pop();
                self.pop();
            }

            LirInstr::Move { dst, src } => {
                // Move is a logical copy - dst now refers to the same value as src.
                // We don't emit any bytecode; we just update the register tracking.
                // This works because in LIR, Move is used to copy a value to a result
                // register, and the source is typically not used again.
                //
                // If src is tracked, dst now refers to the same stack position.
                // If src is not tracked (e.g., after control flow merge), we assume
                // the value is on top of the stack and track dst there.
                if let Some(&pos) = self.reg_to_stack.get(src) {
                    // dst now refers to the same stack position as src
                    self.reg_to_stack.insert(*dst, pos);
                    // Update the stack to show dst at this position
                    if pos < self.stack.len() {
                        self.stack[pos] = *dst;
                    }
                } else {
                    // src not tracked - assume value is on top of stack
                    // This can happen after control flow merges
                    if !self.stack.is_empty() {
                        let top = self.stack.len() - 1;
                        self.stack[top] = *dst;
                        self.reg_to_stack.insert(*dst, top);
                    }
                }
            }

            LirInstr::Dup { dst, src } => {
                // Duplicate the value - actually emit a Dup instruction.
                // This creates a new copy on the stack.
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::Dup);
                self.push_reg(*dst);
            }

            LirInstr::Pop { src } => {
                // Pop the value from the stack (discard it).
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::Pop);
                self.pop();
            }

            LirInstr::LoadResumeValue { dst } => {
                // The resume value is already on the operand stack
                // (pushed by the VM's resume_continuation).
                // The stack simulation already has the pre-yield state.
                // Just register the resume value.
                self.push_reg(*dst);
            }

            LirInstr::PushHandler { handler_label } => {
                self.bytecode.emit(Instruction::PushHandler);
                // Placeholder for handler offset - will need patching
                // Note: handler offset is absolute, not relative
                let pos = self.bytecode.current_pos();
                self.bytecode.emit_i16(0);
                self.bytecode.emit_i16(-1); // no finally
                self.pending_handler_jumps.push((pos, *handler_label));
            }

            LirInstr::PopHandler => {
                self.bytecode.emit(Instruction::PopHandler);
            }

            LirInstr::CheckException => {
                self.bytecode.emit(Instruction::CheckException);
            }

            LirInstr::MatchException { dst, exception_id } => {
                self.bytecode.emit(Instruction::MatchException);
                self.bytecode.emit_u16(*exception_id);
                // MatchException pushes a boolean result
                self.push_reg(*dst);
            }

            LirInstr::BindException { var_name } => {
                self.bytecode.emit(Instruction::BindException);
                let name = self
                    .symbol_names
                    .get(&var_name.0)
                    .cloned()
                    .unwrap_or_default();
                let const_idx = self.bytecode.add_symbol(var_name.0, &name);
                self.bytecode.emit_u16(const_idx);
            }

            LirInstr::LoadException { dst } => {
                self.bytecode.emit(Instruction::LoadException);
                self.push_reg(*dst);
            }

            LirInstr::ClearException => {
                self.bytecode.emit(Instruction::ClearException);
            }

            LirInstr::ReraiseException => {
                self.bytecode.emit(Instruction::ReraiseException);
            }

            LirInstr::Throw { value } => {
                self.ensure_on_top(*value);
                // Use existing exception mechanism
                // For now, just leave value on stack
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
            LirConst::Keyword(sym) => {
                let idx = self.bytecode.add_constant(Value::keyword(sym.0));
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
}

impl Default for Emitter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::Span;

    fn synthetic_span() -> Span {
        Span::synthetic()
    }

    #[test]
    fn test_emit_simple() {
        let mut emitter = Emitter::new();

        let mut func = LirFunction::new(0);
        let mut block = BasicBlock::new(Label(0));
        block.instructions.push(SpannedInstr::new(
            LirInstr::Const {
                dst: Reg(0),
                value: LirConst::Int(42),
            },
            synthetic_span(),
        ));
        block.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), synthetic_span());
        func.blocks.push(block);
        func.entry = Label(0);

        let bytecode = emitter.emit(&func);
        assert!(!bytecode.instructions.is_empty());
    }

    #[test]
    fn test_emit_branch() {
        let mut emitter = Emitter::new();

        let mut func = LirFunction::new(0);

        // Entry block
        let mut entry = BasicBlock::new(Label(0));
        entry.instructions.push(SpannedInstr::new(
            LirInstr::Const {
                dst: Reg(0),
                value: LirConst::Bool(true),
            },
            synthetic_span(),
        ));
        entry.terminator = SpannedTerminator::new(
            Terminator::Branch {
                cond: Reg(0),
                then_label: Label(1),
                else_label: Label(2),
            },
            synthetic_span(),
        );
        func.blocks.push(entry);

        // Then block
        let mut then_block = BasicBlock::new(Label(1));
        then_block.instructions.push(SpannedInstr::new(
            LirInstr::Const {
                dst: Reg(1),
                value: LirConst::Int(1),
            },
            synthetic_span(),
        ));
        then_block.terminator =
            SpannedTerminator::new(Terminator::Return(Reg(1)), synthetic_span());
        func.blocks.push(then_block);

        // Else block
        let mut else_block = BasicBlock::new(Label(2));
        else_block.instructions.push(SpannedInstr::new(
            LirInstr::Const {
                dst: Reg(2),
                value: LirConst::Int(2),
            },
            synthetic_span(),
        ));
        else_block.terminator =
            SpannedTerminator::new(Terminator::Return(Reg(2)), synthetic_span());
        func.blocks.push(else_block);

        func.entry = Label(0);

        let bytecode = emitter.emit(&func);
        assert!(!bytecode.instructions.is_empty());
        // Should have Jump instructions for control flow
        assert!(bytecode
            .instructions
            .iter()
            .any(|&b| b == Instruction::Jump as u8 || b == Instruction::JumpIfFalse as u8));
    }
}
