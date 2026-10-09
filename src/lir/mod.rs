// audited: 2026-10-06
// docs/impl/lir.md
//! Low-level Intermediate Representation: SSA form with basic blocks and
//! virtual registers, close to the target but architecture-independent.
//!
//! Pipeline:
//! ```text
//! HIR → Lower → LIR → Emit → Bytecode
//! ```

pub mod build;
pub mod code;
mod display;
mod emit;
pub mod intrinsics;
pub mod lower;
#[cfg(test)]
pub(crate) mod testkit;
mod types;

pub use build::{LirBuilder, LirHead};
pub use code::{
    ConstList, ConstRec, ConstRef, FrozenModule, InstrRef, LirBody, LirCode, LirOwned, LirView,
    Slots, TemplateBytes,
};
pub use display::terminator_kind;
pub use emit::{ClosureCompiled, Emitter};
pub use lower::Lowerer;
pub use types::{
    for_each_terminator_use, BinOp, CallSiteInfo, ClosureId, CmpOp, ConvOp, Label, OperandProof,
    Reg, Terminator, UnaryOp, YieldPointInfo,
};
