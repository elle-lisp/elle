//! Inline integer fast paths for arithmetic and comparison operations.
//!
//! For each binary or comparison op, the JIT emits a diamond-shaped CFG:
//! tag check → fast path (native int op) / slow path (extern helper) → merge.
//! This avoids the overhead of a function call for the common integer case.

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types::I64;
use cranelift_codegen::ir::InstBuilder;
use cranelift_frontend::FunctionBuilder;
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Module};

use crate::lir::{BinOp, CmpOp, UnaryOp};
use crate::value::repr::{PAYLOAD_MASK, TAG_FALSE, TAG_INT, TAG_INT_MASK, TAG_TRUE};

use super::JitError;

/// Emit inline integer fast path for a binary arithmetic operation.
///
/// Generates a diamond CFG: tag check → fast block / slow block → merge.
/// For Div/Rem, an extra block checks for zero divisor.
pub(crate) fn emit_int_binop_fast_path(
    module: &mut JITModule,
    builder: &mut FunctionBuilder,
    op: BinOp,
    lhs: cranelift_codegen::ir::Value,
    rhs: cranelift_codegen::ir::Value,
    slow_path_func_id: FuncId,
) -> Result<cranelift_codegen::ir::Value, JitError> {
    let is_div_rem = matches!(op, BinOp::Div | BinOp::Rem);

    // Create blocks
    let int_check_block = if is_div_rem {
        Some(builder.create_block())
    } else {
        None
    };
    let fast_block = builder.create_block();
    let slow_block = builder.create_block();
    let merge_block = builder.create_block();

    // Add phi parameter to merge block
    builder.append_block_param(merge_block, I64);

    // Emit tag check in current block
    let tag_mask = builder.ins().iconst(I64, TAG_INT_MASK as i64);
    let tag_int = builder.ins().iconst(I64, TAG_INT as i64);
    let a_tag = builder.ins().band(lhs, tag_mask);
    let b_tag = builder.ins().band(rhs, tag_mask);
    let a_is_int = builder.ins().icmp(IntCC::Equal, a_tag, tag_int);
    let b_is_int = builder.ins().icmp(IntCC::Equal, b_tag, tag_int);
    let both_int = builder.ins().band(a_is_int, b_is_int);

    if is_div_rem {
        let int_check = int_check_block.unwrap();
        builder
            .ins()
            .brif(both_int, int_check, &[], slow_block, &[]);

        // Int check block: verify divisor is non-zero
        builder.switch_to_block(int_check);
        builder.seal_block(int_check); // one predecessor
        let payload_mask = builder.ins().iconst(I64, PAYLOAD_MASK as i64);
        let b_pay = builder.ins().band(rhs, payload_mask);
        let zero = builder.ins().iconst(I64, 0);
        let b_nonzero = builder.ins().icmp(IntCC::NotEqual, b_pay, zero);
        builder
            .ins()
            .brif(b_nonzero, fast_block, &[], slow_block, &[]);
    } else {
        builder
            .ins()
            .brif(both_int, fast_block, &[], slow_block, &[]);
    }

    // Fast block
    builder.switch_to_block(fast_block);
    builder.seal_block(fast_block); // one predecessor

    let fast_result = match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul => {
            let payload_mask = builder.ins().iconst(I64, PAYLOAD_MASK as i64);
            let a_pay = builder.ins().band(lhs, payload_mask);
            let b_pay = builder.ins().band(rhs, payload_mask);
            let raw = match op {
                BinOp::Add => builder.ins().iadd(a_pay, b_pay),
                BinOp::Sub => builder.ins().isub(a_pay, b_pay),
                BinOp::Mul => builder.ins().imul(a_pay, b_pay),
                _ => unreachable!(),
            };
            let truncated = builder.ins().band(raw, payload_mask);
            let tag = builder.ins().iconst(I64, TAG_INT as i64);
            builder.ins().bor(tag, truncated)
        }
        BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => {
            let payload_mask = builder.ins().iconst(I64, PAYLOAD_MASK as i64);
            let a_pay = builder.ins().band(lhs, payload_mask);
            let b_pay = builder.ins().band(rhs, payload_mask);
            let raw = match op {
                BinOp::BitAnd => builder.ins().band(a_pay, b_pay),
                BinOp::BitOr => builder.ins().bor(a_pay, b_pay),
                BinOp::BitXor => builder.ins().bxor(a_pay, b_pay),
                _ => unreachable!(),
            };
            let tag = builder.ins().iconst(I64, TAG_INT as i64);
            builder.ins().bor(tag, raw)
        }
        BinOp::Shl => {
            let payload_mask = builder.ins().iconst(I64, PAYLOAD_MASK as i64);
            // Sign-extend value
            let a_raw = builder.ins().band(lhs, payload_mask);
            let sixteen = builder.ins().iconst(I64, 16);
            let a_shifted = builder.ins().ishl(a_raw, sixteen);
            let a_signed = builder.ins().sshr(a_shifted, sixteen);
            // Shift amount
            let b_pay = builder.ins().band(rhs, payload_mask);
            let raw = builder.ins().ishl(a_signed, b_pay);
            let truncated = builder.ins().band(raw, payload_mask);
            let tag = builder.ins().iconst(I64, TAG_INT as i64);
            builder.ins().bor(tag, truncated)
        }
        BinOp::Shr => {
            let payload_mask = builder.ins().iconst(I64, PAYLOAD_MASK as i64);
            // Sign-extend value
            let a_raw = builder.ins().band(lhs, payload_mask);
            let sixteen = builder.ins().iconst(I64, 16);
            let a_shifted = builder.ins().ishl(a_raw, sixteen);
            let a_signed = builder.ins().sshr(a_shifted, sixteen);
            // Shift amount
            let b_pay = builder.ins().band(rhs, payload_mask);
            let raw = builder.ins().sshr(a_signed, b_pay);
            let truncated = builder.ins().band(raw, payload_mask);
            let tag = builder.ins().iconst(I64, TAG_INT as i64);
            builder.ins().bor(tag, truncated)
        }
        BinOp::Div | BinOp::Rem => {
            let payload_mask = builder.ins().iconst(I64, PAYLOAD_MASK as i64);
            // Sign-extend both for signed division
            let a_raw = builder.ins().band(lhs, payload_mask);
            let sixteen = builder.ins().iconst(I64, 16);
            let a_shifted = builder.ins().ishl(a_raw, sixteen);
            let a_signed = builder.ins().sshr(a_shifted, sixteen);
            // Re-extract rhs payload (can't use value from different block)
            let b_raw = builder.ins().band(rhs, payload_mask);
            let b_shifted = builder.ins().ishl(b_raw, sixteen);
            let b_signed = builder.ins().sshr(b_shifted, sixteen);
            let raw = match op {
                BinOp::Div => builder.ins().sdiv(a_signed, b_signed),
                BinOp::Rem => builder.ins().srem(a_signed, b_signed),
                _ => unreachable!(),
            };
            let truncated = builder.ins().band(raw, payload_mask);
            let tag = builder.ins().iconst(I64, TAG_INT as i64);
            builder.ins().bor(tag, truncated)
        }
    };
    builder.ins().jump(merge_block, &[fast_result]);

    // Slow block
    builder.switch_to_block(slow_block);
    // Seal slow_block: for div/rem it has two predecessors (tag check + zero check),
    // for others it has one predecessor. Both are emitted by this point.
    builder.seal_block(slow_block);

    let func_ref = module.declare_func_in_func(slow_path_func_id, builder.func);
    let call = builder.ins().call(func_ref, &[lhs, rhs]);
    let slow_result = builder.inst_results(call)[0];
    builder.ins().jump(merge_block, &[slow_result]);

    // Merge block
    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);

    Ok(builder.block_params(merge_block)[0])
}

/// Emit inline integer fast path for a comparison operation.
///
/// Generates a diamond CFG: tag check → fast block / slow block → merge.
/// Eq/Ne use bit equality; ordered comparisons sign-extend payloads.
pub(crate) fn emit_int_cmpop_fast_path(
    module: &mut JITModule,
    builder: &mut FunctionBuilder,
    op: CmpOp,
    lhs: cranelift_codegen::ir::Value,
    rhs: cranelift_codegen::ir::Value,
    slow_path_func_id: FuncId,
) -> Result<cranelift_codegen::ir::Value, JitError> {
    let fast_block = builder.create_block();
    let slow_block = builder.create_block();
    let merge_block = builder.create_block();

    // Add phi parameter to merge block
    builder.append_block_param(merge_block, I64);

    // Emit tag check in current block
    let tag_mask = builder.ins().iconst(I64, TAG_INT_MASK as i64);
    let tag_int = builder.ins().iconst(I64, TAG_INT as i64);
    let a_tag = builder.ins().band(lhs, tag_mask);
    let b_tag = builder.ins().band(rhs, tag_mask);
    let a_is_int = builder.ins().icmp(IntCC::Equal, a_tag, tag_int);
    let b_is_int = builder.ins().icmp(IntCC::Equal, b_tag, tag_int);
    let both_int = builder.ins().band(a_is_int, b_is_int);
    builder
        .ins()
        .brif(both_int, fast_block, &[], slow_block, &[]);

    // Fast block
    builder.switch_to_block(fast_block);
    builder.seal_block(fast_block);

    let tag_true = builder.ins().iconst(I64, TAG_TRUE as i64);
    let tag_false = builder.ins().iconst(I64, TAG_FALSE as i64);

    let fast_result = match op {
        CmpOp::Eq | CmpOp::Ne => {
            // Bit equality is correct for integers (same TAG_INT prefix)
            let cc = match op {
                CmpOp::Eq => IntCC::Equal,
                CmpOp::Ne => IntCC::NotEqual,
                _ => unreachable!(),
            };
            let cmp = builder.ins().icmp(cc, lhs, rhs);
            builder.ins().select(cmp, tag_true, tag_false)
        }
        CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge => {
            // Sign-extend both payloads for signed comparison
            let payload_mask = builder.ins().iconst(I64, PAYLOAD_MASK as i64);
            let a_raw = builder.ins().band(lhs, payload_mask);
            let sixteen = builder.ins().iconst(I64, 16);
            let a_shifted = builder.ins().ishl(a_raw, sixteen);
            let a_signed = builder.ins().sshr(a_shifted, sixteen);
            let b_raw = builder.ins().band(rhs, payload_mask);
            let b_shifted = builder.ins().ishl(b_raw, sixteen);
            let b_signed = builder.ins().sshr(b_shifted, sixteen);
            let cc = match op {
                CmpOp::Lt => IntCC::SignedLessThan,
                CmpOp::Le => IntCC::SignedLessThanOrEqual,
                CmpOp::Gt => IntCC::SignedGreaterThan,
                CmpOp::Ge => IntCC::SignedGreaterThanOrEqual,
                _ => unreachable!(),
            };
            let cmp = builder.ins().icmp(cc, a_signed, b_signed);
            builder.ins().select(cmp, tag_true, tag_false)
        }
    };
    builder.ins().jump(merge_block, &[fast_result]);

    // Slow block
    builder.switch_to_block(slow_block);
    builder.seal_block(slow_block);

    let func_ref = module.declare_func_in_func(slow_path_func_id, builder.func);
    let call = builder.ins().call(func_ref, &[lhs, rhs]);
    let slow_result = builder.inst_results(call)[0];
    builder.ins().jump(merge_block, &[slow_result]);

    // Merge block
    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);

    Ok(builder.block_params(merge_block)[0])
}

/// Emit inline fast path for a unary operation.
///
/// - `Not`: Fully inlined — truthiness check works for all types, no slow path.
/// - `Neg`: Diamond with single-operand tag check, sign-extend + negate.
/// - `BitNot`: Diamond with single-operand tag check, XOR payload with PAYLOAD_MASK.
pub(crate) fn emit_unary_fast_path(
    module: &mut JITModule,
    builder: &mut FunctionBuilder,
    op: UnaryOp,
    src: cranelift_codegen::ir::Value,
    slow_path_func_id: FuncId,
) -> Result<cranelift_codegen::ir::Value, JitError> {
    match op {
        UnaryOp::Not => {
            // Fully inline — no diamond, no slow path.
            // Truthiness: upper 16 bits == 0x7FF9 means falsy (nil or false).
            let forty_eight = builder.ins().iconst(I64, 48);
            let shifted = builder.ins().ushr(src, forty_eight);
            let falsy_tag = builder.ins().iconst(I64, 0x7FF9);
            let is_falsy = builder.ins().icmp(IntCC::Equal, shifted, falsy_tag);
            let tag_true = builder.ins().iconst(I64, TAG_TRUE as i64);
            let tag_false = builder.ins().iconst(I64, TAG_FALSE as i64);
            let result = builder.ins().select(is_falsy, tag_true, tag_false);
            Ok(result)
        }
        UnaryOp::Neg | UnaryOp::BitNot => {
            // Diamond: single-operand tag check → fast/slow → merge
            let fast_block = builder.create_block();
            let slow_block = builder.create_block();
            let merge_block = builder.create_block();

            builder.append_block_param(merge_block, I64);

            // Tag check
            let tag_mask = builder.ins().iconst(I64, TAG_INT_MASK as i64);
            let tag_int = builder.ins().iconst(I64, TAG_INT as i64);
            let a_tag = builder.ins().band(src, tag_mask);
            let is_int = builder.ins().icmp(IntCC::Equal, a_tag, tag_int);
            builder.ins().brif(is_int, fast_block, &[], slow_block, &[]);

            // Fast block
            builder.switch_to_block(fast_block);
            builder.seal_block(fast_block); // one predecessor

            let fast_result = match op {
                UnaryOp::Neg => {
                    // Sign-extend payload, negate, truncate, re-tag
                    let payload_mask = builder.ins().iconst(I64, PAYLOAD_MASK as i64);
                    let raw = builder.ins().band(src, payload_mask);
                    let sixteen = builder.ins().iconst(I64, 16);
                    let shifted = builder.ins().ishl(raw, sixteen);
                    let signed = builder.ins().sshr(shifted, sixteen);
                    let negated = builder.ins().ineg(signed);
                    let truncated = builder.ins().band(negated, payload_mask);
                    let tag = builder.ins().iconst(I64, TAG_INT as i64);
                    builder.ins().bor(tag, truncated)
                }
                UnaryOp::BitNot => {
                    // XOR payload with PAYLOAD_MASK flips all 48 payload bits
                    let payload_mask = builder.ins().iconst(I64, PAYLOAD_MASK as i64);
                    let payload = builder.ins().band(src, payload_mask);
                    let flipped = builder.ins().bxor(payload, payload_mask);
                    let tag = builder.ins().iconst(I64, TAG_INT as i64);
                    builder.ins().bor(tag, flipped)
                }
                UnaryOp::Not => unreachable!(),
            };
            builder.ins().jump(merge_block, &[fast_result]);

            // Slow block
            builder.switch_to_block(slow_block);
            builder.seal_block(slow_block); // one predecessor

            let func_ref = module.declare_func_in_func(slow_path_func_id, builder.func);
            let call = builder.ins().call(func_ref, &[src]);
            let slow_result = builder.inst_results(call)[0];
            builder.ins().jump(merge_block, &[slow_result]);

            // Merge block
            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);

            Ok(builder.block_params(merge_block)[0])
        }
    }
}
