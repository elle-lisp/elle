//! LIR to Cranelift IR translation
//!
//! This module contains `FunctionTranslator`, which translates individual
//! LIR instructions and terminators to Cranelift IR.

use std::collections::HashMap;

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types::I64;
use cranelift_codegen::ir::{InstBuilder, MemFlags};
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Module};

use crate::lir::{BinOp, CmpOp, Label, LirConst, LirInstr, Terminator, UnaryOp};
use crate::value::repr::{PAYLOAD_MASK, TAG_EMPTY_LIST, TAG_FALSE, TAG_INT, TAG_NIL, TAG_TRUE};

use super::compiler::RuntimeHelpers;
use super::JitError;

/// Helper to create a Variable from a register/slot index
#[inline]
fn var(n: u32) -> Variable {
    Variable::from_u32(n)
}

/// Translator for a single function
pub(crate) struct FunctionTranslator<'a> {
    module: &'a mut JITModule,
    helpers: &'a RuntimeHelpers,
    pub(crate) lir: &'a crate::lir::LirFunction,
    pub(crate) env_ptr: Option<cranelift_codegen::ir::Value>,
    pub(crate) args_ptr: Option<cranelift_codegen::ir::Value>,
    pub(crate) vm_ptr: Option<cranelift_codegen::ir::Value>,
}

impl<'a> FunctionTranslator<'a> {
    pub(crate) fn new(
        module: &'a mut JITModule,
        helpers: &'a RuntimeHelpers,
        lir: &'a crate::lir::LirFunction,
    ) -> Self {
        FunctionTranslator {
            module,
            helpers,
            lir,
            env_ptr: None,
            args_ptr: None,
            vm_ptr: None,
        }
    }

    /// Translate a single LIR instruction
    /// Returns true if the instruction emitted a terminator (e.g., TailCall)
    pub(crate) fn translate_instr(
        &mut self,
        builder: &mut FunctionBuilder,
        instr: &LirInstr,
        _block_map: &HashMap<Label, cranelift_codegen::ir::Block>,
    ) -> Result<bool, JitError> {
        match instr {
            LirInstr::Const { dst, value } => {
                let val = self.translate_const(builder, value);
                builder.def_var(var(dst.0), val);
            }

            LirInstr::ValueConst { dst, value } => {
                let bits = value.to_bits();
                let val = builder.ins().iconst(I64, bits as i64);
                builder.def_var(var(dst.0), val);
            }

            LirInstr::Move { dst, src } => {
                let val = builder.use_var(var(src.0));
                builder.def_var(var(dst.0), val);
            }

            LirInstr::Dup { dst, src } => {
                let val = builder.use_var(var(src.0));
                builder.def_var(var(dst.0), val);
            }

            LirInstr::LoadLocal { dst, slot } => {
                // In LIR, locals are just registers
                let val = builder.use_var(var(*slot as u32));
                builder.def_var(var(dst.0), val);
            }

            LirInstr::StoreLocal { slot, src } => {
                let val = builder.use_var(var(src.0));
                builder.def_var(var(*slot as u32), val);
            }

            LirInstr::LoadCapture { dst, index } => {
                // The LIR uses indices where:
                // - [0, num_captures) are captures (from env)
                // - [num_captures, num_captures + arity) are parameters (from args)
                let num_captures = self.lir.num_captures;
                if *index < num_captures {
                    // Load from closure environment (captures)
                    let env_ptr = self.env_ptr.ok_or_else(|| {
                        JitError::InvalidLir("LoadCapture without env pointer".to_string())
                    })?;
                    let offset = (*index as i32) * 8;
                    let addr = builder.ins().iadd_imm(env_ptr, offset as i64);
                    let val = builder.ins().load(I64, MemFlags::trusted(), addr, 0);
                    builder.def_var(var(dst.0), val);
                } else {
                    // Load from arguments array (parameters)
                    let args_ptr = self.args_ptr.ok_or_else(|| {
                        JitError::InvalidLir("LoadCapture without args pointer".to_string())
                    })?;
                    let param_index = *index - num_captures;
                    let offset = (param_index as i32) * 8;
                    let addr = builder.ins().iadd_imm(args_ptr, offset as i64);
                    let val = builder.ins().load(I64, MemFlags::trusted(), addr, 0);
                    builder.def_var(var(dst.0), val);
                }
            }

            LirInstr::LoadCaptureRaw { dst, index } => {
                // Same as LoadCapture for now (Phase 1 doesn't handle cells specially)
                let num_captures = self.lir.num_captures;
                if *index < num_captures {
                    let env_ptr = self.env_ptr.ok_or_else(|| {
                        JitError::InvalidLir("LoadCaptureRaw without env pointer".to_string())
                    })?;
                    let offset = (*index as i32) * 8;
                    let addr = builder.ins().iadd_imm(env_ptr, offset as i64);
                    let val = builder.ins().load(I64, MemFlags::trusted(), addr, 0);
                    builder.def_var(var(dst.0), val);
                } else {
                    let args_ptr = self.args_ptr.ok_or_else(|| {
                        JitError::InvalidLir("LoadCaptureRaw without args pointer".to_string())
                    })?;
                    let param_index = *index - num_captures;
                    let offset = (param_index as i32) * 8;
                    let addr = builder.ins().iadd_imm(args_ptr, offset as i64);
                    let val = builder.ins().load(I64, MemFlags::trusted(), addr, 0);
                    builder.def_var(var(dst.0), val);
                }
            }

            LirInstr::BinOp { dst, op, lhs, rhs } => {
                let lhs_val = builder.use_var(var(lhs.0));
                let rhs_val = builder.use_var(var(rhs.0));
                let result = self.call_binary_helper(builder, *op, lhs_val, rhs_val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::UnaryOp { dst, op, src } => {
                let src_val = builder.use_var(var(src.0));
                let result = self.call_unary_helper(builder, *op, src_val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::Compare { dst, op, lhs, rhs } => {
                let lhs_val = builder.use_var(var(lhs.0));
                let rhs_val = builder.use_var(var(rhs.0));
                let result = self.call_compare_helper(builder, *op, lhs_val, rhs_val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::IsNil { dst, src } => {
                let src_val = builder.use_var(var(src.0));
                let result = self.call_helper_unary(builder, self.helpers.is_nil, src_val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::IsPair { dst, src } => {
                let src_val = builder.use_var(var(src.0));
                let result = self.call_helper_unary(builder, self.helpers.is_pair, src_val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::Pop { src: _ } => {
                // No-op in JIT (stack operations are implicit)
            }

            // === Phase 3: Data structures ===
            LirInstr::Cons { dst, head, tail } => {
                let head_val = builder.use_var(var(head.0));
                let tail_val = builder.use_var(var(tail.0));
                let result =
                    self.call_helper_binary(builder, self.helpers.cons, head_val, tail_val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::Car { dst, pair } => {
                let pair_val = builder.use_var(var(pair.0));
                let result = self.call_helper_unary(builder, self.helpers.car, pair_val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::Cdr { dst, pair } => {
                let pair_val = builder.use_var(var(pair.0));
                let result = self.call_helper_unary(builder, self.helpers.cdr, pair_val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::MakeVector { dst, elements } => {
                // Allocate stack space for elements
                if elements.is_empty() {
                    // Empty vector - pass null pointer and 0 count
                    let null_ptr = builder.ins().iconst(I64, 0);
                    let count = builder.ins().iconst(I64, 0);
                    let result = self.call_helper_binary(
                        builder,
                        self.helpers.make_vector,
                        null_ptr,
                        count,
                    )?;
                    builder.def_var(var(dst.0), result);
                } else {
                    // Create stack slot for elements
                    let slot =
                        builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                            (elements.len() * 8) as u32,
                            0,
                        ));
                    // Store each element
                    for (i, elem_reg) in elements.iter().enumerate() {
                        let elem_val = builder.use_var(var(elem_reg.0));
                        builder.ins().stack_store(elem_val, slot, (i * 8) as i32);
                    }
                    let elements_addr = builder.ins().stack_addr(I64, slot, 0);
                    let count = builder.ins().iconst(I64, elements.len() as i64);
                    let result = self.call_helper_binary(
                        builder,
                        self.helpers.make_vector,
                        elements_addr,
                        count,
                    )?;
                    builder.def_var(var(dst.0), result);
                }
            }

            // === Phase 3: Cell operations ===
            LirInstr::MakeCell { dst, value } => {
                let val = builder.use_var(var(value.0));
                let result = self.call_helper_unary(builder, self.helpers.make_cell, val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::LoadCell { dst, cell } => {
                let cell_val = builder.use_var(var(cell.0));
                let result = self.call_helper_unary(builder, self.helpers.load_cell, cell_val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::StoreCell { cell, value } => {
                let cell_val = builder.use_var(var(cell.0));
                let val = builder.use_var(var(value.0));
                let _result =
                    self.call_helper_binary(builder, self.helpers.store_cell, cell_val, val)?;
                // Result is NIL, we don't need to store it
            }

            LirInstr::StoreCapture { index, src } => {
                let env_ptr = self.env_ptr.ok_or_else(|| {
                    JitError::InvalidLir("StoreCapture without env pointer".to_string())
                })?;
                let idx_val = builder.ins().iconst(I64, *index as i64);
                let val = builder.use_var(var(src.0));
                let _result = self.call_helper_ternary(
                    builder,
                    self.helpers.store_capture,
                    env_ptr,
                    idx_val,
                    val,
                )?;
            }

            // === Phase 3: Global variables ===
            LirInstr::LoadGlobal { dst, sym } => {
                let sym_bits = builder.ins().iconst(I64, sym.0 as i64);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("LoadGlobal without vm pointer".to_string())
                })?;
                let result =
                    self.call_helper_binary(builder, self.helpers.load_global, sym_bits, vm)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::StoreGlobal { sym, src } => {
                let sym_bits = builder.ins().iconst(I64, sym.0 as i64);
                let val = builder.use_var(var(src.0));
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("StoreGlobal without vm pointer".to_string())
                })?;
                let _result = self.call_helper_ternary(
                    builder,
                    self.helpers.store_global,
                    sym_bits,
                    val,
                    vm,
                )?;
            }

            // === Phase 3: Function calls ===
            LirInstr::Call { dst, func, args } => {
                let func_val = builder.use_var(var(func.0));
                let vm = self
                    .vm_ptr
                    .ok_or_else(|| JitError::InvalidLir("Call without vm pointer".to_string()))?;

                if args.is_empty() {
                    // No args - pass null pointer
                    let null_ptr = builder.ins().iconst(I64, 0);
                    let nargs = builder.ins().iconst(I64, 0);
                    let result = self.call_helper_call(builder, func_val, null_ptr, nargs, vm)?;
                    builder.def_var(var(dst.0), result);
                } else {
                    // Create stack slot for args
                    let slot =
                        builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                            (args.len() * 8) as u32,
                            0,
                        ));
                    // Store each arg
                    for (i, arg_reg) in args.iter().enumerate() {
                        let arg_val = builder.use_var(var(arg_reg.0));
                        builder.ins().stack_store(arg_val, slot, (i * 8) as i32);
                    }
                    let args_addr = builder.ins().stack_addr(I64, slot, 0);
                    let nargs = builder.ins().iconst(I64, args.len() as i64);
                    let result = self.call_helper_call(builder, func_val, args_addr, nargs, vm)?;
                    builder.def_var(var(dst.0), result);
                }
            }

            LirInstr::TailCall { func, args } => {
                // For now, treat TailCall as a regular call that returns immediately
                // True TCO in JIT would require more complex handling
                let func_val = builder.use_var(var(func.0));
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("TailCall without vm pointer".to_string())
                })?;

                let result = if args.is_empty() {
                    let null_ptr = builder.ins().iconst(I64, 0);
                    let nargs = builder.ins().iconst(I64, 0);
                    self.call_helper_call(builder, func_val, null_ptr, nargs, vm)?
                } else {
                    let slot =
                        builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                            (args.len() * 8) as u32,
                            0,
                        ));
                    for (i, arg_reg) in args.iter().enumerate() {
                        let arg_val = builder.use_var(var(arg_reg.0));
                        builder.ins().stack_store(arg_val, slot, (i * 8) as i32);
                    }
                    let args_addr = builder.ins().stack_addr(I64, slot, 0);
                    let nargs = builder.ins().iconst(I64, args.len() as i64);
                    self.call_helper_call(builder, func_val, args_addr, nargs, vm)?
                };
                // Return the result immediately
                builder.ins().return_(&[result]);
                return Ok(true); // Block is terminated
            }

            // === Still unsupported (Phase 4+) ===
            LirInstr::MakeClosure { .. } => {
                return Err(JitError::UnsupportedInstruction("MakeClosure".to_string()));
            }
            LirInstr::LoadResumeValue { .. } => {
                return Err(JitError::UnsupportedInstruction(
                    "LoadResumeValue".to_string(),
                ));
            }
            LirInstr::PushHandler { .. } => {
                return Err(JitError::UnsupportedInstruction("PushHandler".to_string()));
            }
            LirInstr::PopHandler => {
                return Err(JitError::UnsupportedInstruction("PopHandler".to_string()));
            }
            LirInstr::CheckException => {
                return Err(JitError::UnsupportedInstruction(
                    "CheckException".to_string(),
                ));
            }
            LirInstr::MatchException { .. } => {
                return Err(JitError::UnsupportedInstruction(
                    "MatchException".to_string(),
                ));
            }
            LirInstr::BindException { .. } => {
                return Err(JitError::UnsupportedInstruction(
                    "BindException".to_string(),
                ));
            }
            LirInstr::LoadException { .. } => {
                return Err(JitError::UnsupportedInstruction(
                    "LoadException".to_string(),
                ));
            }
            LirInstr::ClearException => {
                return Err(JitError::UnsupportedInstruction(
                    "ClearException".to_string(),
                ));
            }
            LirInstr::ReraiseException => {
                return Err(JitError::UnsupportedInstruction(
                    "ReraiseException".to_string(),
                ));
            }
            LirInstr::Throw { .. } => {
                return Err(JitError::UnsupportedInstruction("Throw".to_string()));
            }
            LirInstr::JumpIfFalseInline { .. } => {
                // These are handled by the emitter, not present in final LIR
                return Err(JitError::UnsupportedInstruction(
                    "JumpIfFalseInline".to_string(),
                ));
            }
            LirInstr::JumpInline { .. } => {
                return Err(JitError::UnsupportedInstruction("JumpInline".to_string()));
            }
            LirInstr::LabelMarker { .. } => {
                // No-op marker
            }
        }
        Ok(false)
    }

    /// Translate a terminator
    pub(crate) fn translate_terminator(
        &mut self,
        builder: &mut FunctionBuilder,
        term: &Terminator,
        block_map: &HashMap<Label, cranelift_codegen::ir::Block>,
    ) -> Result<(), JitError> {
        match term {
            Terminator::Return(reg) => {
                let val = builder.use_var(var(reg.0));
                builder.ins().return_(&[val]);
            }

            Terminator::Jump(label) => {
                let target = block_map.get(label).ok_or_else(|| {
                    JitError::InvalidLir(format!("Unknown jump target: {:?}", label))
                })?;
                builder.ins().jump(*target, &[]);
            }

            Terminator::Branch {
                cond,
                then_label,
                else_label,
            } => {
                let cond_val = builder.use_var(var(cond.0));
                let then_block = block_map.get(then_label).ok_or_else(|| {
                    JitError::InvalidLir(format!("Unknown then target: {:?}", then_label))
                })?;
                let else_block = block_map.get(else_label).ok_or_else(|| {
                    JitError::InvalidLir(format!("Unknown else target: {:?}", else_label))
                })?;

                // Check truthiness: value != NIL && value != FALSE
                let nil = builder.ins().iconst(I64, TAG_NIL as i64);
                let false_val = builder.ins().iconst(I64, TAG_FALSE as i64);
                let not_nil = builder.ins().icmp(IntCC::NotEqual, cond_val, nil);
                let not_false = builder.ins().icmp(IntCC::NotEqual, cond_val, false_val);
                let is_truthy = builder.ins().band(not_nil, not_false);

                builder
                    .ins()
                    .brif(is_truthy, *then_block, &[], *else_block, &[]);
            }

            Terminator::Yield { .. } => {
                return Err(JitError::NotPure);
            }

            Terminator::Unreachable => {
                builder
                    .ins()
                    .trap(cranelift_codegen::ir::TrapCode::unwrap_user(0));
            }
        }
        Ok(())
    }

    /// Translate a constant to a Cranelift value
    fn translate_const(
        &self,
        builder: &mut FunctionBuilder,
        value: &LirConst,
    ) -> cranelift_codegen::ir::Value {
        let bits = match value {
            LirConst::Nil => TAG_NIL,
            LirConst::EmptyList => TAG_EMPTY_LIST,
            LirConst::Bool(true) => TAG_TRUE,
            LirConst::Bool(false) => TAG_FALSE,
            LirConst::Int(n) => TAG_INT | ((*n as u64) & PAYLOAD_MASK),
            LirConst::Float(f) => {
                // Use Value::float to handle NaN-boxing correctly
                crate::value::Value::float(*f).to_bits()
            }
            LirConst::String(_) => {
                // Strings require heap allocation - not supported in Phase 1
                // Return NIL as placeholder
                TAG_NIL
            }
            LirConst::Symbol(id) => crate::value::Value::symbol(id.0).to_bits(),
            LirConst::Keyword(id) => crate::value::Value::keyword(id.0).to_bits(),
        };
        builder.ins().iconst(I64, bits as i64)
    }

    /// Call a binary runtime helper
    fn call_binary_helper(
        &mut self,
        builder: &mut FunctionBuilder,
        op: BinOp,
        lhs: cranelift_codegen::ir::Value,
        rhs: cranelift_codegen::ir::Value,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let func_id = match op {
            BinOp::Add => self.helpers.add,
            BinOp::Sub => self.helpers.sub,
            BinOp::Mul => self.helpers.mul,
            BinOp::Div => self.helpers.div,
            BinOp::Rem => self.helpers.rem,
            BinOp::BitAnd => self.helpers.bit_and,
            BinOp::BitOr => self.helpers.bit_or,
            BinOp::BitXor => self.helpers.bit_xor,
            BinOp::Shl => self.helpers.shl,
            BinOp::Shr => self.helpers.shr,
        };
        self.call_helper_binary(builder, func_id, lhs, rhs)
    }

    /// Call a unary runtime helper
    fn call_unary_helper(
        &mut self,
        builder: &mut FunctionBuilder,
        op: UnaryOp,
        src: cranelift_codegen::ir::Value,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let func_id = match op {
            UnaryOp::Neg => self.helpers.neg,
            UnaryOp::Not => self.helpers.not,
            UnaryOp::BitNot => self.helpers.bit_not,
        };
        self.call_helper_unary(builder, func_id, src)
    }

    /// Call a comparison runtime helper
    fn call_compare_helper(
        &mut self,
        builder: &mut FunctionBuilder,
        op: CmpOp,
        lhs: cranelift_codegen::ir::Value,
        rhs: cranelift_codegen::ir::Value,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let func_id = match op {
            CmpOp::Eq => self.helpers.eq,
            CmpOp::Ne => self.helpers.ne,
            CmpOp::Lt => self.helpers.lt,
            CmpOp::Le => self.helpers.le,
            CmpOp::Gt => self.helpers.gt,
            CmpOp::Ge => self.helpers.ge,
        };
        self.call_helper_binary(builder, func_id, lhs, rhs)
    }

    /// Call a binary helper function
    fn call_helper_binary(
        &mut self,
        builder: &mut FunctionBuilder,
        func_id: FuncId,
        a: cranelift_codegen::ir::Value,
        b: cranelift_codegen::ir::Value,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let func_ref = self.module.declare_func_in_func(func_id, builder.func);
        let call = builder.ins().call(func_ref, &[a, b]);
        Ok(builder.inst_results(call)[0])
    }

    /// Call a unary helper function
    fn call_helper_unary(
        &mut self,
        builder: &mut FunctionBuilder,
        func_id: FuncId,
        a: cranelift_codegen::ir::Value,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let func_ref = self.module.declare_func_in_func(func_id, builder.func);
        let call = builder.ins().call(func_ref, &[a]);
        Ok(builder.inst_results(call)[0])
    }

    /// Call a ternary helper function
    fn call_helper_ternary(
        &mut self,
        builder: &mut FunctionBuilder,
        func_id: FuncId,
        a: cranelift_codegen::ir::Value,
        b: cranelift_codegen::ir::Value,
        c: cranelift_codegen::ir::Value,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let func_ref = self.module.declare_func_in_func(func_id, builder.func);
        let call = builder.ins().call(func_ref, &[a, b, c]);
        Ok(builder.inst_results(call)[0])
    }

    /// Call the elle_jit_call helper (4 args: func, args_ptr, nargs, vm)
    fn call_helper_call(
        &mut self,
        builder: &mut FunctionBuilder,
        func: cranelift_codegen::ir::Value,
        args_ptr: cranelift_codegen::ir::Value,
        nargs: cranelift_codegen::ir::Value,
        vm: cranelift_codegen::ir::Value,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let func_ref = self
            .module
            .declare_func_in_func(self.helpers.call, builder.func);
        let call = builder.ins().call(func_ref, &[func, args_ptr, nargs, vm]);
        Ok(builder.inst_results(call)[0])
    }
}
