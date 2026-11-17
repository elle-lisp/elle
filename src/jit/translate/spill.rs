// audited: 2026-11-17
// docs/impl/jit.md
//! The stack slots compiled code hands the runtime: one fresh per register list, and one every side exit shares.
//!
//! A call, an array, a parameter frame and a region group each store their
//! registers in a fresh slot of their own. The prologue sizes the shared slot
//! once to the worst-case save requirement, and a side-exit saves live locals
//! and operands into it. The shared slot's methods stay `pub(crate)` because the
//! compiler driver allocates it in the prologue, and the terminator and
//! instruction paths spill into it.

use super::*;

impl<'a> FunctionTranslator<'a> {
    /// Store the values of `regs` in a fresh stack slot, one `Value` each, and
    /// answer the slot's address: null where `regs` is empty. Every register
    /// is read before the caller writes any variable, so a list that names a
    /// variable the caller then binds reads its old value.
    pub(crate) fn spill_regs(
        &mut self,
        builder: &mut FunctionBuilder,
        regs: &[Reg],
    ) -> cranelift_codegen::ir::Value {
        if regs.is_empty() {
            return builder.ins().iconst(I64, 0);
        }
        let slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            (regs.len() * std::mem::size_of::<crate::value::Value>()) as u32,
            0,
        ));
        for (i, reg) in regs.iter().enumerate() {
            let (tag, payload) = self.use_var_pair(builder, reg.0);
            store_value_slot(builder, slot, i as u32, tag, payload);
        }
        builder.ins().stack_addr(I64, slot, 0)
    }

    /// Allocate the shared spill slot sized to the maximum spill requirement.
    pub(crate) fn allocate_shared_spill_slot(&mut self, builder: &mut FunctionBuilder) {
        let num_locals = self.lir.num_locals() as usize;

        let max_operands = self
            .lir
            .yield_points()
            .chain(self.lir.call_sites())
            .chain(self.lir.charge_sites())
            .map(|site| site.stack_regs.len())
            .max()
            .unwrap_or(0);
        // Spill saves: arity params (arg_vars) + num_locals locals (local_var_base)
        // + operand stack entries.
        let arity = self.lir.num_params();
        let max_total = arity + num_locals + max_operands;

        if max_total > 0 {
            // Each Value is 16 bytes
            let slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                (max_total * 16) as u32,
                0,
            ));
            self.shared_spill_slot = Some(slot);
        }
    }

    /// Spill local variables and operand stack registers to the shared stack slot.
    ///
    /// Returns a Cranelift value pointing to the spilled buffer (*const Value),
    /// or a null pointer constant if there's nothing to spill.
    pub(crate) fn spill_locals_and_operands(
        &mut self,
        builder: &mut FunctionBuilder,
        stack_regs: &[Reg],
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let arity = self.lir.num_params() as u16;
        let num_locals = self.lir.num_locals();
        // Spill params (from arg vars) + all local vars (from local_var_base).
        // num_locals includes non-LBox param copies + let-bound locals.
        let num_locally_defined = num_locals;
        let total = arity as usize + num_locally_defined as usize + stack_regs.len();

        if total == 0 {
            return Ok(builder.ins().iconst(I64, 0)); // null pointer
        }

        let slot = self
            .shared_spill_slot
            .expect("JIT bug: spill_locals_and_operands called but no shared spill slot allocated");

        let mut slot_idx: u32 = 0;

        // 1. Spill parameters (from arg variables)
        for i in 0..arity as u32 {
            let base = self.arg_var_base + i;
            let (tag, payload) = self.use_var_pair(builder, base);
            store_value_slot(builder, slot, slot_idx, tag, payload);
            slot_idx += 1;
        }

        // 2. Spill locally-defined variables
        for i in 0..num_locally_defined as u32 {
            let base = self.local_var_base + i;
            let (tag, payload) = self.use_var_pair(builder, base);
            store_value_slot(builder, slot, slot_idx, tag, payload);
            slot_idx += 1;
        }

        // 3. Spill operand stack registers
        for reg in stack_regs {
            let (tag, payload) = self.use_var_pair(builder, reg.0);
            store_value_slot(builder, slot, slot_idx, tag, payload);
            slot_idx += 1;
        }

        Ok(builder.ins().stack_addr(I64, slot, 0))
    }
}
