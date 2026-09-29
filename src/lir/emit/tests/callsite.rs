// audited: 2026-09-29
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
fn tail_call_func(signal: crate::signals::Signal) -> LirFunction {
    LirFixture::new(Arity::Exact(0))
        .signal(signal)
        .block(
            0,
            vec![
                LirInstr::Const {
                    dst: Reg(0),
                    value: LirConst::Int(7),
                },
                LirInstr::Const {
                    dst: Reg(1),
                    value: LirConst::Nil,
                },
                LirInstr::Const {
                    dst: Reg(2),
                    value: LirConst::Int(1),
                },
                LirInstr::TailCall {
                    dst: Reg(3),
                    func: Reg(1),
                    args: vec![Reg(2)],
                    arity_checked: false,
                    region: StaticRegion::new(1).unwrap(),
                    defer_callee_release: false,
                    deferred_release_slot: None,
                    borrowed_arg_slots: vec![],
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
    let func = tail_call_func(crate::signals::Signal::yields());
    let (bytecode, _, call_sites) = Emitter::new().emit(&func);

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
    let func = tail_call_func(crate::signals::Signal::silent());
    let (_, _, call_sites) = Emitter::new().emit(&func);
    assert!(call_sites.is_empty(), "got {call_sites:?}");
}
