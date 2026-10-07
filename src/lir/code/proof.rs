// audited: 2026-10-06
//! The constructors that build an arithmetic, comparison or unary instruction, with or without an operand proof.
//!
//! docs/impl/lir.md

use super::instr::InstrRef;
use crate::lir::{BinOp, CmpOp, OperandProof, Reg, UnaryOp};

impl InstrRef<'_> {
    /// A binary arithmetic or bitwise operation, claiming nothing about its
    /// operands.
    pub fn binop(dst: Reg, op: BinOp, lhs: Reg, rhs: Reg) -> Self {
        Self::binop_proved(dst, op, lhs, rhs, OperandProof::Unproven)
    }

    /// A comparison, claiming nothing about its operands.
    pub fn compare(dst: Reg, op: CmpOp, lhs: Reg, rhs: Reg) -> Self {
        Self::compare_proved(dst, op, lhs, rhs, OperandProof::Unproven)
    }

    /// A unary operation, claiming nothing about its operand.
    pub fn unary(dst: Reg, op: UnaryOp, src: Reg) -> Self {
        Self::unary_proved(dst, op, src, OperandProof::Unproven)
    }

    /// A binary operation over operands proven to be integers.
    pub fn int_binop(dst: Reg, op: BinOp, lhs: Reg, rhs: Reg) -> Self {
        Self::binop_proved(dst, op, lhs, rhs, OperandProof::Int)
    }

    /// A comparison of operands proven to be integers.
    pub fn int_compare(dst: Reg, op: CmpOp, lhs: Reg, rhs: Reg) -> Self {
        Self::compare_proved(dst, op, lhs, rhs, OperandProof::Int)
    }

    /// A unary operation over an operand proven to be an integer.
    pub fn int_unary(dst: Reg, op: UnaryOp, src: Reg) -> Self {
        Self::unary_proved(dst, op, src, OperandProof::Int)
    }

    /// A binary operation carrying `proof`.
    pub fn binop_proved(dst: Reg, op: BinOp, lhs: Reg, rhs: Reg, proof: OperandProof) -> Self {
        InstrRef::BinOp {
            dst,
            op,
            lhs,
            rhs,
            proof,
        }
    }

    /// A comparison carrying `proof`.
    pub fn compare_proved(dst: Reg, op: CmpOp, lhs: Reg, rhs: Reg, proof: OperandProof) -> Self {
        InstrRef::Compare {
            dst,
            op,
            lhs,
            rhs,
            proof,
        }
    }

    /// A unary operation carrying `proof`.
    pub fn unary_proved(dst: Reg, op: UnaryOp, src: Reg, proof: OperandProof) -> Self {
        InstrRef::UnaryOp {
            dst,
            op,
            src,
            proof,
        }
    }
}
