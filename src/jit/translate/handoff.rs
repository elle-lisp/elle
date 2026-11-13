// audited: 2026-11-13
// docs/impl/region/handoff.md
//! A call's hand-off word: the offered positions spilled for the call helper, and the `nil` each slot the callee took receives.

use super::*;
use crate::lir::Offers;
use cranelift_codegen::ir::{StackSlot, StackSlotData, StackSlotKind};

impl<'a> FunctionTranslator<'a> {
    /// The address to hand a call helper for `offers`, and the slot it names:
    /// the offered positions spilled to a word of this frame, or 0 and no
    /// slot where the call offers nothing.
    pub(super) fn handoff_word(
        &mut self,
        builder: &mut FunctionBuilder,
        offers: Offers<'_>,
    ) -> (cranelift_codegen::ir::Value, Option<StackSlot>) {
        if offers.is_empty() {
            return (builder.ins().iconst(I64, 0), None);
        }
        let positions = offers.iter().fold(0u64, |w, o| w | (1 << o.position));
        let slot =
            builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
        let word = builder.ins().iconst(I64, positions as i64);
        builder.ins().stack_store(I64, word, slot, 0);
        (builder.ins().stack_addr(I64, slot, 0), Some(slot))
    }

    /// Stamp `nil` into the local of each offer whose position the helper
    /// wrote back into `slot`'s word, so the frame's release of it loads an
    /// immediate. Runs before the call's signal check, so a park spills the
    /// stamped value.
    pub(super) fn discharge_handoff(
        &mut self,
        builder: &mut FunctionBuilder,
        offers: Offers<'_>,
        slot: StackSlot,
    ) {
        let taken = builder.ins().stack_load(I64, I64, slot, 0);
        let nil_tag = builder.ins().iconst(I64, TAG_NIL as i64);
        let nil_payload = builder.ins().iconst(I64, 0);
        for offer in offers.iter() {
            let bit = builder
                .ins()
                .band_imm_u(taken, (1u64 << offer.position) as i64);
            let took = builder.ins().icmp_imm_u(IntCC::NotEqual, bit, 0);
            let local = self.local_slot_to_var(offer.slot);
            let (tag, payload) = self.use_var_pair(builder, local);
            let tag = builder.ins().select(took, nil_tag, tag);
            let payload = builder.ins().select(took, nil_payload, payload);
            self.def_var_pair(builder, local, tag, payload);
        }
    }
}
