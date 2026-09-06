// audited: 2026-09-06
// src/lir/lower/AGENTS.md
// docs/intrinsics.md
//! Lowering a `%`-intrinsic: arithmetic, comparison, conversion, pairs, bitwise,
//! and the type predicates.
//!
//! The match is a chain. This file lowers the operand registers once, then takes
//! the ops above; `rest` takes the collection, freeze/thaw and remaining
//! predicate ops from the same registers.

use super::*;

impl<'a> Lowerer<'a> {
    /// What the front end proved about `args`.
    ///
    /// `Int` needs every operand's inferred type to be exactly `int`. A node the
    /// inference never typed reads as Top, an absent map reads the same way, and
    /// both give `Unproven` — which every backend serves correctly, only slower.
    ///
    /// The types come from the map the intrinsic operand contract discharged
    /// against, so this claims nothing the contract did not already prove
    /// (docs/impl/lir.md).
    fn operand_proof(&self, args: &[Hir]) -> OperandProof {
        let all_int = args.iter().all(|arg| {
            self.hir_types.get(&arg.id).copied() == Some(crate::hir::types::TypeInterner::INT)
        });
        if all_int {
            OperandProof::Int
        } else {
            OperandProof::Unproven
        }
    }

    pub(super) fn lower_intrinsic(
        &mut self,
        op: crate::hir::IntrinsicOp,
        args: &[Hir],
    ) -> Result<Reg, String> {
        use crate::hir::IntrinsicOp;

        // Read the proof from the argument nodes before lowering consumes them.
        let proof = self.operand_proof(args);

        // Lower all arguments first
        let mut arg_regs = Vec::with_capacity(args.len());
        for arg in args {
            arg_regs.push(self.lower_expr(arg)?);
        }

        let dst = self.fresh_reg();
        match op {
            // Binary arithmetic
            IntrinsicOp::Add => {
                self.emit(LirInstr::binop_proved(
                    dst,
                    BinOp::Add,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            IntrinsicOp::Sub => {
                if arg_regs.len() == 1 {
                    self.emit(LirInstr::unary_proved(
                        dst,
                        UnaryOp::Neg,
                        arg_regs[0],
                        proof,
                    ));
                } else {
                    self.emit(LirInstr::binop_proved(
                        dst,
                        BinOp::Sub,
                        arg_regs[0],
                        arg_regs[1],
                        proof,
                    ));
                }
            }
            IntrinsicOp::Mul => {
                self.emit(LirInstr::binop_proved(
                    dst,
                    BinOp::Mul,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            IntrinsicOp::Div => {
                self.emit(LirInstr::binop_proved(
                    dst,
                    BinOp::Div,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            IntrinsicOp::Rem => {
                self.emit(LirInstr::binop_proved(
                    dst,
                    BinOp::Rem,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
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
                // Every step operates on the two original operands or on an
                // integer derived from them, so each carries the same proof.
                let t = self.fresh_reg();
                self.emit(LirInstr::binop_proved(
                    t,
                    BinOp::Rem,
                    arg_regs[0],
                    b1,
                    proof,
                ));
                // Step 2: t2 = t + b
                let b2 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: b2,
                    slot: b_slot,
                });
                let t2 = self.fresh_reg();
                self.emit(LirInstr::binop_proved(t2, BinOp::Add, t, b2, proof));
                // Step 3: result = t2 % b
                let b3 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: b3,
                    slot: b_slot,
                });
                self.emit(LirInstr::binop_proved(dst, BinOp::Rem, t2, b3, proof));
            }
            // Comparisons
            IntrinsicOp::Eq => {
                self.emit(LirInstr::compare_proved(
                    dst,
                    CmpOp::Eq,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            IntrinsicOp::Lt => {
                self.emit(LirInstr::compare_proved(
                    dst,
                    CmpOp::Lt,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            IntrinsicOp::Gt => {
                self.emit(LirInstr::compare_proved(
                    dst,
                    CmpOp::Gt,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            IntrinsicOp::Le => {
                self.emit(LirInstr::compare_proved(
                    dst,
                    CmpOp::Le,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            IntrinsicOp::Ge => {
                self.emit(LirInstr::compare_proved(
                    dst,
                    CmpOp::Ge,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            // Logical
            IntrinsicOp::Not => {
                // `%not` is truthiness negation, total on every value, and the
                // JIT inlines it whatever the operand is.
                self.emit(LirInstr::unary(dst, UnaryOp::Not, arg_regs[0]));
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
            // Bitwise. The contract already requires proven ints here, so these
            // carry the proof by construction.
            IntrinsicOp::BitAnd => {
                self.emit(LirInstr::binop_proved(
                    dst,
                    BinOp::BitAnd,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            IntrinsicOp::BitOr => {
                self.emit(LirInstr::binop_proved(
                    dst,
                    BinOp::BitOr,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            IntrinsicOp::BitXor => {
                self.emit(LirInstr::binop_proved(
                    dst,
                    BinOp::BitXor,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            IntrinsicOp::Shl => {
                self.emit(LirInstr::binop_proved(
                    dst,
                    BinOp::Shl,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            IntrinsicOp::Shr => {
                self.emit(LirInstr::binop_proved(
                    dst,
                    BinOp::Shr,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            // Bitwise NOT
            IntrinsicOp::BitNot => {
                self.emit(LirInstr::unary_proved(
                    dst,
                    UnaryOp::BitNot,
                    arg_regs[0],
                    proof,
                ));
            }
            // Not-equal comparison
            IntrinsicOp::Ne => {
                self.emit(LirInstr::compare_proved(
                    dst,
                    CmpOp::Ne,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
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
