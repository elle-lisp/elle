// audited: 2026-11-14
//! The constructors that build a call which offers nothing, for a test whose subject is not the handoff.
//!
//! src/lir/AGENTS.md
//! docs/impl/region/handoff.md

use super::instr::InstrRef;
use super::operand::{Offers, Slots};
use crate::hir::region::StaticRegion;
use crate::lir::Reg;

impl<'a> InstrRef<'a> {
    /// A call of `func` with `args`, offering no argument to move.
    pub fn call(
        dst: Reg,
        func: Reg,
        args: &'a [Reg],
        arity_checked: bool,
        region: StaticRegion,
    ) -> Self {
        InstrRef::Call {
            dst,
            func,
            args,
            arity_checked,
            region,
            moves: Offers::NONE,
        }
    }

    /// A suspending call of `func` with `args`, offering no argument to move.
    pub fn suspending_call(
        dst: Reg,
        func: Reg,
        args: &'a [Reg],
        arity_checked: bool,
        region: StaticRegion,
    ) -> Self {
        InstrRef::SuspendingCall {
            dst,
            func,
            args,
            arity_checked,
            region,
            moves: Offers::NONE,
        }
    }

    /// A tail call of `func` with `args` that defers no release, stashes no
    /// borrowed argument and mints nothing late.
    pub fn tail_call(
        dst: Reg,
        func: Reg,
        args: &'a [Reg],
        arity_checked: bool,
        region: StaticRegion,
    ) -> Self {
        InstrRef::TailCall {
            dst,
            func,
            args,
            arity_checked,
            region,
            defer_callee_release: false,
            deferred_release_slot: None,
            borrowed_arg_slots: Slots::new(&[]),
            late_mints: Offers::NONE,
        }
    }
}
