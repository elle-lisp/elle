//! Region-slot extraction for `LirInstr`.
//!
//! Kept beside the enum but in its own file so the enum root stays a pure
//! data definition: the enum's doc-heavy variants dominate `instr.rs`, and the
//! only behavior on it — reading the per-call/allocation region slot off a
//! variant — is cohesive enough to live on its own.

use super::LirInstr;
use crate::hir::region::StaticRegion;

impl LirInstr {
    /// The static region slot this allocating / calling instruction is stamped
    /// with, if any. `None` for instructions that neither allocate nor route a
    /// per-call region (the region is *structurally absent* from those
    /// variants). Mirrors the region the JIT and bytecode emitters read off
    /// each instruction. The RC instructions (`IncrefRegion`/`DecrefRegion`/
    /// `DecrefValueRegion`) are intentionally **not** included here — they carry
    /// their own region operand handled directly in their handlers, and the
    /// per-call routing region this returns is a different thing.
    pub fn region(&self) -> Option<StaticRegion> {
        match self {
            LirInstr::MakeClosure { region, .. }
            | LirInstr::Call { region, .. }
            | LirInstr::SuspendingCall { region, .. }
            | LirInstr::TailCall { region, .. }
            | LirInstr::List { region, .. }
            | LirInstr::MaterializeConst { region, .. }
            | LirInstr::MakeArrayMut { region, .. }
            | LirInstr::MakeCaptureCell { region, .. }
            | LirInstr::CallArrayMut { region, .. }
            | LirInstr::TailCallArrayMut { region, .. }
            | LirInstr::Freeze { region, .. }
            | LirInstr::Thaw { region, .. } => Some(*region),
            _ => None,
        }
    }
}
