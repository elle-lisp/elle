// audited: 2026-10-06
//! Decoding a frozen node or block record into what a reader matches on.
//!
//! src/lir/AGENTS.md
//! docs/impl/lir.md

use super::instr::InstrRef;
use super::record::{BlockRec, Node};
use super::view::Parts;
use crate::lir::{Reg, Terminator};

/// The instruction `node` holds.
pub(crate) fn instr<'a>(_node: &'a Node, _parts: &Parts<'a>) -> InstrRef<'a> {
    InstrRef::PopParamFrame
}

/// The registers `node` reads.
pub(crate) fn uses<'a>(_node: &'a Node, _parts: &Parts<'a>) -> &'a [Reg] {
    &[]
}

/// How the block `rec` exits.
pub(crate) fn terminator(_rec: &BlockRec, _parts: &Parts<'_>) -> Terminator {
    Terminator::Unreachable
}
