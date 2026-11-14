// audited: 2026-11-13
// docs/impl/region/operand.md
// docs/impl/jit.md
//! Translating `DecrefPostponed`: the release an operand arm postponed to its tail call, run as the interpreter runs it.

use super::*;

impl<'a> FunctionTranslator<'a> {
    /// Release what the postponed slot `slot` holds, stamp the route slot
    /// `route` `nil` when it held a value, and stamp `slot` `nil`. A `nil`
    /// value has no region, so the release helper does nothing for it, and a
    /// `select` keeps the route on that path. Both slots are variables, so no
    /// branch is needed.
    pub(super) fn translate_decref_postponed(
        &mut self,
        builder: &mut FunctionBuilder,
        slot: u16,
        route: u16,
    ) -> Result<(), JitError> {
        let held = self.local_slot_to_var(slot);
        let (ht, hp) = self.use_var_pair(builder, held);
        let vm = self.vm_ptr.ok_or_else(|| {
            JitError::InvalidLir("DecrefPostponed without vm pointer".to_string())
        })?;
        let func_ref = self
            .module
            .declare_func_in_func(self.helpers.decref_value_region, builder.func);
        builder.ins().call(func_ref, &[ht, hp, vm]);
        let nil_tag = builder.ins().iconst(I64, TAG_NIL as i64);
        let zero = builder.ins().iconst(I64, 0);
        let was_nil = builder.ins().icmp_imm_u(IntCC::Equal, ht, TAG_NIL as i64);
        let base = self.local_slot_to_var(route);
        let (rt, rp) = self.use_var_pair(builder, base);
        let tag = builder.ins().select(was_nil, rt, nil_tag);
        let payload = builder.ins().select(was_nil, rp, zero);
        self.def_var_pair(builder, base, tag, payload);
        self.def_var_pair(builder, held, nil_tag, zero);
        Ok(())
    }
}
