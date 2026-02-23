//! Intrinsic operation mapping for operator specialization.
//!
//! Maps known primitive operator SymbolIds to specialized LIR instructions
//! (BinOp, CmpOp, UnaryOp) so the lowerer can emit them directly instead
//! of generic LoadGlobal + Call sequences.

use super::types::{BinOp, CmpOp, UnaryOp};
use crate::symbol::SymbolTable;
use crate::value::SymbolId;
use rustc_hash::FxHashMap;

/// A known intrinsic operation that can be compiled to specialized instructions.
#[derive(Debug, Clone, Copy)]
pub enum IntrinsicOp {
    Binary(BinOp),
    Compare(CmpOp),
    Unary(UnaryOp),
}

/// Build the intrinsics map from a symbol table.
///
/// Maps SymbolId to IntrinsicOp for known primitive operations.
/// Only includes operators that are registered as global primitives
/// and whose semantics match the corresponding LIR instruction exactly.
pub fn build_intrinsics(symbols: &SymbolTable) -> FxHashMap<SymbolId, IntrinsicOp> {
    let mut map = FxHashMap::default();

    let mut add = |name: &str, op: IntrinsicOp| {
        if let Some(id) = symbols.get(name) {
            map.insert(id, op);
        }
    };

    // Binary arithmetic
    add("+", IntrinsicOp::Binary(BinOp::Add));
    add("-", IntrinsicOp::Binary(BinOp::Sub));
    add("*", IntrinsicOp::Binary(BinOp::Mul));
    add("/", IntrinsicOp::Binary(BinOp::Div));
    // `rem` uses truncated remainder, matching BinOp::Rem / Instruction::Rem.
    // `%` is Euclidean modulo (different for negative numbers) — not mapped.
    add("rem", IntrinsicOp::Binary(BinOp::Rem));

    // Comparisons
    add("=", IntrinsicOp::Compare(CmpOp::Eq));
    add("<", IntrinsicOp::Compare(CmpOp::Lt));
    add(">", IntrinsicOp::Compare(CmpOp::Gt));
    add("<=", IntrinsicOp::Compare(CmpOp::Le));
    add(">=", IntrinsicOp::Compare(CmpOp::Ge));

    // Unary
    // `-` with 1 arg is handled as a special case in try_lower_intrinsic.
    add("not", IntrinsicOp::Unary(UnaryOp::Not));

    map
}
