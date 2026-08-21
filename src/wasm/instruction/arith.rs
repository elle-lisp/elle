//! Inline arithmetic, comparison, and unary emission.
//!
//! These emitters produce WASM inline (no runtime call) for the fast int/float
//! numeric paths, branching on the operand tags to pick i64 vs f64 opcodes and
//! folding the boolean result back into a tagged value.

use super::*;

impl WasmEmitter {
    pub(in crate::wasm) fn emit_binop(
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

    pub(in crate::wasm) fn emit_compare(
        &self,
        f: &mut Function,
        dst: Reg,
        op: CmpOp,
        lhs: Reg,
        rhs: Reg,
    ) {
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

    pub(super) fn emit_unary(&self, f: &mut Function, dst: Reg, op: UnaryOp, src: Reg) {
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

    pub(in crate::wasm) fn emit_tag_check(
        &self,
        f: &mut Function,
        dst: Reg,
        src: Reg,
        expected_tag: u64,
    ) {
        f.instruction(&Instruction::LocalGet(self.tag_local(src)));
        f.instruction(&Instruction::I64Const(expected_tag as i64));
        f.instruction(&Instruction::I64Eq);
        self.emit_bool_from_i32(f, dst);
    }

    pub(super) fn emit_bool_from_i32(&self, f: &mut Function, dst: Reg) {
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
}
