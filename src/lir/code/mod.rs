// audited: 2026-10-06
//! The frozen form of LIR: plain records, read through one view by everything but the lowerer.
//!
//! src/lir/AGENTS.md
//! docs/impl/lir.md

mod body;
mod compare;
mod decode;
mod freeze;
mod gpu;
mod head;
mod instr;
mod op;
mod operand;
mod owned;
mod proof;
mod record;
mod split;
mod view;

pub use body::LirBody;
pub use freeze::freeze;
pub(crate) use head::PayloadHeader;
pub use instr::InstrRef;
pub use op::Op;
pub use operand::{ConstList, ConstRef, Slots, TemplateBytes};
pub use owned::{FrozenModule, LirCode, LirOwned};
pub use record::{BlockRec, ConstRec, Node, SiteRec, NO_FILE, NO_REG};
pub use view::{BlockRef, LirView, NodeRef, SiteRef};

#[cfg(test)]
mod tests;
