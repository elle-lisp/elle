// audited: 2026-11-24
//! A non-tail call to the running closure with immediate arguments: whether a
//! call is admitted, the guards it runs, and the direct call.
//!
//! docs/impl/jit-selfcall.md

use std::collections::HashSet;
use std::mem::offset_of;

use cranelift_codegen::ir::Value as ClifValue;

use super::*;
use crate::jit::value::TAIL_CALL_SENTINEL_JV;
use crate::lir::{LirView, Offers};
use crate::value::{Arity, Closure, Value};
use crate::vm::native_stack::{StackFloor, COMPILED_CALL_RESERVE};
use crate::vm::VM;

/// A closure object as `HeapObject`'s `#[repr(u64)]` lays out its `Closure`
/// variant: the tag word, then the fields in order.
#[repr(C)]
struct ClosureObject {
    _tag: u64,
    closure: Closure,
    _traits: Value,
}

/// The byte offset of `Closure::squelch_mask` from the address a closure
/// value's payload holds.
pub(crate) const SQUELCH_IN_CLOSURE_OBJECT: i32 =
    (offset_of!(ClosureObject, closure) + offset_of!(Closure, squelch_mask)) as i32;

/// The byte offset of the running fiber's `call_depth` from the start of a
/// `VM`.
pub(crate) const CALL_DEPTH_IN_VM: i32 =
    (offset_of!(VM, fiber) + offset_of!(crate::value::Fiber, call_depth)) as i32;

/// The byte offset of `RuntimeConfig::max_depth` from the start of a `VM`.
pub(crate) const MAX_DEPTH_IN_VM: i32 =
    (offset_of!(VM, runtime_config) + offset_of!(crate::config::RuntimeConfig, max_depth)) as i32;

/// What admission reads off a function's LIR, gathered in one pass: the
/// registers a `LoadSelf` defines, the registers an immediate defines, and the
/// parameter count where the arity is exact. Each register is defined once.
pub(crate) struct SelfCalls {
    selves: HashSet<Reg>,
    immediates: HashSet<Reg>,
    exact: Option<usize>,
}

impl SelfCalls {
    /// Scan `lir`. A constant is an immediate, and so is the result of an
    /// operation over proven integers, which is an integer or a boolean.
    pub(crate) fn of(lir: &LirView<'_>) -> Self {
        let mut selves = HashSet::new();
        let mut immediates = HashSet::new();
        for node in lir.blocks().flat_map(|block| block.nodes()) {
            match node.instr() {
                InstrRef::LoadSelf { dst } => {
                    selves.insert(dst);
                }
                InstrRef::Const { dst, .. } => {
                    immediates.insert(dst);
                }
                InstrRef::BinOp { dst, proof, .. }
                | InstrRef::UnaryOp { dst, proof, .. }
                | InstrRef::Compare { dst, proof, .. }
                    if proof.is_int() =>
                {
                    immediates.insert(dst);
                }
                _ => {}
            }
        }
        let exact = match lir.arity() {
            Arity::Exact(n) => Some(n),
            _ => None,
        };
        SelfCalls {
            selves,
            immediates,
            exact,
        }
    }

    /// Whether a call of `func` on `args`, offering `moves`, is a direct
    /// self-call.
    pub(crate) fn admits(&self, func: Reg, args: &[Reg], moves: Offers<'_>) -> bool {
        self.selves.contains(&func)
            && self.exact == Some(args.len())
            && moves.is_empty()
            && args.iter().all(|arg| self.immediates.contains(arg))
    }
}

impl FunctionTranslator<'_> {
    /// Call this function's own code on the spilled arguments when the three
    /// guards pass, and `elle_jit_call` on the callee (`ft`, `fp`) when one
    /// fails. A tail call the callee leaves pending runs through
    /// `elle_jit_self_call_tail`. Answers the result; the builder is left on
    /// the block where the three paths meet.
    pub(super) fn emit_self_call(
        &mut self,
        builder: &mut FunctionBuilder,
        ft: ClifValue,
        fp: ClifValue,
        args_addr: ClifValue,
        nargs: ClifValue,
        region_id: ClifValue,
    ) -> Result<(ClifValue, ClifValue), JitError> {
        let missing = || JitError::InvalidLir("self-call outside a prologue".to_string());
        let vm = self.vm_ptr.ok_or_else(missing)?;
        let env = self.env_ptr.ok_or_else(missing)?;
        let (self_tag, self_payload) = self.self_tag_payload.ok_or_else(missing)?;
        let flags = MemFlagsData::trusted();

        let mask = builder
            .ins()
            .load(I64, flags, self_payload, SQUELCH_IN_CLOSURE_OBJECT);
        let unsquelched = builder.ins().icmp_imm_u(IntCC::Equal, mask, 0);
        let sp = builder.ins().get_stack_pointer(I64);
        let floor = builder
            .ins()
            .load(I64, flags, vm, StackFloor::WORD_OFFSET_IN_VM as i32);
        let room = builder.ins().isub(sp, floor);
        let reserve = COMPILED_CALL_RESERVE as i64;
        let roomy = builder
            .ins()
            .icmp_imm_u(IntCC::UnsignedGreaterThanOrEqual, room, reserve);
        let depth = builder.ins().load(I64, flags, vm, CALL_DEPTH_IN_VM);
        let cap = builder.ins().load(I64, flags, vm, MAX_DEPTH_IN_VM);
        let deeper = builder.ins().iadd_imm_s(depth, 1);
        let under = builder
            .ins()
            .icmp(IntCC::UnsignedLessThanOrEqual, deeper, cap);
        let passes = builder.ins().band(unsquelched, roomy);
        let passes = builder.ins().band(passes, under);

        let direct = builder.create_block();
        let pending = builder.create_block();
        let dispatch = builder.create_block();
        let merge = builder.create_block();
        builder.append_block_param(merge, I64);
        builder.append_block_param(merge, I64);
        builder.set_cold_block(pending);
        builder.set_cold_block(dispatch);
        builder.ins().brif(passes, direct, &[], dispatch, &[]);

        builder.switch_to_block(direct);
        builder.seal_block(direct);
        builder.ins().store(flags, deeper, vm, CALL_DEPTH_IN_VM);
        let own = self.module.declare_func_in_func(self.own, builder.func);
        let call = builder
            .ins()
            .call(own, &[env, args_addr, nargs, vm, self_tag, self_payload]);
        let (rt, rp) = (builder.inst_results(call)[0], builder.inst_results(call)[1]);
        let depth = builder.ins().load(I64, flags, vm, CALL_DEPTH_IN_VM);
        let shallower = builder.ins().iadd_imm_s(depth, -1);
        builder.ins().store(flags, shallower, vm, CALL_DEPTH_IN_VM);
        let sentinel = TAIL_CALL_SENTINEL_JV.tag as i64;
        let is_tail = builder.ins().icmp_imm_u(IntCC::Equal, rt, sentinel);
        let result = [BlockArg::Value(rt), BlockArg::Value(rp)];
        builder.ins().brif(is_tail, pending, &[], merge, &result);

        builder.switch_to_block(pending);
        builder.seal_block(pending);
        let tail = self
            .module
            .declare_func_in_func(self.helpers.self_call_tail, builder.func);
        let call = builder.ins().call(tail, &[self_tag, self_payload, vm]);
        let (tt, tp) = (builder.inst_results(call)[0], builder.inst_results(call)[1]);
        builder
            .ins()
            .jump(merge, &[BlockArg::Value(tt), BlockArg::Value(tp)]);

        builder.switch_to_block(dispatch);
        builder.seal_block(dispatch);
        let no_offer = builder.ins().iconst(I64, 0);
        let (dt, dp) =
            self.call_helper_call(builder, ft, fp, args_addr, nargs, vm, region_id, no_offer)?;
        builder
            .ins()
            .jump(merge, &[BlockArg::Value(dt), BlockArg::Value(dp)]);

        builder.switch_to_block(merge);
        builder.seal_block(merge);
        let params = builder.block_params(merge);
        Ok((params[0], params[1]))
    }
}
