// audited: 2026-11-21
//! `SiteRef`: one yield point, call site or fuel charge site of a frozen function, as a view reads it.
//!
//! src/lir/emit/overview.md
//! docs/impl/lir.md

use super::record::SiteRec;
use crate::lir::Reg;

/// A yield point, a call site or a fuel charge site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SiteRef<'a> {
    /// The bytecode offset the interpreter resumes at.
    pub resume_ip: usize,
    /// The local slots below the operands.
    pub num_locals: u16,
    /// The registers on the operand stack, bottom to top.
    pub stack_regs: &'a [Reg],
    /// Whether a resume delivers its value into the site's result. False for
    /// an instruction that produces no value, and for a charge site.
    pub push_resume_value: bool,
}

impl<'a> SiteRef<'a> {
    pub(super) fn of(rec: &SiteRec, regs: &'a [Reg]) -> SiteRef<'a> {
        let at = rec.regs as usize;
        SiteRef {
            resume_ip: rec.resume_ip as usize,
            num_locals: rec.num_locals,
            stack_regs: &regs[at..at + rec.n_regs as usize],
            push_resume_value: rec.delivers != 0,
        }
    }
}
