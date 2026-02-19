//! Low-level Intermediate Representation (LIR)
//!
//! LIR is SSA form with basic blocks and virtual registers.
//! It is close to the target but still architecture-independent.
//!
//! Pipeline:
//! ```text
//! HIR → Lower → LIR → Emit → Bytecode
//! ```

mod emit;
mod lower;
mod types;

pub use emit::Emitter;
pub use lower::Lowerer;
pub use types::{
    BasicBlock, BinOp, CmpOp, Label, LirConst, LirFunction, LirInstr, Reg, SpannedInstr,
    SpannedTerminator, Terminator, UnaryOp,
};
