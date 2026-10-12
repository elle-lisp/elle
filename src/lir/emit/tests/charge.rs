// audited: 2026-10-12
//! The charge sites a function records: one at each instruction where the
//! interpreter charges fuel, with the operand stack the interpreter has there.
//!
//! src/lir/AGENTS.md
//! docs/impl/jit.md

use super::*;
use crate::hir::region::StaticRegion;
use crate::signals::Signal;

/// A signal that may signal, so the function records its charge sites. Any
/// signal bit does, an error included (src/lir/AGENTS.md).
fn may_signal() -> Signal {
    Signal::errors()
}

fn int(dst: u32, n: i64) -> InstrRef<'static> {
    InstrRef::Const {
        dst: Reg(dst),
        value: ConstRef::Int(n),
    }
}

fn region(n: u32) -> StaticRegion {
    StaticRegion::new(n).unwrap()
}

/// One block, with `signal`: an operand left on the stack below the call, then `call`, which
/// reads `Reg(1)` as its callee and `Reg(2)` as its argument or argument array,
/// then a return of `Reg(3)`. The constants are pushed in the order `call`
/// reads them, so it finds them on top and copies nothing.
fn call_func(signal: Signal, callee_below_args: bool, call: InstrRef<'_>) -> LirOwned {
    let callee = InstrRef::Const {
        dst: Reg(1),
        value: ConstRef::Nil,
    };
    let arg = int(2, 1);
    let [second, third] = if callee_below_args {
        [callee, arg]
    } else {
        [arg, callee]
    };
    LirFixture::new(Arity::Exact(0))
        .signal(signal)
        .block(
            0,
            &[int(0, 7), second, third, call],
            Terminator::Return(Reg(3)),
        )
        .build()
}

/// The offset of the first instruction named `opcode` in `bytecode`.
fn offset_of(bytecode: &Bytecode, opcode: &str) -> usize {
    decode(&bytecode.instructions)
        .iter()
        .find(|d| d.opcode == opcode)
        .unwrap_or_else(|| panic!("no {opcode} in the bytecode"))
        .at
}

/// Each call instruction charges fuel before it runs, so a pause there parks
/// the frame at the opcode with the callee and its arguments still on the
/// stack, and the interpreter runs the call again on resume.
#[test]
fn each_call_records_a_charge_site_at_its_opcode() {
    // Counter-factual: a site recorded after the call, where a call site sits,
    // resumes past an instruction that never ran, and the call's result is
    // whatever the resume pushes there.
    let call = |arity_checked| InstrRef::Call {
        dst: Reg(3),
        func: Reg(1),
        args: &[Reg(2)],
        arity_checked,
        region: region(1),
    };
    let tail_call = |arity_checked| InstrRef::TailCall {
        dst: Reg(3),
        func: Reg(1),
        args: &[Reg(2)],
        arity_checked,
        region: region(1),
        defer_callee_release: false,
        deferred_release_slot: None,
        borrowed_arg_slots: crate::lir::Slots::new(&[]),
    };
    let args_last = vec![Reg(0), Reg(2), Reg(1)];
    let array_last = vec![Reg(0), Reg(1), Reg(2)];
    let cases = [
        (
            "Call",
            call_func(may_signal(), false, call(false)),
            args_last.clone(),
        ),
        (
            "CallChecked",
            call_func(may_signal(), false, call(true)),
            args_last.clone(),
        ),
        (
            "Call",
            call_func(
                may_signal(),
                false,
                InstrRef::SuspendingCall {
                    dst: Reg(3),
                    func: Reg(1),
                    args: &[Reg(2)],
                    arity_checked: false,
                    region: region(1),
                },
            ),
            args_last.clone(),
        ),
        (
            "TailCall",
            call_func(may_signal(), false, tail_call(false)),
            args_last.clone(),
        ),
        (
            "TailCallChecked",
            call_func(may_signal(), false, tail_call(true)),
            args_last.clone(),
        ),
        (
            "CallArrayMut",
            call_func(
                may_signal(),
                true,
                InstrRef::CallArrayMut {
                    dst: Reg(3),
                    func: Reg(1),
                    args: Reg(2),
                    region: region(1),
                    args_region: region(2),
                },
            ),
            array_last.clone(),
        ),
        (
            "TailCallArrayMut",
            call_func(
                may_signal(),
                true,
                InstrRef::TailCallArrayMut {
                    func: Reg(1),
                    args: Reg(2),
                    region: region(1),
                    args_region: region(2),
                },
            ),
            array_last.clone(),
        ),
    ];
    for (opcode, func, stack) in cases {
        let compiled = emitter().emit(&func.view());
        let sites = &compiled.charge_sites;
        assert_eq!(sites.len(), 1, "{opcode}: one call, one charge site");
        assert_eq!(
            sites[0].ip,
            offset_of(&compiled.bytecode, opcode),
            "{opcode}: the site is the opcode's own offset",
        );
        assert_eq!(
            sites[0].stack_regs, stack,
            "{opcode}: the stack still holds the callee and its arguments",
        );
    }
}

/// `while` in miniature, with a value live across the loop: the entry leaves
/// `Reg(0)` on the stack, the header compares the counter in local 0 with it,
/// and the body jumps back to the header.
fn loop_func(signal: Signal) -> LirOwned {
    LirFixture::new(Arity::Exact(0))
        .signal(signal)
        .num_locals(1)
        .block(
            0,
            &[
                int(0, 10),
                int(1, 0),
                InstrRef::StoreLocal {
                    slot: 0,
                    src: Reg(1),
                },
            ],
            Terminator::Jump(Label(1)),
        )
        .block(
            1,
            &[
                InstrRef::LoadLocal {
                    dst: Reg(2),
                    slot: 0,
                },
                InstrRef::compare(Reg(3), CmpOp::Lt, Reg(2), Reg(0)),
            ],
            Terminator::Branch {
                cond: Reg(3),
                then_label: Label(2),
                else_label: Label(3),
            },
        )
        .block(
            2,
            &[
                InstrRef::LoadLocal {
                    dst: Reg(4),
                    slot: 0,
                },
                int(5, 1),
                InstrRef::binop(Reg(6), BinOp::Add, Reg(4), Reg(5)),
                InstrRef::StoreLocal {
                    slot: 0,
                    src: Reg(6),
                },
            ],
            Terminator::Jump(Label(1)),
        )
        .block(3, &[], Terminator::Return(Reg(0)))
        .build()
}

/// Only a backward jump charges, so a loop records one site, at the jump that
/// closes it. The stack there is the one the edge trims to, which is the
/// header's.
#[test]
fn a_backward_jump_records_a_charge_site_with_the_stack_its_edge_trims_to() {
    // Counter-factual: a site at every jump charges the entry's jump into the
    // loop and both arms of the branch, and spends three times the budget the
    // interpreter spends on the same loop.
    let compiled = emitter().emit(&loop_func(may_signal()).view());
    let sites = &compiled.charge_sites;
    assert_eq!(
        sites.iter().map(|s| s.ip).collect::<Vec<_>>(),
        interpreter_charge_points(&compiled.bytecode.instructions),
        "one site, at the backward jump",
    );
    assert_eq!(sites.len(), 1, "the forward jumps record nothing");
    assert_eq!(
        sites[0].stack_regs,
        vec![Reg(0)],
        "the stack the back edge trims to, with the live limit on it",
    );
    assert_eq!(sites[0].num_locals, 1, "the local below the stack");
}

/// A `Branch` emits a `Jump` to its `then` block, and when that block was
/// emitted first the jump runs backward and the interpreter charges it.
#[test]
fn a_branch_back_to_an_emitted_block_records_a_charge_site() {
    // The trap: a `do`-`while` shape closes its loop with the branch rather
    // than with a `Jump` terminator, so a rule that looked only at `Jump`
    // terminators misses the one charge the loop makes.
    let func = LirFixture::new(Arity::Exact(0))
        .signal(may_signal())
        .num_locals(1)
        .block(
            0,
            &[
                int(0, 0),
                InstrRef::StoreLocal {
                    slot: 0,
                    src: Reg(0),
                },
            ],
            Terminator::Jump(Label(1)),
        )
        .block(
            1,
            &[
                InstrRef::LoadLocal {
                    dst: Reg(1),
                    slot: 0,
                },
                int(2, 1),
                InstrRef::binop(Reg(3), BinOp::Add, Reg(1), Reg(2)),
                InstrRef::StoreLocal {
                    slot: 0,
                    src: Reg(3),
                },
                InstrRef::LoadLocal {
                    dst: Reg(4),
                    slot: 0,
                },
                int(5, 10),
                InstrRef::compare(Reg(6), CmpOp::Lt, Reg(4), Reg(5)),
            ],
            Terminator::Branch {
                cond: Reg(6),
                then_label: Label(1),
                else_label: Label(2),
            },
        )
        .block(
            2,
            &[InstrRef::LoadLocal {
                dst: Reg(7),
                slot: 0,
            }],
            Terminator::Return(Reg(7)),
        )
        .build();
    let compiled = emitter().emit(&func.view());
    let points = interpreter_charge_points(&compiled.bytecode.instructions);
    assert_eq!(points.len(), 1, "the branch's jump back to its own block");
    assert_eq!(
        compiled
            .charge_sites
            .iter()
            .map(|s| s.ip)
            .collect::<Vec<_>>(),
        points,
    );
    assert!(
        compiled.charge_sites[0].stack_regs.is_empty(),
        "the branch consumed its condition before the jump",
    );
}

/// A silent function records no charge site, for its charges never pause
/// (docs/signals/inference.md).
#[test]
fn a_silent_function_records_no_charge_site() {
    // Counter-factual: a site in a silent function is a park the function's
    // caller never checks for, since a silent caller records no call site.
    let call = InstrRef::Call {
        dst: Reg(3),
        func: Reg(1),
        args: &[Reg(2)],
        arity_checked: false,
        region: region(1),
    };
    for (what, func) in [
        ("a call", call_func(Signal::silent(), false, call)),
        ("a loop", loop_func(Signal::silent())),
    ] {
        let compiled = emitter().emit(&func.view());
        assert!(
            compiled.charge_sites.is_empty(),
            "{what} in a silent function records no charge site; got {:?}",
            compiled.charge_sites
        );
    }
}

/// Over real code, a function that may signal records a site at exactly the
/// offsets where the interpreter charges, in bytecode order, and a silent one
/// records none.
#[test]
fn charge_sites_are_where_the_interpreter_charges() {
    // Counter-factual: a compiled function that misses one charging
    // instruction spends less than its interpreted self, and a driver stepping
    // it one unit at a time sees it run past where the interpreter paused.
    for source in [CORPUS, crate::pipeline::sources::STDLIB] {
        let lambdas = compiled_lambdas(source);
        assert!(!lambdas.is_empty(), "the source compiles to lambdas");
        assert!(
            lambdas
                .iter()
                .any(|(_, _, lir)| lir.view().signal().may_suspend()),
            "the source holds functions that may signal"
        );
        for (name, bytecode, lir) in lambdas {
            let lir = lir.view();
            let expected = if lir.signal().may_suspend() {
                interpreter_charge_points(&bytecode)
            } else {
                Vec::new()
            };
            assert_eq!(
                lir.charge_sites().map(|s| s.resume_ip).collect::<Vec<_>>(),
                expected,
                "{name} ({:?}): a charge site at each offset where the interpreter \
                 charges, in a function that may signal",
                lir.signal(),
            );
            for site in lir.charge_sites() {
                assert_eq!(
                    site.num_locals,
                    lir.num_locals(),
                    "{name}: each site counts the function's locals",
                );
            }
        }
    }
}
