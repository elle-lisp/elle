use super::*;

impl<'a> Lowerer<'a> {
    pub(super) fn lower_intrinsic(
        &mut self,
        op: crate::hir::IntrinsicOp,
        args: &[Hir],
    ) -> Result<Reg, String> {
        use crate::hir::IntrinsicOp;

        // Lower all arguments first
        let mut arg_regs = Vec::with_capacity(args.len());
        for arg in args {
            arg_regs.push(self.lower_expr(arg)?);
        }

        let dst = self.fresh_reg();
        match op {
            // Binary arithmetic
            IntrinsicOp::Add => {
                self.emit(LirInstr::BinOp {
                    dst,
                    op: BinOp::Add,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::Sub => {
                if arg_regs.len() == 1 {
                    self.emit(LirInstr::UnaryOp {
                        dst,
                        op: UnaryOp::Neg,
                        src: arg_regs[0],
                    });
                } else {
                    self.emit(LirInstr::BinOp {
                        dst,
                        op: BinOp::Sub,
                        lhs: arg_regs[0],
                        rhs: arg_regs[1],
                    });
                }
            }
            IntrinsicOp::Mul => {
                self.emit(LirInstr::BinOp {
                    dst,
                    op: BinOp::Mul,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::Div => {
                self.emit(LirInstr::BinOp {
                    dst,
                    op: BinOp::Div,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::Rem => {
                self.emit(LirInstr::BinOp {
                    dst,
                    op: BinOp::Rem,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::Mod => {
                // Floored modulus: ((a % b) + b) % b
                // The stack-based emitter consumes registers on use, so spill b
                // to a local slot and reload fresh copies for each operation.
                let b_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.emit(LirInstr::StoreLocal {
                    slot: b_slot,
                    src: arg_regs[1],
                });
                // Step 1: t = a % b (uses original arg_regs, but b was consumed by StoreLocal)
                let b1 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: b1,
                    slot: b_slot,
                });
                let t = self.fresh_reg();
                self.emit(LirInstr::BinOp {
                    dst: t,
                    op: BinOp::Rem,
                    lhs: arg_regs[0],
                    rhs: b1,
                });
                // Step 2: t2 = t + b
                let b2 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: b2,
                    slot: b_slot,
                });
                let t2 = self.fresh_reg();
                self.emit(LirInstr::BinOp {
                    dst: t2,
                    op: BinOp::Add,
                    lhs: t,
                    rhs: b2,
                });
                // Step 3: result = t2 % b
                let b3 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: b3,
                    slot: b_slot,
                });
                self.emit(LirInstr::BinOp {
                    dst,
                    op: BinOp::Rem,
                    lhs: t2,
                    rhs: b3,
                });
            }
            // Comparisons
            IntrinsicOp::Eq => {
                self.emit(LirInstr::Compare {
                    dst,
                    op: CmpOp::Eq,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::Lt => {
                self.emit(LirInstr::Compare {
                    dst,
                    op: CmpOp::Lt,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::Gt => {
                self.emit(LirInstr::Compare {
                    dst,
                    op: CmpOp::Gt,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::Le => {
                self.emit(LirInstr::Compare {
                    dst,
                    op: CmpOp::Le,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::Ge => {
                self.emit(LirInstr::Compare {
                    dst,
                    op: CmpOp::Ge,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            // Logical
            IntrinsicOp::Not => {
                self.emit(LirInstr::UnaryOp {
                    dst,
                    op: UnaryOp::Not,
                    src: arg_regs[0],
                });
            }
            // Conversion
            IntrinsicOp::Int => {
                self.emit(LirInstr::Convert {
                    dst,
                    op: ConvOp::FloatToInt,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::Float => {
                self.emit(LirInstr::Convert {
                    dst,
                    op: ConvOp::IntToFloat,
                    src: arg_regs[0],
                });
            }
            // List operations
            IntrinsicOp::Pair => {
                self.emit_alloc(|region| LirInstr::List {
                    region,
                    dst,
                    head: arg_regs[0],
                    tail: arg_regs[1],
                });
            }
            IntrinsicOp::First => {
                self.emit(LirInstr::First {
                    dst,
                    pair: arg_regs[0],
                });
            }
            IntrinsicOp::Rest => {
                self.emit(LirInstr::Rest {
                    dst,
                    pair: arg_regs[0],
                });
            }
            // Bitwise
            IntrinsicOp::BitAnd => {
                self.emit(LirInstr::BinOp {
                    dst,
                    op: BinOp::BitAnd,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::BitOr => {
                self.emit(LirInstr::BinOp {
                    dst,
                    op: BinOp::BitOr,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::BitXor => {
                self.emit(LirInstr::BinOp {
                    dst,
                    op: BinOp::BitXor,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::Shl => {
                self.emit(LirInstr::BinOp {
                    dst,
                    op: BinOp::Shl,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            IntrinsicOp::Shr => {
                self.emit(LirInstr::BinOp {
                    dst,
                    op: BinOp::Shr,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            // Bitwise NOT
            IntrinsicOp::BitNot => {
                self.emit(LirInstr::UnaryOp {
                    dst,
                    op: UnaryOp::BitNot,
                    src: arg_regs[0],
                });
            }
            // Not-equal comparison
            IntrinsicOp::Ne => {
                self.emit(LirInstr::Compare {
                    dst,
                    op: CmpOp::Ne,
                    lhs: arg_regs[0],
                    rhs: arg_regs[1],
                });
            }
            // Type predicates
            IntrinsicOp::IsNil => {
                self.emit(LirInstr::IsNil {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsEmpty => {
                self.emit(LirInstr::IsEmpty {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsBool => {
                self.emit(LirInstr::IsBool {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsInt => {
                self.emit(LirInstr::IsInt {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsFloat => {
                self.emit(LirInstr::IsFloat {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsString => {
                self.emit(LirInstr::IsString {
                    dst,
                    src: arg_regs[0],
                });
            }
            _ => return self.lower_intrinsic_rest(op, &arg_regs, dst),
        }
        Ok(dst)
    }
}

mod rest;
