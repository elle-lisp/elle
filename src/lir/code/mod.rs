// audited: 2026-10-06
//! The frozen form of LIR: plain records, read through one view by everything but the lowerer.
//!
//! src/lir/AGENTS.md
//! docs/impl/lir.md

mod decode;
mod freeze;
mod instr;
mod op;
mod owned;
mod record;
mod view;

pub use freeze::freeze;
pub use instr::{ConstList, ConstRef, InstrRef, Slots, TemplateBytes};
pub use op::Op;
pub use owned::{FrozenModule, LirCode, LirOwned};
pub use record::{BlockRec, ConstRec, Node, SiteRec, NO_FILE, NO_REG};
pub use view::{BlockRef, LirView, NodeRef, SiteRef};

#[cfg(test)]
mod tests;
