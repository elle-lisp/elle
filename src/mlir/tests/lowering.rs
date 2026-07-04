use super::*;

/// Build LIR: fn(a, b) { return a + b }
fn make_add() -> LirFunction {
    let mut func = LirFunction::new(Arity::Exact(2));
    func.name = Some("add".to_string());
    func.signal = Signal::errors();
    let mut block = BasicBlock::new(Label(0));
    block.instructions.push(SpannedInstr::new(
        LirInstr::LoadCaptureRaw {
            dst: Reg(0),
            index: 0,
        },
        s(),
    ));
    block.instructions.push(SpannedInstr::new(
        LirInstr::LoadCaptureRaw {
            dst: Reg(1),
            index: 1,
        },
        s(),
    ));
    block.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(2),
            op: BinOp::Add,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        s(),
    ));
    block.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), s());
    func.blocks.push(block);
    func.num_regs = 3;
    func
}

/// Build LIR: fn() { return 42 }
fn make_const() -> LirFunction {
    let mut func = LirFunction::new(Arity::Exact(0));
    func.name = Some("the_answer".to_string());
    let mut block = BasicBlock::new(Label(0));
    block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Int(42),
        },
        s(),
    ));
    block.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), s());
    func.blocks.push(block);
    func.num_regs = 1;
    func
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
