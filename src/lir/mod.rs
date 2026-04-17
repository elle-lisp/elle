//! Low-level Intermediate Representation (LIR)
//!
//! LIR is SSA form with basic blocks and virtual registers.
//! It is close to the target but still architecture-independent.
//!
//! Pipeline:
//! ```text
//! HIR → Lower → LIR → Emit → Bytecode
//! ```

mod display;
mod emit;
pub mod intrinsics;
pub mod lower;
mod types;

pub use display::terminator_kind;
pub use emit::Emitter;
pub use lower::{Lowerer, ScopeStats};
pub use types::{
    closure_value_const_count, BasicBlock, BinOp, CallSiteInfo, ClosureId, CmpOp, ConvOp, Label,
    LirConst, LirFunction, LirInstr, LirModule, Reg, SpannedInstr, SpannedTerminator, Terminator,
    UnaryOp, YieldPointInfo,
};
