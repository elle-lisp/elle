//! Inline integer fast paths for arithmetic and comparison operations.
//!
//! For each binary or comparison op, the JIT emits a diamond-shaped CFG:
//! tag check → fast path (native int op) / slow path (extern helper) → merge.
//! This avoids the overhead of a function call for the common integer case.
//!
//! With the 16-byte tagged-union Value:
//!   - TAG_INT = 0, so checking `tag == 0` is a single comparison
//!   - payload is the raw i64 value — NO masking or sign-extension needed
//!   - results: tag = 0 (TAG_INT), payload = result i64
//!   - booleans: tag = TAG_TRUE (3) or TAG_FALSE (4), payload = 0
//!
//! Fast and slow paths both produce TWO Cranelift values: (tag, payload).

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types::I64;
use cranelift_codegen::ir::InstBuilder;
use cranelift_frontend::FunctionBuilder;
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Module};

use crate::lir::{BinOp, CmpOp, UnaryOp};
use crate::value::repr::{TAG_FALSE, TAG_INT, TAG_TRUE};

use super::JitError;

/// Emit inline integer fast path for a binary arithmetic operation.
///
/// Generates a diamond CFG: tag check → fast block / slow block → merge.
/// For Div/Rem, an extra block checks for zero divisor.
///
/// All paths produce (tag_result, payload_result).
/// The merge block has TWO phi parameters: (tag, payload).
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_int_binop_fast_path(
    module: &mut JITModule,
    builder: &mut FunctionBuilder,
    op: BinOp,
    lhs_tag: cranelift_codegen::ir::Value,
    lhs_payload: cranelift_codegen::ir::Value,
    rhs_tag: cranelift_codegen::ir::Value,
    rhs_payload: cranelift_codegen::ir::Value,
    slow_path_func_id: FuncId,
) -> Result<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value), JitError> {
    let is_div_rem = matches!(op, BinOp::Div | BinOp::Rem);

    let int_check_block = if is_div_rem {
        Some(builder.create_block())
    } else {
        None
    };
    let fast_block = builder.create_block();
    let slow_block = builder.create_block();
    let merge_block = builder.create_block();

    // Merge block has two phi params: (tag, payload)
    builder.append_block_param(merge_block, I64); // tag
    builder.append_block_param(merge_block, I64); // payload

    // Tag check: both tags == TAG_INT (= 0)
    let zero = builder.ins().iconst(I64, 0);
    let a_is_int = builder.ins().icmp(IntCC::Equal, lhs_tag, zero);
    let b_is_int = builder.ins().icmp(IntCC::Equal, rhs_tag, zero);
    let both_int = builder.ins().band(a_is_int, b_is_int);

    if is_div_rem {
        let int_check = int_check_block.unwrap();
        builder
            .ins()
            .brif(both_int, int_check, &[], slow_block, &[]);

        builder.switch_to_block(int_check);
        builder.seal_block(int_check);
        // Divisor is rhs_payload (raw i64) — check for zero
        let b_nonzero = builder.ins().icmp(IntCC::NotEqual, rhs_payload, zero);
        builder
            .ins()
            .brif(b_nonzero, fast_block, &[], slow_block, &[]);
    } else {
        builder
            .ins()
            .brif(both_int, fast_block, &[], slow_block, &[]);
    }

    // Fast block: operate directly on payloads (already i64, no masking)
    builder.switch_to_block(fast_block);
    builder.seal_block(fast_block);

    let (fast_tag, fast_payload) = match op {
        BinOp::Add => {
            let raw = builder.ins().iadd(lhs_payload, rhs_payload);
            let tag = builder.ins().iconst(I64, TAG_INT as i64);
            (tag, raw)
        }
        BinOp::Sub => {
            let raw = builder.ins().isub(lhs_payload, rhs_payload);
            let tag = builder.ins().iconst(I64, TAG_INT as i64);
            (tag, raw)
        }
        BinOp::Mul => {
            let raw = builder.ins().imul(lhs_payload, rhs_payload);
            let tag = builder.ins().iconst(I64, TAG_INT as i64);
            (tag, raw)
        }
        BinOp::BitAnd => {
            let raw = builder.ins().band(lhs_payload, rhs_payload);
            let tag = builder.ins().iconst(I64, TAG_INT as i64);
            (tag, raw)
        }
        BinOp::BitOr => {
            let raw = builder.ins().bor(lhs_payload, rhs_payload);
            let tag = builder.ins().iconst(I64, TAG_INT as i64);
            (tag, raw)
        }
        BinOp::BitXor => {
            let raw = builder.ins().bxor(lhs_payload, rhs_payload);
            let tag = builder.ins().iconst(I64, TAG_INT as i64);
            (tag, raw)
        }
        BinOp::Shl => {
            let raw = builder.ins().ishl(lhs_payload, rhs_payload);
            let tag = builder.ins().iconst(I64, TAG_INT as i64);
            (tag, raw)
        }
        BinOp::Shr => {
            let raw = builder.ins().sshr(lhs_payload, rhs_payload);
            let tag = builder.ins().iconst(I64, TAG_INT as i64);
            (tag, raw)
        }
        BinOp::Div => {
            let raw = builder.ins().sdiv(lhs_payload, rhs_payload);
            let tag = builder.ins().iconst(I64, TAG_INT as i64);
            (tag, raw)
        }
        BinOp::Rem => {
            let raw = builder.ins().srem(lhs_payload, rhs_payload);
            let tag = builder.ins().iconst(I64, TAG_INT as i64);
            (tag, raw)
        }
    };
    builder.ins().jump(merge_block, &[fast_tag, fast_payload]);

    // Slow block: call runtime helper
    builder.switch_to_block(slow_block);
    builder.seal_block(slow_block);

    let func_ref = module.declare_func_in_func(slow_path_func_id, builder.func);
    let call = builder
        .ins()
        .call(func_ref, &[lhs_tag, lhs_payload, rhs_tag, rhs_payload]);
    let slow_tag = builder.inst_results(call)[0];
    let slow_payload = builder.inst_results(call)[1];
    builder.ins().jump(merge_block, &[slow_tag, slow_payload]);

    // Merge block
    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);

    let result_tag = builder.block_params(merge_block)[0];
    let result_payload = builder.block_params(merge_block)[1];
    Ok((result_tag, result_payload))
}

/// Emit inline integer fast path for a comparison operation.
///
/// Generates a diamond CFG: tag check → fast block / slow block → merge.
/// All paths produce (tag, payload) where tag is TAG_TRUE or TAG_FALSE, payload is 0.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_int_cmpop_fast_path(
    module: &mut JITModule,
    builder: &mut FunctionBuilder,
    op: CmpOp,
    lhs_tag: cranelift_codegen::ir::Value,
    lhs_payload: cranelift_codegen::ir::Value,
    rhs_tag: cranelift_codegen::ir::Value,
    rhs_payload: cranelift_codegen::ir::Value,
    slow_path_func_id: FuncId,
) -> Result<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value), JitError> {
    let fast_block = builder.create_block();
    let slow_block = builder.create_block();
    let merge_block = builder.create_block();

    builder.append_block_param(merge_block, I64); // tag
    builder.append_block_param(merge_block, I64); // payload

    // Tag check: both tags == 0 (TAG_INT)
    let zero = builder.ins().iconst(I64, 0);
    let a_is_int = builder.ins().icmp(IntCC::Equal, lhs_tag, zero);
    let b_is_int = builder.ins().icmp(IntCC::Equal, rhs_tag, zero);
    let both_int = builder.ins().band(a_is_int, b_is_int);
    builder
        .ins()
        .brif(both_int, fast_block, &[], slow_block, &[]);

    // Fast block: compare payloads directly (already i64)
    builder.switch_to_block(fast_block);
    builder.seal_block(fast_block);

    let tag_true = builder.ins().iconst(I64, TAG_TRUE as i64);
    let tag_false = builder.ins().iconst(I64, TAG_FALSE as i64);
    let zero_payload = builder.ins().iconst(I64, 0);

    let fast_tag = match op {
        CmpOp::Eq => {
            // For integers, tag AND payload must match — since both are TAG_INT,
            // we only need to compare payloads.
            let cmp = builder.ins().icmp(IntCC::Equal, lhs_payload, rhs_payload);
            builder.ins().select(cmp, tag_true, tag_false)
        }
        CmpOp::Ne => {
            let cmp = builder
                .ins()
                .icmp(IntCC::NotEqual, lhs_payload, rhs_payload);
            builder.ins().select(cmp, tag_true, tag_false)
        }
        CmpOp::Lt => {
            let cmp = builder
                .ins()
                .icmp(IntCC::SignedLessThan, lhs_payload, rhs_payload);
            builder.ins().select(cmp, tag_true, tag_false)
        }
        CmpOp::Le => {
            let cmp = builder
                .ins()
                .icmp(IntCC::SignedLessThanOrEqual, lhs_payload, rhs_payload);
            builder.ins().select(cmp, tag_true, tag_false)
        }
        CmpOp::Gt => {
            let cmp = builder
                .ins()
                .icmp(IntCC::SignedGreaterThan, lhs_payload, rhs_payload);
            builder.ins().select(cmp, tag_true, tag_false)
        }
        CmpOp::Ge => {
            let cmp = builder
                .ins()
                .icmp(IntCC::SignedGreaterThanOrEqual, lhs_payload, rhs_payload);
            builder.ins().select(cmp, tag_true, tag_false)
        }
    };
    builder.ins().jump(merge_block, &[fast_tag, zero_payload]);

    // Slow block: call runtime helper
    builder.switch_to_block(slow_block);
    builder.seal_block(slow_block);

    let func_ref = module.declare_func_in_func(slow_path_func_id, builder.func);
    let call = builder
        .ins()
        .call(func_ref, &[lhs_tag, lhs_payload, rhs_tag, rhs_payload]);
    let slow_tag = builder.inst_results(call)[0];
    let slow_payload = builder.inst_results(call)[1];
    builder.ins().jump(merge_block, &[slow_tag, slow_payload]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);

    let result_tag = builder.block_params(merge_block)[0];
    let result_payload = builder.block_params(merge_block)[1];
    Ok((result_tag, result_payload))
}

/// Emit inline fast path for a unary operation.
///
/// - `Not`: Fully inlined — truthiness check works for all types, no slow path.
/// - `Neg`: Diamond with single-operand tag check, negate payload.
/// - `BitNot`: Diamond with single-operand tag check, bitwise NOT payload.
///
/// Returns (tag, payload).
pub(crate) fn emit_unary_fast_path(
    module: &mut JITModule,
    builder: &mut FunctionBuilder,
    op: UnaryOp,
    src_tag: cranelift_codegen::ir::Value,
    src_payload: cranelift_codegen::ir::Value,
    slow_path_func_id: FuncId,
) -> Result<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value), JitError> {
    match op {
        UnaryOp::Not => {
            // Fully inline — no diamond, no slow path.
            // Truthiness: value is falsy iff tag == TAG_NIL (2) or tag == TAG_FALSE (4).
            let tag_nil = builder
                .ins()
                .iconst(I64, crate::value::repr::TAG_NIL as i64);
            let tag_false_v = builder.ins().iconst(I64, TAG_FALSE as i64);
            let is_nil = builder.ins().icmp(IntCC::Equal, src_tag, tag_nil);
            let is_false = builder.ins().icmp(IntCC::Equal, src_tag, tag_false_v);
            let is_falsy = builder.ins().bor(is_nil, is_false);

            let tag_true = builder.ins().iconst(I64, TAG_TRUE as i64);
            let tag_false = builder.ins().iconst(I64, TAG_FALSE as i64);
            let zero_payload = builder.ins().iconst(I64, 0);
            let result_tag = builder.ins().select(is_falsy, tag_true, tag_false);
            Ok((result_tag, zero_payload))
        }
        UnaryOp::Neg | UnaryOp::BitNot => {
            let fast_block = builder.create_block();
            let slow_block = builder.create_block();
            let merge_block = builder.create_block();

            builder.append_block_param(merge_block, I64); // tag
            builder.append_block_param(merge_block, I64); // payload

            // Tag check: src_tag == 0 (TAG_INT)
            let zero = builder.ins().iconst(I64, 0);
            let is_int = builder.ins().icmp(IntCC::Equal, src_tag, zero);
            builder.ins().brif(is_int, fast_block, &[], slow_block, &[]);

            builder.switch_to_block(fast_block);
            builder.seal_block(fast_block);

            let (fast_tag, fast_payload) = match op {
                UnaryOp::Neg => {
                    // Negate the payload (raw i64) directly
                    let negated = builder.ins().ineg(src_payload);
                    let tag = builder.ins().iconst(I64, TAG_INT as i64);
                    (tag, negated)
                }
                UnaryOp::BitNot => {
                    // Bitwise NOT on the i64 payload
                    let notted = builder.ins().bnot(src_payload);
                    let tag = builder.ins().iconst(I64, TAG_INT as i64);
                    (tag, notted)
                }
                UnaryOp::Not => unreachable!(),
            };
            builder.ins().jump(merge_block, &[fast_tag, fast_payload]);

            builder.switch_to_block(slow_block);
            builder.seal_block(slow_block);

            let func_ref = module.declare_func_in_func(slow_path_func_id, builder.func);
            let call = builder.ins().call(func_ref, &[src_tag, src_payload]);
            let slow_tag = builder.inst_results(call)[0];
            let slow_payload = builder.inst_results(call)[1];
            builder.ins().jump(merge_block, &[slow_tag, slow_payload]);

            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);

            let result_tag = builder.block_params(merge_block)[0];
            let result_payload = builder.block_params(merge_block)[1];
            Ok((result_tag, result_payload))
        }
    }
}
