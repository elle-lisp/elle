// audited: 2026-10-06
//! The frozen form of LIR: plain records, read through one view by everything but the lowerer.
//!
//! src/lir/AGENTS.md
//! docs/impl/lir.md

mod body;
mod compare;
mod decode;
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
pub(crate) use head::{Files, PayloadHeader};
pub use instr::InstrRef;
pub use op::Op;
pub use operand::{ConstList, ConstRef, Slots, TemplateBytes};
pub use owned::{FrozenModule, LirCode, LirOwned};
pub(crate) use record::{flag, term};
pub use record::{BlockRec, ConstRec, Node, SiteRec, NO_FILE, NO_REG};
pub(crate) use view::Parts;
pub use view::{BlockRef, LirView, NodeRef, SiteRef};

#[cfg(test)]
mod tests;
