//! Helper methods for FunctionTranslator (call emission, fast paths, exception checks)

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types::I64;
use cranelift_codegen::ir::InstBuilder;
use cranelift_frontend::FunctionBuilder;
use cranelift_module::{FuncId, Module};

use crate::lir::{BinOp, CmpOp, LirConst, UnaryOp};
use crate::value::repr::{
    TAG_EMPTY_LIST, TAG_FALSE, TAG_FLOAT, TAG_INT, TAG_KEYWORD, TAG_NIL, TAG_SYMBOL, TAG_TRUE,
};
use crate::value::SymbolId;

use super::translate::FunctionTranslator;
use super::JitError;

/// Helper to create a Variable from a register/slot index
#[inline]
fn var(n: u32) -> cranelift_frontend::Variable {
    cranelift_frontend::Variable::from_u32(n)
}

impl<'a> FunctionTranslator<'a> {
    /// Translate a constant to a pair of Cranelift values (tag, payload)
    pub(crate) fn translate_const(
        &self,
        builder: &mut FunctionBuilder,
        value: &LirConst,
    ) -> (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value) {
        match value {
            LirConst::Nil => {
                let t = builder.ins().iconst(I64, TAG_NIL as i64);
                let p = builder.ins().iconst(I64, 0);
                (t, p)
            }
            LirConst::EmptyList => {
                let t = builder.ins().iconst(I64, TAG_EMPTY_LIST as i64);
                let p = builder.ins().iconst(I64, 0);
                (t, p)
            }
            LirConst::Bool(true) => {
                let t = builder.ins().iconst(I64, TAG_TRUE as i64);
                let p = builder.ins().iconst(I64, 0);
                (t, p)
            }
            LirConst::Bool(false) => {
                let t = builder.ins().iconst(I64, TAG_FALSE as i64);
                let p = builder.ins().iconst(I64, 0);
                (t, p)
            }
            LirConst::Int(n) => {
                let t = builder.ins().iconst(I64, TAG_INT as i64);
                let p = builder.ins().iconst(I64, *n);
                (t, p)
            }
            LirConst::Float(f) => {
                let t = builder.ins().iconst(I64, TAG_FLOAT as i64);
                // Store f64 bits in payload as i64 reinterpretation
                let p = builder.ins().iconst(I64, f64::to_bits(*f) as i64);
                (t, p)
            }
            LirConst::String(s) => {
                // Create heap-allocated string at JIT-compile time, embed tag+payload.
                // The string lives on Rc heap; kept alive by LirFunction's constant pool.
                let v = crate::value::Value::string(s.clone());
                let t = builder.ins().iconst(I64, v.tag as i64);
                let p = builder.ins().iconst(I64, v.payload as i64);
                (t, p)
            }
            LirConst::Symbol(id) => {
                let v = crate::value::Value::symbol(id.0);
                let t = builder.ins().iconst(I64, TAG_SYMBOL as i64);
                let p = builder.ins().iconst(I64, v.payload as i64);
                (t, p)
            }
            LirConst::Keyword(name) => {
                let v = crate::value::Value::keyword(name);
                let t = builder.ins().iconst(I64, TAG_KEYWORD as i64);
                let p = builder.ins().iconst(I64, v.payload as i64);
                (t, p)
            }
            LirConst::ClosureRef(_) => {
                panic!("bug: ClosureRef in JIT — should have been patched during reconstruction")
            }
        }
    }

    /// Call a binary runtime helper with inline integer fast path.
    /// Returns (tag, payload).
    pub(crate) fn call_binary_helper(
        &mut self,
        builder: &mut FunctionBuilder,
        op: BinOp,
        lhs_tag: cranelift_codegen::ir::Value,
        lhs_payload: cranelift_codegen::ir::Value,
        rhs_tag: cranelift_codegen::ir::Value,
        rhs_payload: cranelift_codegen::ir::Value,
    ) -> Result<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value), JitError> {
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
        super::fastpath::emit_int_binop_fast_path(
            self.module,
            builder,
            op,
            lhs_tag,
            lhs_payload,
            rhs_tag,
            rhs_payload,
            func_id,
        )
    }

    /// Call a unary runtime helper with inline fast path. Returns (tag, payload).
    pub(crate) fn call_unary_helper(
        &mut self,
        builder: &mut FunctionBuilder,
        op: UnaryOp,
        src_tag: cranelift_codegen::ir::Value,
        src_payload: cranelift_codegen::ir::Value,
    ) -> Result<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value), JitError> {
        let func_id = match op {
            UnaryOp::Neg => self.helpers.neg,
            UnaryOp::Not => self.helpers.not,
            UnaryOp::BitNot => self.helpers.bit_not,
        };
        super::fastpath::emit_unary_fast_path(
            self.module,
            builder,
            op,
            src_tag,
            src_payload,
            func_id,
        )
    }

    /// Call a comparison runtime helper with inline integer fast path.
    /// Returns (tag, payload).
    pub(crate) fn call_compare_helper(
        &mut self,
        builder: &mut FunctionBuilder,
        op: CmpOp,
        lhs_tag: cranelift_codegen::ir::Value,
        lhs_payload: cranelift_codegen::ir::Value,
        rhs_tag: cranelift_codegen::ir::Value,
        rhs_payload: cranelift_codegen::ir::Value,
    ) -> Result<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value), JitError> {
        let func_id = match op {
            CmpOp::Eq => self.helpers.eq,
            CmpOp::Ne => self.helpers.ne,
            CmpOp::Lt => self.helpers.lt,
            CmpOp::Le => self.helpers.le,
            CmpOp::Gt => self.helpers.gt,
            CmpOp::Ge => self.helpers.ge,
        };
        super::fastpath::emit_int_cmpop_fast_path(
            self.module,
            builder,
            op,
            lhs_tag,
            lhs_payload,
            rhs_tag,
            rhs_payload,
            func_id,
        )
    }

    /// Call a helper that takes a single Value (tag, payload) and returns (tag, payload).
    pub(crate) fn call_helper_value_unary(
        &mut self,
        builder: &mut FunctionBuilder,
        func_id: FuncId,
        tag: cranelift_codegen::ir::Value,
        payload: cranelift_codegen::ir::Value,
    ) -> Result<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value), JitError> {
        let func_ref = self.module.declare_func_in_func(func_id, builder.func);
        let call = builder.ins().call(func_ref, &[tag, payload]);
        Ok((builder.inst_results(call)[0], builder.inst_results(call)[1]))
    }

    /// Call a helper that takes two Values (atag, apay, btag, bpay) and returns (tag, payload).
    pub(crate) fn call_helper_value_binary(
        &mut self,
        builder: &mut FunctionBuilder,
        func_id: FuncId,
        a_tag: cranelift_codegen::ir::Value,
        a_payload: cranelift_codegen::ir::Value,
        b_tag: cranelift_codegen::ir::Value,
        b_payload: cranelift_codegen::ir::Value,
    ) -> Result<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value), JitError> {
        let func_ref = self.module.declare_func_in_func(func_id, builder.func);
        let call = builder
            .ins()
            .call(func_ref, &[a_tag, a_payload, b_tag, b_payload]);
        Ok((builder.inst_results(call)[0], builder.inst_results(call)[1]))
    }

    /// Call a helper that takes a Value + vm pointer and returns (tag, payload).
    /// Signature: (tag, payload, vm) -> (tag, payload)
    pub(crate) fn call_helper_value_vm(
        &mut self,
        builder: &mut FunctionBuilder,
        func_id: FuncId,
        tag: cranelift_codegen::ir::Value,
        payload: cranelift_codegen::ir::Value,
        vm: cranelift_codegen::ir::Value,
    ) -> Result<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value), JitError> {
        let func_ref = self.module.declare_func_in_func(func_id, builder.func);
        let call = builder.ins().call(func_ref, &[tag, payload, vm]);
        Ok((builder.inst_results(call)[0], builder.inst_results(call)[1]))
    }

    /// Call a helper with a vm-only parameter. Returns (tag, payload).
    /// Signature: (vm) -> (tag, payload)
    pub(crate) fn call_helper_vm_only(
        &mut self,
        builder: &mut FunctionBuilder,
        func_id: FuncId,
        vm: cranelift_codegen::ir::Value,
    ) -> Result<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value), JitError> {
        let func_ref = self.module.declare_func_in_func(func_id, builder.func);
        let call = builder.ins().call(func_ref, &[vm]);
        Ok((builder.inst_results(call)[0], builder.inst_results(call)[1]))
    }

    /// Call a helper with two Values + vm. Returns (tag, payload).
    /// Signature: (atag, apay, btag, bpay, vm) -> (tag, payload)
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn call_helper_value_binary_vm(
        &mut self,
        builder: &mut FunctionBuilder,
        func_id: FuncId,
        a_tag: cranelift_codegen::ir::Value,
        a_payload: cranelift_codegen::ir::Value,
        b_tag: cranelift_codegen::ir::Value,
        b_payload: cranelift_codegen::ir::Value,
        vm: cranelift_codegen::ir::Value,
    ) -> Result<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value), JitError> {
        let func_ref = self.module.declare_func_in_func(func_id, builder.func);
        let call = builder
            .ins()
            .call(func_ref, &[a_tag, a_payload, b_tag, b_payload, vm]);
        Ok((builder.inst_results(call)[0], builder.inst_results(call)[1]))
    }

    /// Call elle_jit_rotate_pools to rotate slab pools at a self-tail-call boundary.
    /// Signature: (vm) -> void
    pub(crate) fn call_rotate_pools(
        &mut self,
        builder: &mut FunctionBuilder,
        vm: cranelift_codegen::ir::Value,
    ) -> Result<(), JitError> {
        let func_ref = self
            .module
            .declare_func_in_func(self.helpers.rotate_pools, builder.func);
        builder.ins().call(func_ref, &[vm]);
        Ok(())
    }

    /// Call the elle_jit_call helper.
    /// Signature: (func_tag, func_payload, args_ptr, nargs, vm) -> (tag, payload)
    pub(crate) fn call_helper_call(
        &mut self,
        builder: &mut FunctionBuilder,
        func_tag: cranelift_codegen::ir::Value,
        func_payload: cranelift_codegen::ir::Value,
        args_ptr: cranelift_codegen::ir::Value,
        nargs: cranelift_codegen::ir::Value,
        vm: cranelift_codegen::ir::Value,
    ) -> Result<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value), JitError> {
        let func_ref = self
            .module
            .declare_func_in_func(self.helpers.call, builder.func);
        let call = builder
            .ins()
            .call(func_ref, &[func_tag, func_payload, args_ptr, nargs, vm]);
        Ok((builder.inst_results(call)[0], builder.inst_results(call)[1]))
    }

    /// Call the elle_jit_tail_call helper.
    /// Signature: (func_tag, func_payload, args_ptr, nargs, vm) -> (tag, payload)
    pub(crate) fn call_helper_tail_call(
        &mut self,
        builder: &mut FunctionBuilder,
        func_tag: cranelift_codegen::ir::Value,
        func_payload: cranelift_codegen::ir::Value,
        args_ptr: cranelift_codegen::ir::Value,
        nargs: cranelift_codegen::ir::Value,
        vm: cranelift_codegen::ir::Value,
    ) -> Result<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value), JitError> {
        let func_ref = self
            .module
            .declare_func_in_func(self.helpers.tail_call, builder.func);
        let call = builder
            .ins()
            .call(func_ref, &[func_tag, func_payload, args_ptr, nargs, vm]);
        Ok((builder.inst_results(call)[0], builder.inst_results(call)[1]))
    }

    /// Emit a direct call to an SCC peer function.
    /// Returns (tag, payload).
    pub(crate) fn emit_direct_scc_call(
        &mut self,
        builder: &mut FunctionBuilder,
        peer_func_id: FuncId,
        target_sym: SymbolId,
        args: &[crate::lir::Reg],
        vm: cranelift_codegen::ir::Value,
    ) -> Result<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value), JitError> {
        let func_ref = self.module.declare_func_in_func(peer_func_id, builder.func);

        // Build args on stack (each Value is 16 bytes)
        let (args_ptr, nargs) = if args.is_empty() {
            let null = builder.ins().iconst(I64, 0);
            let zero = builder.ins().iconst(I64, 0);
            (null, zero)
        } else {
            let slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                (args.len() * 16) as u32, // 16 bytes per Value
                0,
            ));
            for (i, arg_reg) in args.iter().enumerate() {
                let (arg_tag, arg_payload) = self.use_var_pair(builder, arg_reg.0);
                let tag_offset = (i * 16) as i32;
                let payload_offset = (i * 16 + 8) as i32;
                builder.ins().stack_store(arg_tag, slot, tag_offset);
                builder.ins().stack_store(arg_payload, slot, payload_offset);
            }
            let addr = builder.ins().stack_addr(I64, slot, 0);
            let count = builder.ins().iconst(I64, args.len() as i64);
            (addr, count)
        };

        // Null env for capture-free functions
        let null_env = builder.ins().iconst(I64, 0);
        // self_tag/self_payload for self-call detection
        let (call_self_tag, call_self_payload) = if self.self_sym == Some(target_sym) {
            if let Some((st, sp)) = self.self_tag_payload {
                (st, sp)
            } else {
                let z = builder.ins().iconst(I64, 0);
                (z, z)
            }
        } else {
            let z = builder.ins().iconst(I64, 0);
            (z, z)
        };

        let call = builder.ins().call(
            func_ref,
            &[
                null_env,
                args_ptr,
                nargs,
                vm,
                call_self_tag,
                call_self_payload,
            ],
        );
        Ok((builder.inst_results(call)[0], builder.inst_results(call)[1]))
    }

    /// Map a stack-relative local slot (from LirInstr::LoadLocal/StoreLocal)
    /// to a JIT variable index.
    ///
    /// The dual-address-space lowerer assigns stack-relative slots starting
    /// at 0 for all non-LBox locals (non-LBox params + let bindings).
    /// These always map to the local_var_base region.
    pub(crate) fn local_slot_to_var(&self, slot: u16) -> u32 {
        self.local_var_base + slot as u32
    }

    /// Convenience: use a (tag, payload) variable pair for register `r`.
    pub(crate) fn use_var_pair(
        &self,
        builder: &mut FunctionBuilder,
        r: u32,
    ) -> (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value) {
        let tag = builder.use_var(var(self.var_tag(r)));
        let payload = builder.use_var(var(self.var_payload(r)));
        (tag, payload)
    }

    /// Convenience: define a (tag, payload) variable pair for register `r`.
    pub(crate) fn def_var_pair(
        &self,
        builder: &mut FunctionBuilder,
        r: u32,
        tag: cranelift_codegen::ir::Value,
        payload: cranelift_codegen::ir::Value,
    ) {
        builder.def_var(var(self.var_tag(r)), tag);
        builder.def_var(var(self.var_payload(r)), payload);
    }

    /// Emit exception check after a call instruction.
    /// If an exception is pending, return (TAG_NIL, 0) immediately.
    pub(crate) fn emit_exception_check_after_call(
        &mut self,
        builder: &mut FunctionBuilder,
    ) -> Result<(), JitError> {
        let vm = self.vm_ptr.ok_or_else(|| {
            JitError::InvalidLir("emit_exception_check without vm pointer".to_string())
        })?;

        // Call has_exception(vm) -> (tag, payload)
        let (has_exc_tag, _) = self.call_helper_vm_only(builder, self.helpers.has_exception, vm)?;

        // TAG_TRUE = 3, TAG_FALSE = 4, TAG_NIL = 2 — check if tag == TAG_TRUE
        let tag_true = builder.ins().iconst(I64, TAG_TRUE as i64);
        let is_true = builder.ins().icmp(IntCC::Equal, has_exc_tag, tag_true);

        let exc_block = builder.create_block();
        let cont_block = builder.create_block();
        builder.ins().brif(is_true, exc_block, &[], cont_block, &[]);

        builder.switch_to_block(exc_block);
        builder.seal_block(exc_block);
        let nil_tag = builder.ins().iconst(I64, TAG_NIL as i64);
        let zero = builder.ins().iconst(I64, 0);
        builder.ins().return_(&[nil_tag, zero]);

        builder.switch_to_block(cont_block);
        builder.seal_block(cont_block);

        Ok(())
    }

    /// Emit yield check after a call instruction for yielding functions.
    pub(crate) fn emit_yield_check_after_call(
        &mut self,
        builder: &mut FunctionBuilder,
        call_site_idx: u32,
    ) -> Result<(), JitError> {
        let vm = self.vm_ptr.ok_or_else(|| {
            JitError::InvalidLir("emit_yield_check without vm pointer".to_string())
        })?;
        let (self_tag, self_payload) = self.self_tag_payload.ok_or_else(|| {
            JitError::InvalidLir("emit_yield_check without self_tag_payload".to_string())
        })?;

        // Check if any signal is pending
        let (has_sig_tag, _) = self.call_helper_vm_only(builder, self.helpers.has_signal, vm)?;
        let tag_true = builder.ins().iconst(I64, TAG_TRUE as i64);
        let is_true = builder.ins().icmp(IntCC::Equal, has_sig_tag, tag_true);

        let yield_block = builder.create_block();
        let cont_block = builder.create_block();
        builder
            .ins()
            .brif(is_true, yield_block, &[], cont_block, &[]);

        builder.switch_to_block(yield_block);
        builder.seal_block(yield_block);

        let call_site = self.lir.call_sites.get(call_site_idx as usize);
        let stack_regs = match call_site {
            Some(cs) => cs.stack_regs.as_slice(),
            None => &[] as &[crate::lir::Reg],
        };

        let spilled_ptr = self.spill_locals_and_operands(builder, stack_regs)?;
        let call_site_idx_val = builder.ins().iconst(I64, call_site_idx as i64);

        // Call elle_jit_yield_through_call(spilled, call_site_index, vm, closure_tag, closure_payload)
        let func_ref = self
            .module
            .declare_func_in_func(self.helpers.jit_yield_through_call, builder.func);
        let call = builder.ins().call(
            func_ref,
            &[spilled_ptr, call_site_idx_val, vm, self_tag, self_payload],
        );
        let result_tag = builder.inst_results(call)[0];
        let result_payload = builder.inst_results(call)[1];
        builder.ins().return_(&[result_tag, result_payload]);

        builder.switch_to_block(cont_block);
        builder.seal_block(cont_block);

        Ok(())
    }
}
