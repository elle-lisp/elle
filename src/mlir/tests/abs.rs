use super::*;

/// Build LIR: fn(x) { if x > 0 then x else -x }  (absolute value)
fn make_abs() -> LirFunction {
    LirFixture::new(Arity::Exact(1))
        .name("abs")
        .signal(Signal::errors())
        // Block 0: entry — load param, compare > 0, branch
        .block(
            0,
            vec![
                LirInstr::LoadCaptureRaw {
                    dst: Reg(0),
                    index: 0,
                },
                LirInstr::Const {
                    dst: Reg(1),
                    value: LirConst::Int(0),
                },
                LirInstr::Compare {
                    dst: Reg(2),
                    op: CmpOp::Gt,
                    lhs: Reg(0),
                    rhs: Reg(1),
                },
            ],
            Terminator::Branch {
                cond: Reg(2),
                then_label: Label(1),
                else_label: Label(2),
            },
        )
        // Block 1: then — return x
        .block(1, vec![], Terminator::Return(Reg(0)))
        // Block 2: else — return 0 - x
        .block(
            2,
            vec![LirInstr::BinOp {
                dst: Reg(3),
                op: BinOp::Sub,
                lhs: Reg(1),
                rhs: Reg(0),
            }],
            Terminator::Return(Reg(3)),
        )
        .build()
}

#[test]
fn test_execute_abs_positive() {
    assert_eq!(mlir_call(&make_abs(), &[42]).unwrap(), 42);
}

#[test]
fn test_execute_abs_negative() {
    assert_eq!(mlir_call(&make_abs(), &[-7]).unwrap(), 7);
}

#[test]
fn test_execute_abs_zero() {
    assert_eq!(mlir_call(&make_abs(), &[0]).unwrap(), 0);
}

#[test]
fn test_spirv_abs() {
    let func = make_abs();
    let spirv_bytes =
        lower_to_spirv(&func, 256).expect("multi-block SPIR-V lowering should succeed");
    assert!(spirv_bytes.len() >= 20);
    assert_eq!(&spirv_bytes[0..4], &[0x03, 0x02, 0x23, 0x07]);
}
