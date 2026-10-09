// audited: 2026-10-06
//! The call sites a suspending function records: where a caller parks behind a
//! callee that suspended, a tail call's included.
//!
//! src/lir/AGENTS.md
//! docs/impl/region/park.md

use super::*;
use crate::compiler::bytecode::disassemble_lines;
use crate::hir::region::StaticRegion;

/// One block: an operand left on the stack below the call, then a tail call
/// of `r1` with the argument `r2`, then a return of the call's result.
///
/// The call reads its argument below its callee. With `in_place` the constants
/// are pushed in that order and the call finds them on top. Without it the
/// callee is pushed first, so the call copies it to the top and the original
/// stays on the stack beneath.
fn tail_call_func(signal: crate::signals::Signal, in_place: bool) -> LirOwned {
    let operand = InstrRef::Const {
        dst: Reg(0),
        value: ConstRef::Int(7),
    };
    let callee = InstrRef::Const {
        dst: Reg(1),
        value: ConstRef::Nil,
    };
    let arg = InstrRef::Const {
        dst: Reg(2),
        value: ConstRef::Int(1),
    };
    let [first, second, third] = if in_place {
        [operand, arg, callee]
    } else {
        [operand, callee, arg]
    };
    LirFixture::new(Arity::Exact(0))
        .signal(signal)
        .block(
            0,
            &[
                first,
                second,
                third,
                InstrRef::TailCall {
                    dst: Reg(3),
                    func: Reg(1),
                    args: &[Reg(2)],
                    arity_checked: false,
                    region: StaticRegion::new(1).unwrap(),
                    defer_callee_release: false,
                    deferred_release_slot: None,
                    borrowed_arg_slots: crate::lir::Slots::new(&[]),
                },
            ],
            Terminator::Return(Reg(3)),
        )
        .build()
}

/// The bytecode offset of the instruction that follows the tail call: where a
/// native callee that completes falls through to.
fn offset_after_tail_call(bytecode: &Bytecode) -> usize {
    let lines = disassemble_lines(&bytecode.instructions);
    let at = lines
        .iter()
        .position(|l| l.contains("TailCall"))
        .unwrap_or_else(|| panic!("no TailCall in: {lines:?}"));
    let next = lines
        .get(at + 1)
        .unwrap_or_else(|| panic!("nothing follows the TailCall in: {lines:?}"));
    next.trim_start_matches('[')
        .split(']')
        .next()
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or_else(|| panic!("no offset on: {next}"))
}

/// A compiled frame whose tail callee suspends parks at a call site, and the
/// resume runs the block after the tail call from that site's ip. The
/// counter-factual is no site at all: the compiled frame then returns at the
/// call and the releases in that block never run.
#[test]
fn a_suspending_functions_tail_call_records_a_call_site() {
    let func = tail_call_func(crate::signals::Signal::yields(), true);
    let (bytecode, _, call_sites) = emitter().emit(&func.view());

    assert_eq!(call_sites.len(), 1, "one tail call, one call site");
    assert_eq!(
        call_sites[0].resume_ip,
        offset_after_tail_call(&bytecode),
        "the resume ip is where the block after the tail call starts",
    );
    assert_eq!(
        call_sites[0].stack_regs,
        vec![Reg(0)],
        "the stack below the callee and its argument, which the call pops",
    );
}

/// A function that cannot suspend parks nowhere, a tail call's site included.
#[test]
fn a_silent_functions_tail_call_records_no_call_site() {
    let func = tail_call_func(crate::signals::Signal::silent(), true);
    let (_, _, call_sites) = emitter().emit(&func.view());
    assert!(call_sites.is_empty(), "got {call_sites:?}");
}

/// The resumed frame's stack must match the real one, so the copy of the callee
/// that the call leaves beneath its operands belongs to the recorded stack. A
/// `Call` site records it the same way. The counter-factual is the
/// operand-only stack, which shifts every local the resumed block addresses.
#[test]
fn a_tail_call_site_keeps_the_callee_copy_left_beneath_its_operands() {
    let func = tail_call_func(crate::signals::Signal::yields(), false);
    let (bytecode, _, call_sites) = emitter().emit(&func.view());

    let lines = disassemble_lines(&bytecode.instructions);
    assert!(
        lines.iter().any(|l| l.contains("DupN")),
        "the callee was copied to the top: {lines:?}",
    );
    assert_eq!(call_sites.len(), 1, "one tail call, one call site");
    assert_eq!(
        call_sites[0].stack_regs,
        vec![Reg(0), Reg(1)],
        "the original callee stays on the stack beneath the call's operands",
    );
}
