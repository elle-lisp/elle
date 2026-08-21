//! Data operations dispatched through the `rt_data_op` runtime helper.
//!
//! These emitters cover the 1-, 2-, and N-argument variants plus the
//! struct-get special case, all of which marshal their operands into linear
//! memory at `ARGS_BASE` before invoking `rt_data_op`.

use super::*;

impl WasmEmitter {
    /// 1-arg data op via rt_data_op.
    pub(in crate::wasm) fn emit_data_op1(&self, f: &mut Function, dst: Reg, op: i32, src: Reg) {
        self.write_val_to_mem(f, src, 0);
        f.instruction(&Instruction::I32Const(op));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(1));
        f.instruction(&Instruction::Call(FN_RT_DATA_OP));
        self.store_result_with_signal(f, dst);
    }

    /// 1-arg data op with immediate second argument.
    pub(super) fn emit_data_op1_imm(
        &self,
        f: &mut Function,
        dst: Reg,
        op: i32,
        src: Reg,
        imm: i64,
    ) {
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
    pub(super) fn emit_data_op2(&self, f: &mut Function, dst: Reg, op: i32, a: Reg, b: Reg) {
        self.write_val_to_mem(f, a, 0);
        self.write_val_to_mem(f, b, 1);
        f.instruction(&Instruction::I32Const(op));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(2));
        f.instruction(&Instruction::Call(FN_RT_DATA_OP));
        self.store_result_with_signal(f, dst);
    }

    /// N-arg data op via rt_data_op.
    pub(super) fn emit_data_op_n(&self, f: &mut Function, dst: Reg, op: i32, regs: &[Reg]) {
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
    pub(super) fn emit_struct_get(
        &mut self,
        f: &mut Function,
        dst: Reg,
        op: i32,
        src: Reg,
        key: &LirConst,
    ) {
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
}
