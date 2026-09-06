// audited: 2026-09-06
// src/wasm/AGENTS.md
//! What the WASM emitter does with an operand proof.
//!
//! An unproven arithmetic operation emits a float test and two arms; a proven
//! one emits the `i64` instruction alone. Neither shows up in a result, so
//! these compare the emitted module's size (docs/impl/lir.md).

use crate::lir::testkit::LirFixture;
use crate::lir::{BinOp, ClosureId, LirFunction, LirInstr, Reg, Terminator};
use crate::signals::Signal;
use crate::value::Arity;

/// fn(a, b) -> a `op` b, standalone-emittable, with the operation built by
/// `make_op`.
fn arith_closure(op: BinOp, make_op: fn(Reg, BinOp, Reg, Reg) -> LirInstr) -> LirFunction {
    LirFixture::new(Arity::Exact(2))
        .signal(Signal::silent())
        .closure_id(ClosureId(0))
        .num_params(2)
        .block(
            0,
            vec![
                LirInstr::LoadCapture {
                    dst: Reg(0),
                    index: 0,
                },
                LirInstr::LoadCapture {
                    dst: Reg(1),
                    index: 1,
                },
                make_op(Reg(2), op, Reg(0), Reg(1)),
            ],
            Terminator::Return(Reg(2)),
        )
        .build()
}

fn emitted_len(op: BinOp, make_op: fn(Reg, BinOp, Reg, Reg) -> LirInstr) -> usize {
    let vm = crate::vm::VM::new();
    super::super::emit::emit_single_closure(
        &arith_closure(op, make_op),
        None,
        vm.heap_ptr,
        std::ptr::null_mut(),
    )
    .expect("a two-parameter arithmetic closure is standalone-emittable")
    .wasm_bytes
    .len()
}

/// A proven operation drops the float test and the `f64` arm, so its module is
/// strictly smaller than the unproven one's (docs/impl/lir.md).
#[test]
fn a_proven_arithmetic_op_emits_no_float_guard() {
    // The counter-factual: the emitter already elides the guard when its own
    // `known_int` walk saw both operands defined by integer constants. These
    // operands are parameters, which that walk cannot type — so only the proof
    // can shrink this module.
    for op in [BinOp::Add, BinOp::Sub, BinOp::Mul, BinOp::Div] {
        let unproven = emitted_len(op, LirInstr::binop);
        let proven = emitted_len(op, LirInstr::int_binop);
        assert!(
            proven < unproven,
            "{op:?}: a proven op must emit less than an unproven one \
             ({proven} bytes vs {unproven})"
        );
    }
}
