// audited: 2026-11-17
//! A self tail call binds its new arguments as the prologue binds a call's: a celled parameter in a cell, a rest parameter in a list.
//!
//! docs/impl/jit.md

use super::*;
use crate::hir::region::StaticRegion;
use crate::lir::{CmpOp, ConstRef, Label};
use crate::value::Value;

/// Run `code` on `args`, with any value at all as the executing closure:
/// `LoadSelf` reads the pair the caller passes, and the self-call check
/// compares against the same pair, so each tail call to it is the native loop.
fn run_as_self(code: &crate::jit::JitCode, vm: &mut crate::vm::VM, args: &[Value]) -> Value {
    let me = Value::int(7);
    unsafe {
        code.call(
            std::ptr::null(),
            args.as_ptr(),
            args.len() as u32,
            vm as *mut crate::vm::VM as *mut (),
            me.tag,
            me.payload,
        )
    }
    .to_value()
}

/// `f(c, k)`, with `k` celled: while `c` is not 0 it tail-calls `f(c - 1, c)`,
/// and at 0 it answers `k`'s slot raw, cell and all.
///
/// ```text
/// block0: c == 0 ? block1 : block2
/// block1: return raw k
/// block2: tailcall self(c - 1, c)
/// ```
fn returns_its_celled_slot() -> LirOwned {
    LirFixture::new(Arity::Exact(2))
        .name("returns-its-celled-slot")
        .signal(Signal::silent())
        .num_params(2)
        .capture_params_mask(0b10)
        .block(
            0,
            &[
                InstrRef::LoadCapture {
                    dst: Reg(0),
                    index: 0,
                },
                InstrRef::Const {
                    dst: Reg(1),
                    value: ConstRef::Int(0),
                },
                InstrRef::compare(Reg(2), CmpOp::Eq, Reg(0), Reg(1)),
            ],
            Terminator::Branch {
                cond: Reg(2),
                then_label: Label(1),
                else_label: Label(2),
            },
        )
        .block(
            1,
            &[InstrRef::LoadCaptureRaw {
                dst: Reg(3),
                index: 1,
            }],
            Terminator::Return(Reg(3)),
        )
        .block(
            2,
            &[
                InstrRef::LoadCapture {
                    dst: Reg(4),
                    index: 0,
                },
                InstrRef::Const {
                    dst: Reg(5),
                    value: ConstRef::Int(1),
                },
                InstrRef::binop(Reg(6), BinOp::Sub, Reg(4), Reg(5)),
                InstrRef::LoadSelf { dst: Reg(7) },
                InstrRef::tail_call(
                    Reg(8),
                    Reg(7),
                    &[Reg(6), Reg(4)],
                    true,
                    StaticRegion::new(2).unwrap(),
                ),
            ],
            Terminator::Return(Reg(8)),
        )
        .emitted_sites()
        .build()
}

/// `f(2, 7)` takes two trips through the loop and answers `k`'s slot on the
/// third. The prologue wraps 7 in a cell, and each trip must wrap its new `k`
/// the same way, so the slot holds a cell around 1, the `c` of the last trip.
///
/// The counter-factual: the loop wrote the raw argument into the slot. The
/// slot then held the integer 1, and every read of `k` through its cell found
/// no cell.
#[test]
fn a_self_tail_call_wraps_a_celled_parameter_in_a_fresh_cell() {
    let lir = returns_its_celled_slot();
    let code = verified().compile(&lir.view()).expect("compiles");
    let mut vm = crate::vm::VM::new();
    let slot = run_as_self(&code, &mut vm, &[Value::int(2), Value::int(7)]);
    assert!(
        slot.is_capture_cell(),
        "a celled parameter's slot holds a cell after a self tail call, got {slot:?}",
    );
    assert_eq!(
        slot.capture_cell_get(),
        Some(Value::int(1)),
        "the cell holds the argument the last trip passed",
    );
}

/// `g(n & r)`: while `n` is not 0 it tail-calls `g(n - 1, 5)`, and at 0 it
/// answers `r`.
///
/// ```text
/// block0: n == 0 ? block1 : block2
/// block1: return r
/// block2: tailcall self(n - 1, 5)
/// ```
fn returns_its_rest_list() -> LirOwned {
    LirFixture::new(Arity::AtLeast(1))
        .name("returns-its-rest-list")
        .signal(Signal::silent())
        .vararg_kind(crate::hir::VarargKind::List)
        .num_params(2)
        .block(
            0,
            &[
                InstrRef::LoadCapture {
                    dst: Reg(0),
                    index: 0,
                },
                InstrRef::Const {
                    dst: Reg(1),
                    value: ConstRef::Int(0),
                },
                InstrRef::compare(Reg(2), CmpOp::Eq, Reg(0), Reg(1)),
            ],
            Terminator::Branch {
                cond: Reg(2),
                then_label: Label(1),
                else_label: Label(2),
            },
        )
        .block(
            1,
            &[InstrRef::LoadCapture {
                dst: Reg(3),
                index: 1,
            }],
            Terminator::Return(Reg(3)),
        )
        .block(
            2,
            &[
                InstrRef::LoadCapture {
                    dst: Reg(4),
                    index: 0,
                },
                InstrRef::Const {
                    dst: Reg(5),
                    value: ConstRef::Int(1),
                },
                InstrRef::binop(Reg(6), BinOp::Sub, Reg(4), Reg(5)),
                InstrRef::Const {
                    dst: Reg(7),
                    value: ConstRef::Int(5),
                },
                InstrRef::LoadSelf { dst: Reg(8) },
                InstrRef::tail_call(
                    Reg(9),
                    Reg(8),
                    &[Reg(6), Reg(7)],
                    true,
                    StaticRegion::new(2).unwrap(),
                ),
            ],
            Terminator::Return(Reg(9)),
        )
        .emitted_sites()
        .build()
}

/// `g(2)` takes two trips through the loop and answers `r` on the third. The
/// call passes one argument per parameter, so it takes the loop, and its last
/// argument is the one member of a fresh rest list: `(5)`.
///
/// The counter-factual: the loop wrote the raw argument into the rest
/// parameter, and `g(2)` answered `5` where the interpreter answers `(5)`.
#[test]
fn a_self_tail_call_collects_a_rest_parameter_into_a_list() {
    let lir = returns_its_rest_list();
    let code = verified().compile(&lir.view()).expect("compiles");
    let mut vm = crate::vm::VM::new();
    let rest = run_as_self(&code, &mut vm, &[Value::int(2)]);
    let pair = rest
        .as_pair()
        .unwrap_or_else(|| panic!("the rest parameter holds a list, got {rest:?}"));
    assert_eq!(
        pair.first,
        Value::int(5),
        "the list holds the last argument"
    );
    assert!(
        pair.rest.is_empty_list(),
        "the list holds the last argument alone, got rest {:?}",
        pair.rest,
    );
}
