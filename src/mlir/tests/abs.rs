// audited: 2026-10-06
// docs/impl/mlir.md
//! One branching function, `abs`, run through both MLIR tiers: the CPU JIT
//! computes it, and the SPIR-V path lowers its two blocks to a module.

use super::*;

/// Build LIR: fn(x) { if x > 0 then x else -x }  (absolute value)
fn make_abs() -> LirOwned {
    LirFixture::new(Arity::Exact(1))
        .name("abs")
        .signal(Signal::errors())
        // Block 0: entry — load param, compare > 0, branch
        .block(
            0,
            &[
                InstrRef::LoadCaptureRaw {
                    dst: Reg(0),
                    index: 0,
                },
                InstrRef::Const {
                    dst: Reg(1),
                    value: ConstRef::Int(0),
                },
                InstrRef::compare(Reg(2), CmpOp::Gt, Reg(0), Reg(1)),
            ],
            Terminator::Branch {
                cond: Reg(2),
                then_label: Label(1),
                else_label: Label(2),
            },
        )
        // Block 1: then — return x
        .block(1, &[], Terminator::Return(Reg(0)))
        // Block 2: else — return 0 - x
        .block(
            2,
            &[InstrRef::binop(Reg(3), BinOp::Sub, Reg(1), Reg(0))],
            Terminator::Return(Reg(3)),
        )
        .build()
}

#[test]
fn test_execute_abs_positive() {
    assert_eq!(mlir_call(&make_abs().view(), &[42]).unwrap(), 42);
}

#[test]
fn test_execute_abs_negative() {
    assert_eq!(mlir_call(&make_abs().view(), &[-7]).unwrap(), 7);
}

#[test]
fn test_execute_abs_zero() {
    assert_eq!(mlir_call(&make_abs().view(), &[0]).unwrap(), 0);
}

#[test]
fn test_spirv_abs() {
    let func = make_abs();
    let spirv_bytes =
        lower_to_spirv(&func.view(), 256).expect("multi-block SPIR-V lowering should succeed");
    assert!(spirv_bytes.len() >= 20);
    assert_eq!(&spirv_bytes[0..4], &[0x03, 0x02, 0x23, 0x07]);
}
