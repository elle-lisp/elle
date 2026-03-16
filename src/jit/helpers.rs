//! Helper methods for FunctionTranslator (call emission, fast paths, exception checks)

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types::I64;
use cranelift_codegen::ir::InstBuilder;
use cranelift_frontend::FunctionBuilder;
use cranelift_module::{FuncId, Module};

use crate::lir::{BinOp, CmpOp, LirConst, UnaryOp};
use crate::value::repr::{PAYLOAD_MASK, TAG_EMPTY_LIST, TAG_FALSE, TAG_INT, TAG_NIL, TAG_TRUE};
use crate::value::SymbolId;

use super::translate::FunctionTranslator;
use super::JitError;

/// Helper to create a Variable from a register/slot index
#[inline]
fn var(n: u32) -> cranelift_frontend::Variable {
    cranelift_frontend::Variable::from_u32(n)
}

impl<'a> FunctionTranslator<'a> {
    /// Translate a constant to a Cranelift value
    pub(crate) fn translate_const(
        &self,
        builder: &mut FunctionBuilder,
        value: &LirConst,
    ) -> cranelift_codegen::ir::Value {
        let bits = match value {
            LirConst::Nil => TAG_NIL,
            LirConst::EmptyList => TAG_EMPTY_LIST,
            LirConst::Bool(true) => TAG_TRUE,
            LirConst::Bool(false) => TAG_FALSE,
            LirConst::Int(n) => TAG_INT | ((*n as u64) & PAYLOAD_MASK),
            LirConst::Float(f) => {
                // Use Value::float to handle NaN-boxing correctly
                crate::value::Value::float(*f).to_bits()
            }
            LirConst::String(s) => {
                // Create the heap-allocated string Value at compile time
                // and embed its NaN-boxed bits as a constant.
                // The string lives on the Rc heap and will be kept alive
                // by the LirFunction's constant pool.
                crate::value::Value::string(s.clone()).to_bits()
            }
            LirConst::Symbol(id) => crate::value::Value::symbol(id.0).to_bits(),
            LirConst::Keyword(name) => crate::value::Value::keyword(name).to_bits(),
        };
        builder.ins().iconst(I64, bits as i64)
    }

    /// Call a binary runtime helper with inline integer fast path
    pub(crate) fn call_binary_helper(
        &mut self,
        builder: &mut FunctionBuilder,
        op: BinOp,
        lhs: cranelift_codegen::ir::Value,
        rhs: cranelift_codegen::ir::Value,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let func_id = match op {
            BinOp::Add => self.helpers.add,
            BinOp::Sub => self.helpers.sub,
            BinOp::Mul => self.helpers.mul,
            BinOp::Div => self.helpers.div,
            BinOp::Rem => self.helpers.rem,
            BinOp::BitAnd => self.helpers.bit_and,
            BinOp::BitOr => self.helpers.bit_or,
            BinOp::BitXor => self.helpers.bit_xor,
            BinOp::Shl => self.helpers.shl,
            BinOp::Shr => self.helpers.shr,
        };
        super::fastpath::emit_int_binop_fast_path(self.module, builder, op, lhs, rhs, func_id)
    }

    /// Call a unary runtime helper with inline fast path
    pub(crate) fn call_unary_helper(
        &mut self,
        builder: &mut FunctionBuilder,
        op: UnaryOp,
        src: cranelift_codegen::ir::Value,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let func_id = match op {
            UnaryOp::Neg => self.helpers.neg,
            UnaryOp::Not => self.helpers.not,
            UnaryOp::BitNot => self.helpers.bit_not,
        };
        super::fastpath::emit_unary_fast_path(self.module, builder, op, src, func_id)
    }

    /// Call a comparison runtime helper with inline integer fast path
    pub(crate) fn call_compare_helper(
        &mut self,
        builder: &mut FunctionBuilder,
        op: CmpOp,
        lhs: cranelift_codegen::ir::Value,
        rhs: cranelift_codegen::ir::Value,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let func_id = match op {
            CmpOp::Eq => self.helpers.eq,
            CmpOp::Ne => self.helpers.ne,
            CmpOp::Lt => self.helpers.lt,
            CmpOp::Le => self.helpers.le,
            CmpOp::Gt => self.helpers.gt,
            CmpOp::Ge => self.helpers.ge,
        };
        super::fastpath::emit_int_cmpop_fast_path(self.module, builder, op, lhs, rhs, func_id)
    }

    /// Call a binary helper function
    pub(crate) fn call_helper_binary(
        &mut self,
        builder: &mut FunctionBuilder,
        func_id: FuncId,
        a: cranelift_codegen::ir::Value,
        b: cranelift_codegen::ir::Value,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let func_ref = self.module.declare_func_in_func(func_id, builder.func);
        let call = builder.ins().call(func_ref, &[a, b]);
        Ok(builder.inst_results(call)[0])
    }

    /// Call a unary helper function
    pub(crate) fn call_helper_unary(
        &mut self,
        builder: &mut FunctionBuilder,
        func_id: FuncId,
        a: cranelift_codegen::ir::Value,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let func_ref = self.module.declare_func_in_func(func_id, builder.func);
        let call = builder.ins().call(func_ref, &[a]);
        Ok(builder.inst_results(call)[0])
    }

    /// Call a quaternary helper function (4 args)
    pub(crate) fn call_helper_quaternary(
        &mut self,
        builder: &mut FunctionBuilder,
        func_id: FuncId,
        a: cranelift_codegen::ir::Value,
        b: cranelift_codegen::ir::Value,
        c: cranelift_codegen::ir::Value,
        d: cranelift_codegen::ir::Value,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let func_ref = self.module.declare_func_in_func(func_id, builder.func);
        let call = builder.ins().call(func_ref, &[a, b, c, d]);
        Ok(builder.inst_results(call)[0])
    }

    /// Call a ternary helper function
    pub(crate) fn call_helper_ternary(
        &mut self,
        builder: &mut FunctionBuilder,
        func_id: FuncId,
        a: cranelift_codegen::ir::Value,
        b: cranelift_codegen::ir::Value,
        c: cranelift_codegen::ir::Value,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let func_ref = self.module.declare_func_in_func(func_id, builder.func);
        let call = builder.ins().call(func_ref, &[a, b, c]);
        Ok(builder.inst_results(call)[0])
    }

    /// Call the elle_jit_call helper (4 args: func, args_ptr, nargs, vm)
    pub(crate) fn call_helper_call(
        &mut self,
        builder: &mut FunctionBuilder,
        func: cranelift_codegen::ir::Value,
        args_ptr: cranelift_codegen::ir::Value,
        nargs: cranelift_codegen::ir::Value,
        vm: cranelift_codegen::ir::Value,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let func_ref = self
            .module
            .declare_func_in_func(self.helpers.call, builder.func);
        let call = builder.ins().call(func_ref, &[func, args_ptr, nargs, vm]);
        Ok(builder.inst_results(call)[0])
    }

    /// Call the elle_jit_tail_call helper (4 args: func, args_ptr, nargs, vm)
    pub(crate) fn call_helper_tail_call(
        &mut self,
        builder: &mut FunctionBuilder,
        func: cranelift_codegen::ir::Value,
        args_ptr: cranelift_codegen::ir::Value,
        nargs: cranelift_codegen::ir::Value,
        vm: cranelift_codegen::ir::Value,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let func_ref = self
            .module
            .declare_func_in_func(self.helpers.tail_call, builder.func);
        let call = builder.ins().call(func_ref, &[func, args_ptr, nargs, vm]);
        Ok(builder.inst_results(call)[0])
    }

    /// Emit a direct call to an SCC peer function.
    ///
    /// Uses the standard JIT calling convention:
    /// `fn(env, args_ptr, nargs, vm, self_bits) -> Value`
    ///
    /// For Phase 1, we pass null env (capture-free functions only) and
    /// 0 for self_bits. This has two consequences:
    ///
    /// 1. **Self-tail-call degradation**: When peer A calls peer B directly,
    ///    B receives `self_bits = 0`. If B is also self-recursive, its
    ///    self-tail-call optimization won't fire (the comparison against
    ///    `self_bits` always fails). B's self-recursion goes through
    ///    `elle_jit_tail_call` instead of becoming a native loop. This
    ///    means batch-compiled self-recursive functions are *slower* on
    ///    the peer-called path than solo-compiled ones. Phase 2 should
    ///    pass the peer's actual closure bits as `self_bits`.
    ///
    /// 2. **No mutual tail-call elimination**: Direct SCC calls in tail
    ///    position use `call + return`, not jumps. Deep mutual tail
    ///    recursion between peers grows the native stack and will segfault
    ///    rather than producing a clean stack overflow error. Phase 2
    ///    should implement function fusion for true mutual TCE.
    pub(crate) fn emit_direct_scc_call(
        &mut self,
        builder: &mut FunctionBuilder,
        peer_func_id: FuncId,
        target_sym: SymbolId,
        args: &[crate::lir::Reg],
        vm: cranelift_codegen::ir::Value,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let func_ref = self.module.declare_func_in_func(peer_func_id, builder.func);

        // Build args on stack (same layout as standard Call)
        let (args_ptr, nargs) = if args.is_empty() {
            let null = builder.ins().iconst(I64, 0);
            let zero = builder.ins().iconst(I64, 0);
            (null, zero)
        } else {
            let slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                (args.len() * 8) as u32,
                0,
            ));
            for (i, arg_reg) in args.iter().enumerate() {
                let arg_val = builder.use_var(var(arg_reg.0));
                builder.ins().stack_store(arg_val, slot, (i * 8) as i32);
            }
            let addr = builder.ins().stack_addr(I64, slot, 0);
            let count = builder.ins().iconst(I64, args.len() as i64);
            (addr, count)
        };

        // Null env for capture-free functions
        let null_env = builder.ins().iconst(I64, 0);
        // When calling ourselves, pass our self_bits so the callee can detect
        // self-tail-calls and jump to loop_header. For other SCC peers, pass 0.
        let call_self_bits = if self.self_sym == Some(target_sym) {
            self.self_bits
                .unwrap_or_else(|| builder.ins().iconst(I64, 0))
        } else {
            builder.ins().iconst(I64, 0)
        };

        let call = builder
            .ins()
            .call(func_ref, &[null_env, args_ptr, nargs, vm, call_self_bits]);
        Ok(builder.inst_results(call)[0])
    }

    /// Emit exception check after a call instruction.
    /// If an exception is pending, return NIL immediately to bail out to the
    /// interpreter's exception handling.
    pub(crate) fn emit_exception_check_after_call(
        &mut self,
        builder: &mut FunctionBuilder,
    ) -> Result<(), JitError> {
        let vm = self.vm_ptr.ok_or_else(|| {
            JitError::InvalidLir("emit_exception_check without vm pointer".to_string())
        })?;

        // Call has_exception helper
        let has_exc = self.call_helper_unary(builder, self.helpers.has_exception, vm)?;

        // Check truthiness: (value >> 48) != 0x7FF9
        let shifted = builder.ins().ushr_imm(has_exc, 48);
        let falsy_tag = builder.ins().iconst(I64, 0x7FF9_i64);
        let is_truthy = builder.ins().icmp(IntCC::NotEqual, shifted, falsy_tag);

        // Create exception return block and continue block
        let exc_block = builder.create_block();
        let cont_block = builder.create_block();
        builder
            .ins()
            .brif(is_truthy, exc_block, &[], cont_block, &[]);

        // Exception block: return NIL to bail out
        builder.switch_to_block(exc_block);
        builder.seal_block(exc_block);
        let nil = builder.ins().iconst(I64, TAG_NIL as i64);
        builder.ins().return_(&[nil]);

        // Continue block: normal execution continues
        builder.switch_to_block(cont_block);
        builder.seal_block(cont_block);

        Ok(())
    }

    /// Emit yield check after a call instruction for yielding functions.
    ///
    /// Must be called AFTER emit_exception_check_after_call (which handles
    /// error/halt by returning NIL). If we reach this check, the signal
    /// is either absent or SIG_YIELD.
    ///
    /// On yield: spill live registers, call elle_jit_yield_through_call
    /// to build the caller's SuspendedFrame, return YIELD_SENTINEL.
    pub(crate) fn emit_yield_check_after_call(
        &mut self,
        builder: &mut FunctionBuilder,
        call_site_idx: u32,
    ) -> Result<(), JitError> {
        let vm = self.vm_ptr.ok_or_else(|| {
            JitError::InvalidLir("emit_yield_check without vm pointer".to_string())
        })?;
        let self_bits = self.self_bits.ok_or_else(|| {
            JitError::InvalidLir("emit_yield_check without self_bits".to_string())
        })?;

        // Check if any signal is pending (error/halt already handled above,
        // so if truthy here it must be SIG_YIELD)
        let has_sig = self.call_helper_unary(builder, self.helpers.has_signal, vm)?;
        let shifted = builder.ins().ushr_imm(has_sig, 48);
        let falsy_tag = builder.ins().iconst(I64, 0x7FF9_i64);
        let is_truthy = builder.ins().icmp(IntCC::NotEqual, shifted, falsy_tag);

        let yield_block = builder.create_block();
        let cont_block = builder.create_block();
        builder
            .ins()
            .brif(is_truthy, yield_block, &[], cont_block, &[]);

        // Yield block: spill registers, call yield_through_call, return sentinel
        builder.switch_to_block(yield_block);
        builder.seal_block(yield_block);

        // Read call site metadata from LIR
        let call_site = self.lir.call_sites.get(call_site_idx as usize);
        let stack_regs = match call_site {
            Some(cs) => cs.stack_regs.as_slice(),
            None => &[] as &[crate::lir::Reg],
        };

        // Spill locals + operand stack to match interpreter layout
        let spilled_ptr = self.spill_locals_and_operands(builder, stack_regs)?;

        let call_site_idx_val = builder.ins().iconst(I64, call_site_idx as i64);

        // Call elle_jit_yield_through_call(spilled, call_site_index, vm, self_bits)
        let func_ref = self
            .module
            .declare_func_in_func(self.helpers.jit_yield_through_call, builder.func);
        let call = builder
            .ins()
            .call(func_ref, &[spilled_ptr, call_site_idx_val, vm, self_bits]);
        let result = builder.inst_results(call)[0];
        builder.ins().return_(&[result]);

        // Continue block
        builder.switch_to_block(cont_block);
        builder.seal_block(cont_block);

        Ok(())
    }
}
