//! Per-terminator MLIR emission (return / jump / conditional branch).
//!
//! Emits the `func.return` or `cf` branch that ends each block, resolving LIR
//! labels to the pre-built MLIR blocks. The op order matches the original
//! single-function lowering; `Return` also folds the block's scalar type into
//! `ctx.return_type`.

use super::*;

/// Lower `lir_block`'s terminator into `block`, updating `ctx.return_type`.
///
/// `blocks`/`label_to_idx` resolve jump and branch targets to the pre-built
/// MLIR blocks that `Value`s and branch edges point into.
pub(super) fn lower_terminator<'c, 'a>(
    ctx: &mut LowerCtx<'c, 'a>,
    block: &'a Block<'c>,
    blocks: &'a [Block<'c>],
    label_to_idx: &HashMap<Label, usize>,
    lir_block: &crate::lir::BasicBlock,
) -> Result<(), String> {
    let context = ctx.context;
    let location = ctx.location;
    let i64_type = ctx.i64_type;

    match &lir_block.terminator.terminator {
        Terminator::Return(reg) => {
            let val = *ctx
                .regs
                .get(reg)
                .ok_or_else(|| format!("undefined reg r{} in return", reg.0))?;
            let ret_type = ctx.types.get(reg).copied().unwrap_or(ScalarType::Int);
            // Function returns i64; bitcast f64 → i64 for float returns.
            // Bool is already i64 0/1 — no bitcast needed.
            let return_val = if ret_type.is_float() {
                let bc = block.append_operation(arith::bitcast(val, i64_type, location));
                bc.result(0).unwrap().into()
            } else {
                val
            };
            // Consistency check: Float vs non-Float is a real conflict.
            // Bool vs Int are both i64, so no conflict.
            if let Some(prev) = ctx.return_type {
                if prev.is_float() != ret_type.is_float() {
                    return Err("inconsistent return types across blocks".to_string());
                }
            }
            // Prefer Bool if any return is Bool (for correct reboxing).
            ctx.return_type = Some(match (ctx.return_type, ret_type) {
                (Some(ScalarType::Bool), _) | (_, ScalarType::Bool) => ScalarType::Bool,
                _ => ret_type,
            });
            block.append_operation(func::r#return(&[return_val], location));
        }
        Terminator::Jump(label) => {
            let target_idx = label_to_idx
                .get(label)
                .ok_or_else(|| format!("unknown label {}", label.0))?;
            block.append_operation(cf::br(&blocks[*target_idx], &[], location));
        }
        Terminator::Branch {
            cond,
            then_label,
            else_label,
        } => {
            let cond_i64 = *ctx
                .regs
                .get(cond)
                .ok_or_else(|| format!("undefined reg r{} in branch", cond.0))?;
            // Compare to zero for truthiness (0=false, nonzero=true).
            // trunci would take the LSB, giving wrong results for even
            // nonzero values (e.g. 2 → false).
            let zero = block.append_operation(arith::constant(
                context,
                IntegerAttribute::new(i64_type, 0).into(),
                location,
            ));
            let zero_val: Value = zero.result(0).unwrap().into();
            let cmp = block.append_operation(arith::cmpi(
                context,
                CmpiPredicate::Ne,
                cond_i64,
                zero_val,
                location,
            ));
            let cond_val: Value = cmp.result(0).unwrap().into();
            let then_idx = *label_to_idx
                .get(then_label)
                .ok_or_else(|| format!("unknown then label {}", then_label.0))?;
            let else_idx = *label_to_idx
                .get(else_label)
                .ok_or_else(|| format!("unknown else label {}", else_label.0))?;
            block.append_operation(cf::cond_br(
                context,
                cond_val,
                &blocks[then_idx],
                &blocks[else_idx],
                &[],
                &[],
                location,
            ));
        }
        _ => {
            return Err(format!(
                "unsupported terminator: {:?}",
                lir_block.terminator.terminator
            ))
        }
    }
    Ok(())
}
