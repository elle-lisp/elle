// audited: 2026-11-14
//! A call's hand-off word: the offered positions spilled for the call helper, and what each slot it settled receives.
//!
//! docs/impl/region/handoff.md

use super::*;
use crate::lir::{Offer, Offers};
use cranelift_codegen::ir::{StackSlot, StackSlotData, StackSlotKind, Value as ClifValue};

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
        let slot =
            builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
        let word = builder.ins().iconst(I64, offers.positions() as i64);
        builder.ins().stack_store(I64, word, slot, 0);
        (builder.ins().stack_addr(I64, slot, 0), Some(slot))
    }

    /// Stamp `nil` into the local of each move the callee took, so the frame's
    /// release of it loads an immediate.
    pub(super) fn discharge_handoff(
        &mut self,
        builder: &mut FunctionBuilder,
        offers: Offers<'_>,
        slot: StackSlot,
    ) {
        let nil_tag = builder.ins().iconst(I64, TAG_NIL as i64);
        let nil_payload = builder.ins().iconst(I64, 0);
        self.select_settled(builder, offers, slot, |_, _, _| (nil_tag, nil_payload));
    }

    /// Write each late mint's argument, among `args`, into its stash where the
    /// helper made the mint for a native suspend. The replayed fall-through
    /// releases it. Every other stash keeps the `nil` the frame stored.
    pub(super) fn fill_late_stashes(
        &mut self,
        builder: &mut FunctionBuilder,
        offers: Offers<'_>,
        slot: StackSlot,
        args: &[Reg],
    ) {
        self.select_settled(builder, offers, slot, |s, builder, offer| {
            s.use_var_pair(builder, args[offer.position()].0)
        });
    }

    /// Select `value(offer)` into the local of each offer whose position the
    /// helper wrote back into `slot`'s word, and keep every other local as it
    /// is. Runs before the call's signal check, so a park spills the selected
    /// value.
    fn select_settled(
        &mut self,
        builder: &mut FunctionBuilder,
        offers: Offers<'_>,
        slot: StackSlot,
        value: impl Fn(&mut Self, &mut FunctionBuilder, Offer) -> (ClifValue, ClifValue),
    ) {
        let settled = builder.ins().stack_load(I64, I64, slot, 0);
        for offer in offers.iter() {
            let bit = builder.ins().band_imm_u(settled, offer.bit() as i64);
            let set = builder.ins().icmp_imm_u(IntCC::NotEqual, bit, 0);
            let (new_tag, new_payload) = value(self, builder, offer);
            let local = self.local_slot_to_var(offer.slot());
            let (tag, payload) = self.use_var_pair(builder, local);
            let tag = builder.ins().select(set, new_tag, tag);
            let payload = builder.ins().select(set, new_payload, payload);
            self.def_var_pair(builder, local, tag, payload);
        }
    }
}
