// audited: 2026-11-27
//! The local slots an instruction names, each as a read or a store, which slot liveness walks.
//!
//! src/lir/overview.md

use super::instr::InstrRef;
use super::view::LirView;

/// One local slot an instruction names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotRef {
    /// The instruction reads the slot.
    Read(u16),
    /// The instruction replaces the slot's value.
    Store(u16),
}

impl InstrRef<'_> {
    /// Hand `f` each local slot this instruction names, in field order, in
    /// `func`'s environment layout (src/lir/overview.md).
    pub fn slot_refs(&self, func: &LirView<'_>, f: impl FnMut(SlotRef)) {
        let _ = (func, f);
    }
}
