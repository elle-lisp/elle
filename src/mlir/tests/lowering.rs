// audited: 2026-09-06
// docs/impl/mlir.md
//! The straight-line base case: an add and a constant, through MLIR text, the
//! CPU tier, and a SPIR-V module.

use super::*;

/// Build LIR: fn(a, b) { return a + b }
fn make_add() -> LirFunction {
    LirFixture::new(Arity::Exact(2))
        .name("add")
        .signal(Signal::errors())
        .block(
            0,
            vec![
                LirInstr::LoadCaptureRaw {
                    dst: Reg(0),
                    index: 0,
                },
                LirInstr::LoadCaptureRaw {
                    dst: Reg(1),
                    index: 1,
                },
                LirInstr::binop(Reg(2), BinOp::Add, Reg(0), Reg(1)),
            ],
            Terminator::Return(Reg(2)),
        )
        .build()
}

/// Build LIR: fn() { return 42 }
fn make_const() -> LirFunction {
    LirFixture::new(Arity::Exact(0))
        .name("the_answer")
        .block(
            0,
            vec![LirInstr::Const {
                dst: Reg(0),
                value: LirConst::Int(42),
            }],
            Terminator::Return(Reg(0)),
        )
        .build()
}

// ── Lowering tests ──────────────────────────────────────────────

#[test]
fn test_lower_add() {
    let mlir_text = lower_to_mlir(&make_add()).expect("lowering should succeed");
    assert!(
        mlir_text.contains("arith.addi"),
        "should contain arith.addi: {}",
        mlir_text
    );
    assert!(
        mlir_text.contains("func.func"),
        "should contain func.func: {}",
        mlir_text
    );
}

#[test]
fn test_lower_constant() {
    let mlir_text = lower_to_mlir(&make_const()).expect("lowering should succeed");
    assert!(
        mlir_text.contains("42"),
        "should contain constant 42: {}",
        mlir_text
    );
}

// ── Execution tests ─────────────────────────────────────────────

#[test]
fn test_execute_constant() {
    let result = mlir_call(&make_const(), &[]).expect("execution should succeed");
    assert_eq!(result, 42);
}

#[test]
fn test_execute_add() {
    let result = mlir_call(&make_add(), &[10, 32]).expect("execution should succeed");
    assert_eq!(result, 42);
}

#[test]
fn test_execute_add_negative() {
    let result = mlir_call(&make_add(), &[-5, 15]).expect("execution should succeed");
    assert_eq!(result, 10);
}

// ── SPIR-V tests ─────────────────────────────────────────────

#[test]
fn test_spirv_add() {
    let func = make_add();
    let spirv_bytes = lower_to_spirv(&func, 256).expect("SPIR-V lowering should succeed");
    assert!(
        spirv_bytes.len() >= 20,
        "SPIR-V should be non-trivial: {} bytes",
        spirv_bytes.len()
    );
    // SPIR-V magic number: 0x07230203
    assert_eq!(
        &spirv_bytes[0..4],
        &[0x03, 0x02, 0x23, 0x07],
        "SPIR-V magic number"
    );
}
