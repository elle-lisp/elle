//! Making a LIR function shippable across a thread boundary.
//!
//! Split out because the two-pass ValueConst rewrite is a self-contained
//! concern, distinct from the value-tag serialization it delegates into.

use super::super::*;
use super::ctx::SerContext;
use super::from_value_inner;

/// Make a LIR function shippable across a thread boundary, returning the
/// compound-value pool (or `None` if the LIR must be dropped).
///
/// Two passes:
///   1. Lift each *compound* `ValueConst` operand (quoted list, struct, array,
///      …) into a serialized `lir_value_pool` entry and rewrite the instruction
///      to `Const(ValueRef(idx))`. Serialization goes through `ctx`, so any
///      closures nested in the compound intern into the bundle correctly.
///   2. Delegate to `LirFunction::convert_value_consts_for_send`, which inlines
///      scalar operands and rewrites closure operands to `ClosureRef` (keeping
///      the `lir/closure-value-const-count` accounting). It returns `false` only
///      when a closure operand isn't in the intern table; we then drop the LIR
///      and the worker falls back to bytecode.
///
/// `patch_lir_value_refs` / `patch_lir_closure_refs` invert this on receipt.
pub(super) fn convert_lir_for_send(
    lir: &mut crate::lir::LirFunction,
    ctx: &mut SerContext<'_>,
) -> Result<Option<Vec<SendValue>>, String> {
    use crate::lir::{value_to_lir_const, LirConst, LirInstr};

    // Pass 1: compound ValueConsts → ValueRef into the pool.
    let mut pool: Vec<SendValue> = Vec::new();
    for block in &mut lir.blocks {
        for si in &mut block.instructions {
            let (dst, value) = match &si.instr {
                LirInstr::ValueConst { dst, value } => (*dst, *value),
                _ => continue,
            };
            // Leave scalars, closures, and native fns for pass 2 / as-is.
            if value.is_native_fn() || value.is_closure() || value_to_lir_const(value).is_some() {
                continue;
            }
            let sv = from_value_inner(value, ctx)?;
            let idx = pool.len();
            pool.push(sv);
            si.instr = LirInstr::Const {
                dst,
                value: LirConst::ValueRef(idx),
            };
        }
    }

    // Pass 2: scalars inline + closures → ClosureRef (or signal a drop).
    if lir.convert_value_consts_for_send(&ctx.visited) {
        Ok(Some(pool))
    } else {
        Ok(None)
    }
}
