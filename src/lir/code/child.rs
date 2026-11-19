// audited: 2026-11-18
//! Which entry of a parent's child table holds the header a `MakeClosure` builds over.
//!
//! docs/impl/jit-closure.md

use super::instr::InstrRef;
use super::op::Op;
use super::view::LirView;
use crate::lir::ClosureId;

/// An index into a code payload's child table, which holds one header for
/// each `MakeClosure` in the order the emitter wrote them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChildIndex(u16);

impl ChildIndex {
    /// The entry at `ordinal`, the `MakeClosure`'s place among the function's
    /// `MakeClosure`s.
    pub fn new(ordinal: u16) -> Self {
        ChildIndex(ordinal)
    }

    pub fn get(self) -> usize {
        self.0 as usize
    }
}

impl LirView<'_> {
    /// The child-table entry holding the header of closure `id`, or `None`
    /// where no `MakeClosure` names it.
    ///
    /// The emitter pushes one entry for each `MakeClosure` as it walks the
    /// blocks and their nodes, and writes one header per `ClosureId`, so every
    /// entry for `id` holds the same header. This answers the first.
    pub fn child_of(&self, id: ClosureId) -> Option<ChildIndex> {
        self.nodes()
            .filter(|n| n.op() == Op::MakeClosure)
            .position(|n| matches!(n.instr(), InstrRef::MakeClosure { closure_id, .. } if closure_id == id))
            .map(|k| ChildIndex(k as u16))
    }
}
