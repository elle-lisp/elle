use super::*;

/// Build LIR: fn(x) { if x > 0 then x else -x }  (absolute value)
fn make_abs() -> LirFunction {
    let mut func = LirFunction::new(Arity::Exact(1));
    func.name = Some("abs".to_string());
    func.signal = Signal::errors();

    // Block 0: entry — load param, compare > 0, branch
    let mut b0 = BasicBlock::new(Label(0));
    b0.instructions.push(SpannedInstr::new(
        LirInstr::LoadCaptureRaw {
            dst: Reg(0),
            index: 0,
        },
        s(),
    ));
    b0.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(0),
        },
        s(),
    ));
    b0.instructions.push(SpannedInstr::new(
        LirInstr::Compare {
            dst: Reg(2),
            op: CmpOp::Gt,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        s(),
    ));
    b0.terminator = SpannedTerminator::new(
        Terminator::Branch {
            cond: Reg(2),
            then_label: Label(1),
            else_label: Label(2),
        },
        s(),
    );

    // Block 1: then — return x
    let mut b1 = BasicBlock::new(Label(1));
    b1.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), s());

    // Block 2: else — return 0 - x
    let mut b2 = BasicBlock::new(Label(2));
    b2.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(3),
            op: BinOp::Sub,
            lhs: Reg(1),
            rhs: Reg(0),
        },
        s(),
    ));
    b2.terminator = SpannedTerminator::new(Terminator::Return(Reg(3)), s());

    func.blocks = vec![b0, b1, b2];
    func.num_regs = 4;
    func
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
