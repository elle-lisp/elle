// audited: 2026-11-13
//! A self tail call's trip starts as a fresh activation does: a slot this trip never wrote reads nil.
//!
//! docs/impl/jit.md
//! docs/impl/region/rules.md

use super::*;
use crate::hir::region::StaticRegion;
use crate::lir::{CmpOp, ConstRef, Label, Slots};
use crate::value::Value;

/// `f(n)`: a slot written only when `n` is 2, read back when `n` reaches 0,
/// and a self tail call `f(n - 1)` on every other trip.
///
/// ```text
/// block0: n == 2 ? block1 : block2
/// block1: local[0] ← 42; → block2
/// block2: n == 0 ? block3 : block4
/// block3: return local[0]
/// block4: tailcall self(n - 1)
/// ```
fn written_on_the_first_trip() -> LirOwned {
    LirFixture::new(Arity::Exact(1))
        .name("written-on-the-first-trip")
        .signal(Signal::silent())
        .num_params(1)
        .num_locals(1)
        .block(
            0,
            &[
                InstrRef::LoadCapture {
                    dst: Reg(0),
                    index: 0,
                },
                InstrRef::Const {
                    dst: Reg(1),
                    value: ConstRef::Int(2),
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
            &[
                InstrRef::Const {
                    dst: Reg(3),
                    value: ConstRef::Int(42),
                },
                InstrRef::StoreLocal {
                    slot: 0,
                    src: Reg(3),
                },
            ],
            Terminator::Jump(Label(2)),
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
                    value: ConstRef::Int(0),
                },
                InstrRef::compare(Reg(6), CmpOp::Eq, Reg(4), Reg(5)),
            ],
            Terminator::Branch {
                cond: Reg(6),
                then_label: Label(3),
                else_label: Label(4),
            },
        )
        .block(
            3,
            &[InstrRef::LoadLocal {
                dst: Reg(7),
                slot: 0,
            }],
            Terminator::Return(Reg(7)),
        )
        .block(
            4,
            &[
                InstrRef::LoadCapture {
                    dst: Reg(8),
                    index: 0,
                },
                InstrRef::Const {
                    dst: Reg(9),
                    value: ConstRef::Int(1),
                },
                InstrRef::binop(Reg(10), BinOp::Sub, Reg(8), Reg(9)),
                InstrRef::LoadSelf { dst: Reg(11) },
                InstrRef::TailCall {
                    dst: Reg(12),
                    func: Reg(11),
                    args: &[Reg(10)],
                    arity_checked: true,
                    region: StaticRegion::new(2).unwrap(),
                    defer_callee_release: false,
                    deferred_release_slot: None,
                    borrowed_arg_slots: Slots::new(&[]),
                },
            ],
            Terminator::Return(Reg(12)),
        )
        .emitted_sites()
        .build()
}

/// `f(2)` writes the slot on its first trip and returns it on its third. A
/// fresh activation reads `nil` from a slot it never wrote, and a release that
/// loads such a slot does nothing (docs/impl/region/rules.md). The executing
/// closure is any value at all here: `LoadSelf` reads the pair the caller
/// passes, and the self-call check compares against the same pair, so the
/// tail call is the native loop.
///
/// The counter-factual: the loop header jumped straight to the first block,
/// and the third trip read the 42 the first trip wrote. A value-routed release
/// of such a slot frees what an earlier trip moved to the call.
#[test]
fn a_self_tail_call_trip_reads_nil_from_a_slot_an_earlier_trip_wrote() {
    let lir = written_on_the_first_trip();
    let code = verified().compile(&lir.view()).expect("compiles");
    let me = Value::int(7);
    let mut vm = crate::vm::VM::new();
    let args = [Value::int(2)];
    let value = unsafe {
        code.call(
            std::ptr::null(),
            args.as_ptr(),
            1,
            &mut vm as *mut crate::vm::VM as *mut (),
            me.tag,
            me.payload,
        )
    }
    .to_value();
    assert!(
        value.is_nil(),
        "the third trip reads the slot only the first trip wrote; a fresh activation \
         reads nil there, got {value:?}",
    );
}
