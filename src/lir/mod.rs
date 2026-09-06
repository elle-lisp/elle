// audited: 2026-09-05
// docs/impl/lir.md
//! Low-level Intermediate Representation: SSA form with basic blocks and
//! virtual registers, close to the target but architecture-independent.
//!
//! Pipeline:
//! ```text
//! HIR → Lower → LIR → Emit → Bytecode
//! ```

mod display;
mod emit;
pub mod intrinsics;
pub mod lower;
#[cfg(test)]
pub(crate) mod testkit;
mod types;

pub use display::terminator_kind;
pub use emit::{ClosureCompiled, Emitter};
pub use lower::Lowerer;
pub use types::{
    closure_value_const_count, for_each_def, for_each_terminator_use, for_each_use,
    value_to_lir_const, BasicBlock, BinOp, CallSiteInfo, ClosureId, CmpOp, ConvOp, Label, LirConst,
    LirFunction, LirInstr, LirModule, Reg, SpannedInstr, SpannedTerminator, Terminator, UnaryOp,
    YieldPointInfo,
};
