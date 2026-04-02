//! LIR instruction → WASM instruction emission.
//!
//! Translates individual LIR instructions into WASM bytecode via
//! `wasm-encoder`. Covers arithmetic, comparisons, data operations,
//! calls, tail calls, constants, and memory access helpers.

use crate::lir::{BinOp, CmpOp, LirConst, LirInstr, Reg, UnaryOp};
use crate::value::repr::*;
use crate::value::Value;
use wasm_encoder::*;

use super::emit::*;

impl WasmEmitter {
    /// Emit a single LIR instruction as WASM code.
    pub(super) fn emit_instr(&mut self, f: &mut Function, instr: &LirInstr) {
        match instr {
            LirInstr::Const { dst, value } => {
                self.emit_const(f, *dst, value);
                match value {
                    LirConst::Int(_) => {
                        self.known_int.insert(*dst);
                    }
                    _ => {
                        self.known_int.remove(dst);
                    }
                }
            }
            LirInstr::ValueConst { dst, value } => {
                self.emit_value_const(f, *dst, *value);
                self.known_int.remove(dst);
            }
            LirInstr::BinOp { dst, op, lhs, rhs } => {
                let both_int = self.known_int.contains(lhs) && self.known_int.contains(rhs);
                self.emit_binop(f, *dst, *op, *lhs, *rhs, both_int);
                let is_bitwise = matches!(
                    op,
                    BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr
                );
                if both_int || is_bitwise {
                    self.known_int.insert(*dst);
                } else {
                    self.known_int.remove(dst);
                }
            }
            LirInstr::Compare { dst, op, lhs, rhs } => {
                self.emit_compare(f, *dst, *op, *lhs, *rhs);
            }
            LirInstr::UnaryOp { dst, op, src } => {
                self.emit_unary(f, *dst, *op, *src);
            }
            LirInstr::IsNil { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_NIL),
            LirInstr::IsPair { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_CONS),
            LirInstr::IsArray { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_ARRAY),
            LirInstr::IsArrayMut { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_ARRAY_MUT),
            LirInstr::IsStruct { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_STRUCT),
            LirInstr::IsStructMut { dst, src } => {
                self.emit_tag_check(f, *dst, *src, TAG_STRUCT_MUT)
            }
            LirInstr::IsSet { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_SET),
            LirInstr::IsSetMut { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_SET_MUT),
            LirInstr::LoadLocal { dst, slot } => {
                if self.is_closure {
                    f.instruction(&Instruction::LocalGet(self.local_slot_tag(*slot)));
                    f.instruction(&Instruction::LocalSet(self.tag_local(*dst)));
                    f.instruction(&Instruction::LocalGet(self.local_slot_pay(*slot)));
                    f.instruction(&Instruction::LocalSet(self.pay_local(*dst)));
                } else {
                    let src = Reg(*slot as u32);
                    self.copy_reg(f, src, *dst);
                }
            }
            LirInstr::StoreLocal { slot, src } => {
                if self.is_closure {
                    f.instruction(&Instruction::LocalGet(self.tag_local(*src)));
                    f.instruction(&Instruction::LocalSet(self.local_slot_tag(*slot)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(*src)));
                    f.instruction(&Instruction::LocalSet(self.local_slot_pay(*slot)));
                } else {
                    let dst = Reg(*slot as u32);
                    self.copy_reg(f, *src, dst);
                }
            }
            LirInstr::LoadCapture { dst, index } => {
                let offset = (*index as u64) * 16;
                f.instruction(&Instruction::LocalGet(self.env_local()));
                f.instruction(&Instruction::I64Load(MemArg {
                    offset,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::LocalSet(self.tag_local(*dst)));
                f.instruction(&Instruction::LocalGet(self.env_local()));
                f.instruction(&Instruction::I64Load(MemArg {
                    offset: offset + 8,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::LocalSet(self.pay_local(*dst)));
                // Auto-unwrap LBox
                f.instruction(&Instruction::LocalGet(self.tag_local(*dst)));
                f.instruction(&Instruction::I64Const(TAG_LBOX as i64));
                f.instruction(&Instruction::I64Eq);
                f.instruction(&Instruction::If(BlockType::Empty));
                self.emit_data_op1(f, *dst, OP_LOAD_LBOX, *dst);
                f.instruction(&Instruction::End);
            }
            LirInstr::LoadCaptureRaw { dst, index } => {
                let offset = (*index as u64) * 16;
                f.instruction(&Instruction::LocalGet(self.env_local()));
                f.instruction(&Instruction::I64Load(MemArg {
                    offset,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::LocalSet(self.tag_local(*dst)));
                f.instruction(&Instruction::LocalGet(self.env_local()));
                f.instruction(&Instruction::I64Load(MemArg {
                    offset: offset + 8,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::LocalSet(self.pay_local(*dst)));
            }
            LirInstr::StoreCapture { index, src } => {
                let offset = (*index as u64) * 16;
                // Write cell to args[0]
                f.instruction(&Instruction::I32Const(ARGS_BASE));
                f.instruction(&Instruction::LocalGet(self.env_local()));
                f.instruction(&Instruction::I64Load(MemArg {
                    offset,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::I64Store(MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::I32Const(ARGS_BASE));
                f.instruction(&Instruction::LocalGet(self.env_local()));
                f.instruction(&Instruction::I64Load(MemArg {
                    offset: offset + 8,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::I64Store(MemArg {
                    offset: 8,
                    align: 3,
                    memory_index: 0,
                }));
                // Write new value to args[1]
                self.write_val_to_mem(f, *src, 1);
                // Call OP_STORE_LBOX
                f.instruction(&Instruction::I32Const(OP_STORE_LBOX));
                f.instruction(&Instruction::I32Const(ARGS_BASE));
                f.instruction(&Instruction::I32Const(2));
                f.instruction(&Instruction::Call(FN_RT_DATA_OP));
                f.instruction(&Instruction::Drop);
                f.instruction(&Instruction::Drop);
                f.instruction(&Instruction::Drop);
            }
            LirInstr::Call { dst, func, args } => {
                self.emit_call(f, *dst, *func, args);
            }
            LirInstr::SuspendingCall { dst, func, args } => {
                self.emit_call(f, *dst, *func, args);
            }
            LirInstr::TailCall { func, args } => {
                if !self.is_closure {
                    let dst = Reg(0);
                    self.emit_call(f, dst, *func, args);
                    f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
                    f.instruction(&Instruction::I32Const(0));
                    f.instruction(&Instruction::Return);
                } else {
                    for (i, arg) in args.iter().enumerate() {
                        self.write_val_to_mem(f, *arg, i);
                    }
                    f.instruction(&Instruction::LocalGet(self.tag_local(*func)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(*func)));
                    f.instruction(&Instruction::I32Const(ARGS_BASE));
                    f.instruction(&Instruction::I32Const(args.len() as i32));
                    f.instruction(&Instruction::LocalGet(self.env_local()));
                    f.instruction(&Instruction::Call(FN_RT_PREPARE_TAIL_CALL));
                    self.emit_tail_call_dispatch(f);
                }
            }
            LirInstr::RegionEnter | LirInstr::RegionExit => {}
            LirInstr::Cons { dst, head, tail } => {
                self.emit_data_op2(f, *dst, OP_CONS, *head, *tail);
            }
            LirInstr::Car { dst, pair } => {
                self.emit_data_op1(f, *dst, OP_CAR, *pair);
            }
            LirInstr::Cdr { dst, pair } => {
                self.emit_data_op1(f, *dst, OP_CDR, *pair);
            }
            LirInstr::CarDestructure { dst, src } => {
                self.emit_data_op1(f, *dst, OP_CAR_DESTRUCTURE, *src);
            }
            LirInstr::CdrDestructure { dst, src } => {
                self.emit_data_op1(f, *dst, OP_CDR_DESTRUCTURE, *src);
            }
            LirInstr::CarOrNil { dst, src } => {
                self.emit_data_op1(f, *dst, OP_CAR_OR_NIL, *src);
            }
            LirInstr::CdrOrNil { dst, src } => {
                self.emit_data_op1(f, *dst, OP_CDR_OR_NIL, *src);
            }
            LirInstr::MakeArrayMut { dst, elements } => {
                self.emit_data_op_n(f, *dst, OP_MAKE_ARRAY, elements);
            }
            LirInstr::ArrayMutLen { dst, src } => {
                self.emit_data_op1(f, *dst, OP_ARRAY_LEN, *src);
            }
            LirInstr::ArrayMutRefDestructure { dst, src, index } => {
                self.emit_data_op1_imm(f, *dst, OP_ARRAY_REF_DESTRUCTURE, *src, *index as i64);
            }
            LirInstr::ArrayMutSliceFrom { dst, src, index } => {
                self.emit_data_op1_imm(f, *dst, OP_ARRAY_SLICE_FROM, *src, *index as i64);
            }
            LirInstr::ArrayMutRefOrNil { dst, src, index } => {
                self.emit_data_op1_imm(f, *dst, OP_ARRAY_REF_OR_NIL, *src, *index as i64);
            }
            LirInstr::StructGetOrNil { dst, src, key } => {
                self.emit_struct_get(f, *dst, OP_STRUCT_GET_OR_NIL, *src, key);
            }
            LirInstr::StructGetDestructure { dst, src, key } => {
                self.emit_struct_get(f, *dst, OP_STRUCT_GET_DESTRUCTURE, *src, key);
            }
            LirInstr::ArrayMutExtend { dst, array, source } => {
                self.emit_data_op2(f, *dst, OP_ARRAY_EXTEND, *array, *source);
            }
            LirInstr::ArrayMutPush { dst, array, value } => {
                self.emit_data_op2(f, *dst, OP_ARRAY_PUSH, *array, *value);
            }
            LirInstr::MakeLBox { dst, value } => {
                self.emit_data_op1(f, *dst, OP_MAKE_LBOX, *value);
            }
            LirInstr::LoadLBox { dst, cell } => {
                self.emit_data_op1(f, *dst, OP_LOAD_LBOX, *cell);
            }
            LirInstr::StoreLBox { cell, value } => {
                self.emit_data_op2(f, *cell, OP_STORE_LBOX, *cell, *value);
            }
            LirInstr::CallArrayMut { dst, func, args } => {
                self.emit_call_array(f, *dst, *func, *args);
            }
            LirInstr::TailCallArrayMut { func, args } => {
                if !self.is_closure {
                    let dst = Reg(0);
                    self.emit_call_array(f, dst, *func, *args);
                    f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
                    f.instruction(&Instruction::I32Const(0));
                    f.instruction(&Instruction::Return);
                } else {
                    self.write_val_to_mem(f, *args, 1);
                    f.instruction(&Instruction::LocalGet(self.tag_local(*func)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(*func)));
                    f.instruction(&Instruction::I32Const(ARGS_BASE));
                    f.instruction(&Instruction::I32Const(-1));
                    f.instruction(&Instruction::LocalGet(self.env_local()));
                    f.instruction(&Instruction::Call(FN_RT_PREPARE_TAIL_CALL));
                    self.emit_tail_call_dispatch(f);
                }
            }
            LirInstr::Eval { dst, expr, env } => {
                let _ = (dst, expr, env);
                f.instruction(&Instruction::Unreachable);
            }
            LirInstr::LoadResumeValue { dst } => {
                if self.may_suspend {
                    f.instruction(&Instruction::LocalGet(self.resume_tag_local));
                    f.instruction(&Instruction::LocalSet(self.tag_local(*dst)));
                    f.instruction(&Instruction::LocalGet(self.resume_pay_local));
                    f.instruction(&Instruction::LocalSet(self.pay_local(*dst)));
                } else {
                    f.instruction(&Instruction::Unreachable);
                }
            }
            LirInstr::MakeClosure {
                dst,
                func: nested,
                captures,
            } => {
                self.emit_make_closure(f, *dst, nested, captures);
            }
            LirInstr::PushParamFrame { pairs } => {
                for (i, (param_reg, val_reg)) in pairs.iter().enumerate() {
                    self.write_val_to_mem_offset(f, *param_reg, ARGS_BASE + (i as i32) * 32);
                    self.write_val_to_mem_offset(f, *val_reg, ARGS_BASE + (i as i32) * 32 + 16);
                }
                f.instruction(&Instruction::I32Const(ARGS_BASE));
                f.instruction(&Instruction::I32Const(pairs.len() as i32));
                f.instruction(&Instruction::Call(FN_RT_PUSH_PARAM));
            }
            LirInstr::PopParamFrame => {
                f.instruction(&Instruction::Call(FN_RT_POP_PARAM));
            }
            LirInstr::CheckSignalBound { .. } => {}
            LirInstr::StructRest {
                dst,
                src,
                exclude_keys,
            } => {
                self.write_val_to_mem(f, *src, 0);
                for (i, key) in exclude_keys.iter().enumerate() {
                    match key {
                        LirConst::Keyword(name) => {
                            self.emit_const_pool_load(f, *dst, Value::keyword(name));
                        }
                        LirConst::Symbol(id) => {
                            self.emit_const_pool_load(f, *dst, Value::symbol(id.0));
                        }
                        _ => {
                            f.instruction(&Instruction::I64Const(TAG_NIL as i64));
                            f.instruction(&Instruction::LocalSet(self.tag_local(*dst)));
                            f.instruction(&Instruction::I64Const(0));
                            f.instruction(&Instruction::LocalSet(self.pay_local(*dst)));
                        }
                    }
                    self.write_val_to_mem(f, *dst, i + 1);
                }
                f.instruction(&Instruction::I32Const(OP_STRUCT_REST));
                f.instruction(&Instruction::I32Const(ARGS_BASE));
                f.instruction(&Instruction::I32Const(1 + exclude_keys.len() as i32));
                f.instruction(&Instruction::Call(FN_RT_DATA_OP));
                self.store_result_with_signal(f, *dst);
            }
        }
    }

    /// Emit MakeClosure: write captures + metadata to memory, call rt_make_closure.
    fn emit_make_closure(
        &mut self,
        f: &mut Function,
        dst: Reg,
        nested: &crate::lir::LirFunction,
        captures: &[Reg],
    ) {
        let table_idx = self
            .closure_table_idx
            .get(&(nested as *const crate::lir::LirFunction))
            .copied()
            .expect("MakeClosure: nested function not found in table");

        for (i, cap) in captures.iter().enumerate() {
            self.write_val_to_mem(f, *cap, i);
        }

        let meta_base = ARGS_BASE + (captures.len() as i32) * 16;
        let meta_vals: [i64; 8] = [
            nested.num_captures as i64,
            nested.num_params as i64,
            nested.num_locals as i64,
            match nested.arity {
                crate::value::types::Arity::Exact(_) => 0,
                crate::value::types::Arity::AtLeast(_) => 1,
                crate::value::types::Arity::Range(_, _) => 2,
            },
            match nested.arity {
                crate::value::types::Arity::Exact(n) => n as i64,
                crate::value::types::Arity::AtLeast(n) => n as i64,
                crate::value::types::Arity::Range(min, _) => min as i64,
            },
            nested.lbox_params_mask as i64,
            nested.lbox_locals_mask as i64,
            nested.signal.bits.raw() as i64,
        ];
        for (i, val) in meta_vals.iter().enumerate() {
            f.instruction(&Instruction::I32Const(meta_base));
            f.instruction(&Instruction::I64Const(*val));
            f.instruction(&Instruction::I64Store(MemArg {
                offset: (i * 8) as u64,
                align: 3,
                memory_index: 0,
            }));
        }

        f.instruction(&Instruction::I32Const(table_idx as i32));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(meta_base));
        f.instruction(&Instruction::Call(FN_RT_MAKE_CLOSURE));
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
    }

    /// Emit a ValueConst (immediate inline or heap via const pool).
    pub(super) fn emit_value_const(&mut self, f: &mut Function, dst: Reg, value: Value) {
        if value.tag < TAG_HEAP_START {
            f.instruction(&Instruction::I64Const(value.tag as i64));
            f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
            f.instruction(&Instruction::I64Const(value.payload as i64));
            f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        } else {
            let idx = self.const_pool.len() as i32;
            self.const_pool.push(value);
            f.instruction(&Instruction::I32Const(idx));
            f.instruction(&Instruction::Call(FN_RT_LOAD_CONST));
            f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
            f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
        }
    }

    /// Emit a function call via rt_call.
    pub(super) fn emit_call(&self, f: &mut Function, dst: Reg, func: Reg, args: &[Reg]) {
        for (i, arg) in args.iter().enumerate() {
            let offset = (i * 16) as u64;
            f.instruction(&Instruction::I32Const(ARGS_BASE));
            f.instruction(&Instruction::LocalGet(self.tag_local(*arg)));
            f.instruction(&Instruction::I64Store(MemArg {
                offset,
                align: 3,
                memory_index: 0,
            }));
            f.instruction(&Instruction::I32Const(ARGS_BASE));
            f.instruction(&Instruction::LocalGet(self.pay_local(*arg)));
            f.instruction(&Instruction::I64Store(MemArg {
                offset: offset + 8,
                align: 3,
                memory_index: 0,
            }));
        }

        f.instruction(&Instruction::LocalGet(self.tag_local(func)));
        f.instruction(&Instruction::LocalGet(self.pay_local(func)));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(args.len() as i32));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::Call(FN_RT_CALL));
        self.store_result_with_signal(f, dst);
    }

    /// Emit CallArrayMut via rt_call with nargs=-1 protocol.
    pub(super) fn emit_call_array(&self, f: &mut Function, dst: Reg, func: Reg, args_array: Reg) {
        self.write_val_to_mem(f, func, 0);
        self.write_val_to_mem(f, args_array, 1);
        f.instruction(&Instruction::LocalGet(self.tag_local(func)));
        f.instruction(&Instruction::LocalGet(self.pay_local(func)));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(-1));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::Call(FN_RT_CALL));
        self.store_result_with_signal(f, dst);
    }

    /// Store result from (tag, payload, signal) triple, early-return on error.
    pub(super) fn store_result_with_signal(&self, f: &mut Function, dst: Reg) {
        f.instruction(&Instruction::LocalSet(self.signal_local));
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::LocalGet(self.signal_local));
        f.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        f.instruction(&Instruction::LocalGet(self.signal_local));
        f.instruction(&Instruction::If(BlockType::Empty));
        f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
        f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::Return);
        f.instruction(&Instruction::End);
    }

    /// Emit tail call dispatch after rt_prepare_tail_call returns.
    pub(super) fn emit_tail_call_dispatch(&self, f: &mut Function) {
        let tc_signal = self.signal_local;
        let tc_payload = self.pay_local(Reg(0));
        let tc_tag = self.tag_local(Reg(0));
        let tc_is_wasm = self.signal_local + 1;
        let tc_table_idx = self.signal_local + 2;
        let tc_env_ptr = self.signal_local + 3;

        f.instruction(&Instruction::LocalSet(tc_signal));
        f.instruction(&Instruction::LocalSet(tc_payload));
        f.instruction(&Instruction::LocalSet(tc_tag));
        f.instruction(&Instruction::LocalSet(tc_is_wasm));
        f.instruction(&Instruction::LocalSet(tc_table_idx));
        f.instruction(&Instruction::LocalSet(tc_env_ptr));

        f.instruction(&Instruction::LocalGet(tc_is_wasm));
        f.instruction(&Instruction::If(BlockType::Empty));
        {
            f.instruction(&Instruction::LocalGet(tc_env_ptr));
            f.instruction(&Instruction::I32Const(0));
            f.instruction(&Instruction::I32Const(0));
            f.instruction(&Instruction::I32Const(0));
            f.instruction(&Instruction::LocalGet(tc_table_idx));
            f.instruction(&Instruction::ReturnCallIndirect {
                type_index: 5,
                table_index: 0,
            });
        }
        f.instruction(&Instruction::Else);
        {
            f.instruction(&Instruction::LocalGet(tc_tag));
            f.instruction(&Instruction::LocalGet(tc_payload));
            f.instruction(&Instruction::I32Const(0));
            f.instruction(&Instruction::Return);
        }
        f.instruction(&Instruction::End);
    }

    /// 1-arg data op via rt_data_op.
    pub(super) fn emit_data_op1(&self, f: &mut Function, dst: Reg, op: i32, src: Reg) {
        self.write_val_to_mem(f, src, 0);
        f.instruction(&Instruction::I32Const(op));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(1));
        f.instruction(&Instruction::Call(FN_RT_DATA_OP));
        self.store_result_with_signal(f, dst);
    }

    /// 1-arg data op with immediate second argument.
    fn emit_data_op1_imm(&self, f: &mut Function, dst: Reg, op: i32, src: Reg, imm: i64) {
        self.write_val_to_mem(f, src, 0);
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I64Const(TAG_INT as i64));
        f.instruction(&Instruction::I64Store(MemArg {
            offset: 16,
            align: 3,
            memory_index: 0,
        }));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I64Const(imm));
        f.instruction(&Instruction::I64Store(MemArg {
            offset: 24,
            align: 3,
            memory_index: 0,
        }));
        f.instruction(&Instruction::I32Const(op));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(2));
        f.instruction(&Instruction::Call(FN_RT_DATA_OP));
        self.store_result_with_signal(f, dst);
    }

    /// 2-arg data op via rt_data_op.
    fn emit_data_op2(&self, f: &mut Function, dst: Reg, op: i32, a: Reg, b: Reg) {
        self.write_val_to_mem(f, a, 0);
        self.write_val_to_mem(f, b, 1);
        f.instruction(&Instruction::I32Const(op));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(2));
        f.instruction(&Instruction::Call(FN_RT_DATA_OP));
        self.store_result_with_signal(f, dst);
    }

    /// N-arg data op via rt_data_op.
    fn emit_data_op_n(&self, f: &mut Function, dst: Reg, op: i32, regs: &[Reg]) {
        for (i, reg) in regs.iter().enumerate() {
            self.write_val_to_mem(f, *reg, i);
        }
        f.instruction(&Instruction::I32Const(op));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(regs.len() as i32));
        f.instruction(&Instruction::Call(FN_RT_DATA_OP));
        self.store_result_with_signal(f, dst);
    }

    /// Struct get with a constant key.
    fn emit_struct_get(&mut self, f: &mut Function, dst: Reg, op: i32, src: Reg, key: &LirConst) {
        self.write_val_to_mem(f, src, 0);
        match key {
            LirConst::Keyword(name) => self.emit_const_pool_load(f, dst, Value::keyword(name)),
            LirConst::Symbol(id) => self.emit_const_pool_load(f, dst, Value::symbol(id.0)),
            _ => {
                f.instruction(&Instruction::I64Const(TAG_NIL as i64));
                f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
                f.instruction(&Instruction::I64Const(0));
                f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
            }
        }
        self.write_val_to_mem(f, dst, 1);
        f.instruction(&Instruction::I32Const(op));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(2));
        f.instruction(&Instruction::Call(FN_RT_DATA_OP));
        self.store_result_with_signal(f, dst);
    }

    /// Write a register to linear memory at ARGS_BASE + slot*16.
    pub(super) fn write_val_to_mem(&self, f: &mut Function, reg: Reg, slot: usize) {
        self.write_val_to_mem_offset(f, reg, ARGS_BASE + (slot as i32) * 16);
    }

    /// Write a register to linear memory at an absolute offset.
    pub(super) fn write_val_to_mem_offset(&self, f: &mut Function, reg: Reg, base: i32) {
        f.instruction(&Instruction::I32Const(base));
        f.instruction(&Instruction::LocalGet(self.tag_local(reg)));
        f.instruction(&Instruction::I64Store(MemArg {
            offset: 0,
            align: 3,
            memory_index: 0,
        }));
        f.instruction(&Instruction::I32Const(base));
        f.instruction(&Instruction::LocalGet(self.pay_local(reg)));
        f.instruction(&Instruction::I64Store(MemArg {
            offset: 8,
            align: 3,
            memory_index: 0,
        }));
    }

    /// Emit truthiness check: pushes i32 (0=falsy, 1=truthy).
    pub(super) fn emit_truthiness_check(&self, f: &mut Function, cond: Reg) {
        f.instruction(&Instruction::LocalGet(self.tag_local(cond)));
        f.instruction(&Instruction::I64Const(TAG_FALSE as i64));
        f.instruction(&Instruction::I64Ne);
        f.instruction(&Instruction::LocalGet(self.tag_local(cond)));
        f.instruction(&Instruction::I64Const(TAG_NIL as i64));
        f.instruction(&Instruction::I64Ne);
        f.instruction(&Instruction::I32And);
    }

    /// Add value to const pool and emit rt_load_const into dst.
    pub(super) fn emit_const_pool_load(&mut self, f: &mut Function, dst: Reg, value: Value) {
        let idx = self.const_pool.len() as i32;
        self.const_pool.push(value);
        f.instruction(&Instruction::I32Const(idx));
        f.instruction(&Instruction::Call(FN_RT_LOAD_CONST));
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
    }

    pub(super) fn emit_const(&mut self, f: &mut Function, dst: Reg, value: &LirConst) {
        match value {
            LirConst::String(s) => {
                self.emit_const_pool_load(f, dst, Value::string(s.clone()));
            }
            LirConst::Symbol(id) => {
                self.emit_const_pool_load(f, dst, Value::symbol(id.0));
            }
            LirConst::Keyword(name) => {
                self.emit_const_pool_load(f, dst, Value::keyword(name));
            }
            _ => {
                let (tag, payload) = match value {
                    LirConst::Nil => (TAG_NIL as i64, 0i64),
                    LirConst::EmptyList => (TAG_EMPTY_LIST as i64, 0),
                    LirConst::Bool(true) => (TAG_TRUE as i64, 0),
                    LirConst::Bool(false) => (TAG_FALSE as i64, 0),
                    LirConst::Int(n) => (TAG_INT as i64, *n),
                    LirConst::Float(x) => (TAG_FLOAT as i64, x.to_bits() as i64),
                    LirConst::Symbol(_) | LirConst::Keyword(_) | LirConst::String(_) => {
                        unreachable!()
                    }
                };
                f.instruction(&Instruction::I64Const(tag));
                f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
                f.instruction(&Instruction::I64Const(payload));
                f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
            }
        }
    }

    pub(super) fn emit_binop(
        &self,
        f: &mut Function,
        dst: Reg,
        op: BinOp,
        lhs: Reg,
        rhs: Reg,
        both_int: bool,
    ) {
        if both_int
            || matches!(
                op,
                BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr
            )
        {
            f.instruction(&Instruction::I64Const(TAG_INT as i64));
            f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
            f.instruction(&Instruction::LocalGet(self.pay_local(lhs)));
            f.instruction(&Instruction::LocalGet(self.pay_local(rhs)));
            match op {
                BinOp::Add => f.instruction(&Instruction::I64Add),
                BinOp::Sub => f.instruction(&Instruction::I64Sub),
                BinOp::Mul => f.instruction(&Instruction::I64Mul),
                BinOp::Div => f.instruction(&Instruction::I64DivS),
                BinOp::Rem => f.instruction(&Instruction::I64RemS),
                BinOp::BitAnd => f.instruction(&Instruction::I64And),
                BinOp::BitOr => f.instruction(&Instruction::I64Or),
                BinOp::BitXor => f.instruction(&Instruction::I64Xor),
                BinOp::Shl => f.instruction(&Instruction::I64Shl),
                BinOp::Shr => f.instruction(&Instruction::I64ShrS),
            };
            f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
            return;
        }

        f.instruction(&Instruction::LocalGet(self.tag_local(lhs)));
        f.instruction(&Instruction::I64Const(TAG_FLOAT as i64));
        f.instruction(&Instruction::I64Eq);
        f.instruction(&Instruction::LocalGet(self.tag_local(rhs)));
        f.instruction(&Instruction::I64Const(TAG_FLOAT as i64));
        f.instruction(&Instruction::I64Eq);
        f.instruction(&Instruction::I32Or);
        f.instruction(&Instruction::If(BlockType::Empty));
        {
            f.instruction(&Instruction::I64Const(TAG_FLOAT as i64));
            f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
            self.emit_to_f64(f, lhs);
            self.emit_to_f64(f, rhs);
            match op {
                BinOp::Add => {
                    f.instruction(&Instruction::F64Add);
                }
                BinOp::Sub => {
                    f.instruction(&Instruction::F64Sub);
                }
                BinOp::Mul => {
                    f.instruction(&Instruction::F64Mul);
                }
                BinOp::Div => {
                    f.instruction(&Instruction::F64Div);
                }
                BinOp::Rem => {
                    f.instruction(&Instruction::Drop);
                    f.instruction(&Instruction::Drop);
                    self.emit_to_f64(f, lhs);
                    self.emit_to_f64(f, lhs);
                    self.emit_to_f64(f, rhs);
                    f.instruction(&Instruction::F64Div);
                    f.instruction(&Instruction::F64Floor);
                    self.emit_to_f64(f, rhs);
                    f.instruction(&Instruction::F64Mul);
                    f.instruction(&Instruction::F64Sub);
                }
                _ => unreachable!(),
            }
            f.instruction(&Instruction::I64ReinterpretF64);
            f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        }
        f.instruction(&Instruction::Else);
        {
            f.instruction(&Instruction::I64Const(TAG_INT as i64));
            f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
            f.instruction(&Instruction::LocalGet(self.pay_local(lhs)));
            f.instruction(&Instruction::LocalGet(self.pay_local(rhs)));
            match op {
                BinOp::Add => f.instruction(&Instruction::I64Add),
                BinOp::Sub => f.instruction(&Instruction::I64Sub),
                BinOp::Mul => f.instruction(&Instruction::I64Mul),
                BinOp::Div => f.instruction(&Instruction::I64DivS),
                BinOp::Rem => f.instruction(&Instruction::I64RemS),
                _ => unreachable!(),
            };
            f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        }
        f.instruction(&Instruction::End);
    }

    fn emit_to_f64(&self, f: &mut Function, reg: Reg) {
        f.instruction(&Instruction::LocalGet(self.tag_local(reg)));
        f.instruction(&Instruction::I64Const(TAG_FLOAT as i64));
        f.instruction(&Instruction::I64Eq);
        f.instruction(&Instruction::If(BlockType::Result(ValType::F64)));
        f.instruction(&Instruction::LocalGet(self.pay_local(reg)));
        f.instruction(&Instruction::F64ReinterpretI64);
        f.instruction(&Instruction::Else);
        f.instruction(&Instruction::LocalGet(self.pay_local(reg)));
        f.instruction(&Instruction::F64ConvertI64S);
        f.instruction(&Instruction::End);
    }

    pub(super) fn emit_compare(&self, f: &mut Function, dst: Reg, op: CmpOp, lhs: Reg, rhs: Reg) {
        f.instruction(&Instruction::LocalGet(self.tag_local(lhs)));
        f.instruction(&Instruction::I64Const(TAG_FLOAT as i64));
        f.instruction(&Instruction::I64Eq);
        f.instruction(&Instruction::LocalGet(self.tag_local(rhs)));
        f.instruction(&Instruction::I64Const(TAG_FLOAT as i64));
        f.instruction(&Instruction::I64Eq);
        f.instruction(&Instruction::I32Or);
        f.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
        {
            self.emit_to_f64(f, lhs);
            self.emit_to_f64(f, rhs);
            match op {
                CmpOp::Eq => f.instruction(&Instruction::F64Eq),
                CmpOp::Ne => f.instruction(&Instruction::F64Ne),
                CmpOp::Lt => f.instruction(&Instruction::F64Lt),
                CmpOp::Le => f.instruction(&Instruction::F64Le),
                CmpOp::Gt => f.instruction(&Instruction::F64Gt),
                CmpOp::Ge => f.instruction(&Instruction::F64Ge),
            };
        }
        f.instruction(&Instruction::Else);
        {
            f.instruction(&Instruction::LocalGet(self.pay_local(lhs)));
            f.instruction(&Instruction::LocalGet(self.pay_local(rhs)));
            match op {
                CmpOp::Eq => f.instruction(&Instruction::I64Eq),
                CmpOp::Ne => f.instruction(&Instruction::I64Ne),
                CmpOp::Lt => f.instruction(&Instruction::I64LtS),
                CmpOp::Le => f.instruction(&Instruction::I64LeS),
                CmpOp::Gt => f.instruction(&Instruction::I64GtS),
                CmpOp::Ge => f.instruction(&Instruction::I64GeS),
            };
        }
        f.instruction(&Instruction::End);
        self.emit_bool_from_i32(f, dst);
    }

    fn emit_unary(&self, f: &mut Function, dst: Reg, op: UnaryOp, src: Reg) {
        match op {
            UnaryOp::Neg => {
                f.instruction(&Instruction::I64Const(TAG_INT as i64));
                f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
                f.instruction(&Instruction::I64Const(0));
                f.instruction(&Instruction::LocalGet(self.pay_local(src)));
                f.instruction(&Instruction::I64Sub);
                f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
            }
            UnaryOp::Not => {
                f.instruction(&Instruction::LocalGet(self.tag_local(src)));
                f.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                f.instruction(&Instruction::I64Eq);
                f.instruction(&Instruction::LocalGet(self.tag_local(src)));
                f.instruction(&Instruction::I64Const(TAG_NIL as i64));
                f.instruction(&Instruction::I64Eq);
                f.instruction(&Instruction::I32Or);
                self.emit_bool_from_i32(f, dst);
            }
            UnaryOp::BitNot => {
                f.instruction(&Instruction::I64Const(TAG_INT as i64));
                f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
                f.instruction(&Instruction::I64Const(-1));
                f.instruction(&Instruction::LocalGet(self.pay_local(src)));
                f.instruction(&Instruction::I64Xor);
                f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
            }
        }
    }

    pub(super) fn emit_tag_check(&self, f: &mut Function, dst: Reg, src: Reg, expected_tag: u64) {
        f.instruction(&Instruction::LocalGet(self.tag_local(src)));
        f.instruction(&Instruction::I64Const(expected_tag as i64));
        f.instruction(&Instruction::I64Eq);
        self.emit_bool_from_i32(f, dst);
    }

    fn emit_bool_from_i32(&self, f: &mut Function, dst: Reg) {
        f.instruction(&Instruction::If(BlockType::Empty));
        f.instruction(&Instruction::I64Const(TAG_TRUE as i64));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
        f.instruction(&Instruction::Else);
        f.instruction(&Instruction::I64Const(TAG_FALSE as i64));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
        f.instruction(&Instruction::End);
        f.instruction(&Instruction::I64Const(0));
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
    }

    pub(super) fn copy_reg(&self, f: &mut Function, src: Reg, dst: Reg) {
        f.instruction(&Instruction::LocalGet(self.tag_local(src)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
        f.instruction(&Instruction::LocalGet(self.pay_local(src)));
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
    }
}
