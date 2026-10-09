// audited: 2026-10-06
//! Lowering a `%`-intrinsic: arithmetic, comparison, conversion, pairs, bitwise, and the type predicates.
//!
//! src/lir/lower/AGENTS.md
//! docs/intrinsics.md
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
                self.emit(InstrRef::binop_proved(
                    dst,
                    BinOp::Add,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            IntrinsicOp::Sub => {
                if arg_regs.len() == 1 {
                    self.emit(InstrRef::unary_proved(
                        dst,
                        UnaryOp::Neg,
                        arg_regs[0],
                        proof,
                    ));
                } else {
                    self.emit(InstrRef::binop_proved(
                        dst,
                        BinOp::Sub,
                        arg_regs[0],
                        arg_regs[1],
                        proof,
                    ));
                }
            }
            IntrinsicOp::Mul => {
                self.emit(InstrRef::binop_proved(
                    dst,
                    BinOp::Mul,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            IntrinsicOp::Div => {
                self.emit(InstrRef::binop_proved(
                    dst,
                    BinOp::Div,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            IntrinsicOp::Rem => {
                self.emit(InstrRef::binop_proved(
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
                let b_slot = self.fresh_local();
                self.emit(InstrRef::StoreLocal {
                    slot: b_slot,
                    src: arg_regs[1],
                });
                // Step 1: t = a % b (uses original arg_regs, but b was consumed by StoreLocal)
                let b1 = self.fresh_reg();
                self.emit(InstrRef::LoadLocal {
                    dst: b1,
                    slot: b_slot,
                });
                // The proof is Int only where both operands are, and then every
                // intermediate is an integer too: the remainder of two ints,
                // its sum with the divisor, and that sum's remainder. Over
                // anything else the proof is Unproven and no step claims one.
                let t = self.fresh_reg();
                self.emit(InstrRef::binop_proved(
                    t,
                    BinOp::Rem,
                    arg_regs[0],
                    b1,
                    proof,
                ));
                // Step 2: t2 = t + b
                let b2 = self.fresh_reg();
                self.emit(InstrRef::LoadLocal {
                    dst: b2,
                    slot: b_slot,
                });
                let t2 = self.fresh_reg();
                self.emit(InstrRef::binop_proved(t2, BinOp::Add, t, b2, proof));
                // Step 3: result = t2 % b
                let b3 = self.fresh_reg();
                self.emit(InstrRef::LoadLocal {
                    dst: b3,
                    slot: b_slot,
                });
                self.emit(InstrRef::binop_proved(dst, BinOp::Rem, t2, b3, proof));
            }
            // Comparisons
            IntrinsicOp::Eq => {
                self.emit(InstrRef::compare_proved(
                    dst,
                    CmpOp::Eq,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            IntrinsicOp::Lt => {
                self.emit(InstrRef::compare_proved(
                    dst,
                    CmpOp::Lt,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            IntrinsicOp::Gt => {
                self.emit(InstrRef::compare_proved(
                    dst,
                    CmpOp::Gt,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            IntrinsicOp::Le => {
                self.emit(InstrRef::compare_proved(
                    dst,
                    CmpOp::Le,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            IntrinsicOp::Ge => {
                self.emit(InstrRef::compare_proved(
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
                self.emit(InstrRef::unary(dst, UnaryOp::Not, arg_regs[0]));
            }
            // Conversion
            IntrinsicOp::Int => {
                self.emit(InstrRef::Convert {
                    dst,
                    op: ConvOp::FloatToInt,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::Float => {
                self.emit(InstrRef::Convert {
                    dst,
                    op: ConvOp::IntToFloat,
                    src: arg_regs[0],
                });
            }
            // List operations
            IntrinsicOp::Pair => {
                self.emit_alloc(|region| InstrRef::List {
                    region,
                    dst,
                    head: arg_regs[0],
                    tail: arg_regs[1],
                });
            }
            IntrinsicOp::First => {
                self.emit(InstrRef::First {
                    dst,
                    pair: arg_regs[0],
                });
            }
            IntrinsicOp::Rest => {
                self.emit(InstrRef::Rest {
                    dst,
                    pair: arg_regs[0],
                });
            }
            // Bitwise. The contract already requires proven ints here, so these
            // carry the proof by construction.
            IntrinsicOp::BitAnd => {
                self.emit(InstrRef::binop_proved(
                    dst,
                    BinOp::BitAnd,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            IntrinsicOp::BitOr => {
                self.emit(InstrRef::binop_proved(
                    dst,
                    BinOp::BitOr,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            IntrinsicOp::BitXor => {
                self.emit(InstrRef::binop_proved(
                    dst,
                    BinOp::BitXor,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            IntrinsicOp::Shl => {
                self.emit(InstrRef::binop_proved(
                    dst,
                    BinOp::Shl,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            IntrinsicOp::Shr => {
                self.emit(InstrRef::binop_proved(
                    dst,
                    BinOp::Shr,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            // Bitwise NOT
            IntrinsicOp::BitNot => {
                self.emit(InstrRef::unary_proved(
                    dst,
                    UnaryOp::BitNot,
                    arg_regs[0],
                    proof,
                ));
            }
            // Not-equal comparison
            IntrinsicOp::Ne => {
                self.emit(InstrRef::compare_proved(
                    dst,
                    CmpOp::Ne,
                    arg_regs[0],
                    arg_regs[1],
                    proof,
                ));
            }
            // Type predicates
            IntrinsicOp::IsNil => {
                self.emit(InstrRef::IsNil {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsEmpty => {
                self.emit(InstrRef::IsEmpty {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsBool => {
                self.emit(InstrRef::IsBool {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsInt => {
                self.emit(InstrRef::IsInt {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsFloat => {
                self.emit(InstrRef::IsFloat {
                    dst,
                    src: arg_regs[0],
                });
            }
            IntrinsicOp::IsString => {
                self.emit(InstrRef::IsString {
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
