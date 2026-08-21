use super::*;

// ── Bool return tests ──────────────────────────────────────────

/// Build LIR: fn(x) { return x > 0 }
fn make_compare() -> LirFunction {
    LirFixture::new(Arity::Exact(1))
        .name("compare_gt")
        .signal(Signal::errors())
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
            Terminator::Return(Reg(2)),
        )
        .build()
}

#[test]
fn test_lower_compare() {
    let mlir_text = lower_to_mlir(&make_compare()).expect("lowering should succeed");
    assert!(
        mlir_text.contains("arith.cmpi"),
        "should contain arith.cmpi: {}",
        mlir_text
    );
}

#[test]
fn test_execute_compare_positive() {
    let result = mlir_call(&make_compare(), &[5]).expect("execution should succeed");
    assert_eq!(result, 1, "5 > 0 should be 1 (true)");
}

#[test]
fn test_execute_compare_negative() {
    let result = mlir_call(&make_compare(), &[-1]).expect("execution should succeed");
    assert_eq!(result, 0, "-1 > 0 should be 0 (false)");
}

#[test]
fn test_compare_return_type_is_bool() {
    let context = lower::create_context();
    let func = make_compare();
    let (_, ret_type) =
        lower::lower_to_module(&context, &func, 0, 0, 0).expect("lowering should succeed");
    assert_eq!(ret_type, ScalarType::Bool, "compare return should be Bool");
}
