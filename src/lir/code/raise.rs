// audited: 2026-10-06
//! Which instructions may raise at their own site, so a frame that may signal parks there.
//!
//! src/lir/AGENTS.md

use super::instr::InstrRef;
use super::op::Op;

impl Op {
    /// Whether a runtime check inside this instruction may raise an error.
    ///
    /// An error parks the raising frame at the instruction that raised it
    /// (docs/impl/region/unwind.md), so a tier that does not park in the
    /// interpreter needs a park site after each of these. The emitter records
    /// one call site per such instruction in a function that may signal, and
    /// the JIT follows each with a signal check, both reading this answer, so
    /// the two count the same sites. Calls are not here: every call is a site
    /// whatever its callee.
    pub fn may_raise(self) -> bool {
        matches!(
            self,
            Op::MatchFail
                | Op::FirstDestructure
                | Op::RestDestructure
                | Op::ArrayMutRefDestructure
                | Op::ArrayMutSliceFrom
                | Op::StructGetDestructure
                | Op::ArrayMutExtend
                | Op::ArrayMutPush
                | Op::PushParamFrame
                | Op::CheckSignalBound
        )
    }

    /// Whether a restart of this instruction's raise delivers the resume value
    /// into a result. `PushParamFrame` and `CheckSignalBound` produce no value,
    /// so a restart continues past them (docs/impl/vm.md).
    pub fn raise_delivers(self) -> bool {
        !matches!(self, Op::PushParamFrame | Op::CheckSignalBound)
    }
}

impl InstrRef<'_> {
    /// [`Op::may_raise`] for this instruction's opcode.
    pub fn may_raise(&self) -> bool {
        Op::of(self).may_raise()
    }

    /// [`Op::raise_delivers`] for this instruction's opcode.
    pub fn raise_delivers(&self) -> bool {
        Op::of(self).raise_delivers()
    }
}
