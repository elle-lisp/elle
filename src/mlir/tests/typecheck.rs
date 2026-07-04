use super::*;

// ── Mixed-type slot rejection ─────────────────────────────────

/// Build LIR: fn(x) { var s = 0; if x > 0 then s = 1.5 else s = 2; return s }
/// This has a mixed-type local slot (Int in one branch, Float in another).
fn make_mixed_type_slot() -> LirFunction {
    let mut func = LirFunction::new(Arity::Exact(1));
    func.name = Some("mixed_slot".to_string());
    func.signal = Signal::errors();
    func.num_locals = 1;

    // Block 0: entry — load param, store 0 to slot, compare, branch
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
        LirInstr::StoreLocal {
            slot: 0,
            src: Reg(1),
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

    // Block 1: then — store 1.5 (Float) to slot, jump to merge
    let mut b1 = BasicBlock::new(Label(1));
    b1.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(3),
            value: LirConst::Float(1.5),
        },
        s(),
    ));
    b1.instructions.push(SpannedInstr::new(
        LirInstr::StoreLocal {
            slot: 0,
            src: Reg(3),
        },
        s(),
    ));
    b1.terminator = SpannedTerminator::new(Terminator::Jump(Label(3)), s());

    // Block 2: else — store 2 (Int) to slot, jump to merge
    let mut b2 = BasicBlock::new(Label(2));
    b2.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(4),
            value: LirConst::Int(2),
        },
        s(),
    ));
    b2.instructions.push(SpannedInstr::new(
        LirInstr::StoreLocal {
            slot: 0,
            src: Reg(4),
        },
        s(),
    ));
    b2.terminator = SpannedTerminator::new(Terminator::Jump(Label(3)), s());

    // Block 3: merge — load slot, return
    let mut b3 = BasicBlock::new(Label(3));
    b3.instructions.push(SpannedInstr::new(
        LirInstr::LoadLocal {
            dst: Reg(5),
            slot: 0,
        },
        s(),
    ));
    b3.terminator = SpannedTerminator::new(Terminator::Return(Reg(5)), s());

    func.blocks = vec![b0, b1, b2, b3];
    func.num_regs = 6;
    func
}

#[test]
fn test_reject_mixed_type_slot() {
    let func = make_mixed_type_slot();
    // Use check_slot_types directly to avoid partially constructing
    // MLIR ops (melior cleanup of partial modules can crash).
    let err = check_slot_types(&func, 0, 0, 0).unwrap_err();
    assert!(
        err.contains("mixed-type local slot"),
        "should reject cross-block mixed-type slot: {}",
        err
    );
}

/// Build LIR: fn() { var s; r0 = 5; r1 = 2.5; s = r1; <block 1> s = r0; return s }
///
/// Slot 0 (the only local) numerically collides with register r0. The
/// slot is genuinely mixed-type: Float (from r1) in block 0, Int (from
/// r0) in block 1 — `check_slot_types` must reject it. The type checker
/// infers the store source's type from its `reg_types` map; if that map
/// is keyed by raw `u32`, block 0's `StoreLocal slot=0` overwrites the
/// entry for register r0 with the slot's Float type, so block 1's
/// `StoreLocal slot=0 src=r0` mis-reads r0 as Float and the Float/Int
/// conflict slips through undetected (a false negative).
fn make_slot_reg_collision_hides_mixed_type() -> LirFunction {
    let mut func = LirFunction::new(Arity::Exact(0));
    func.name = Some("collision_hides_mixed".to_string());
    func.signal = Signal::errors();
    func.num_locals = 1;

    // Block 0: r0 = 5 (Int); r1 = 2.5 (Float); s = r1 (Float)
    let mut b0 = BasicBlock::new(Label(0));
    b0.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Int(5),
        },
        s(),
    ));
    b0.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Float(2.5),
        },
        s(),
    ));
    b0.instructions.push(SpannedInstr::new(
        LirInstr::StoreLocal {
            slot: 0,
            src: Reg(1),
        },
        s(),
    ));
    b0.terminator = SpannedTerminator::new(Terminator::Jump(Label(1)), s());

    // Block 1: s = r0 (Int) — conflicts with the Float store in block 0
    let mut b1 = BasicBlock::new(Label(1));
    b1.instructions.push(SpannedInstr::new(
        LirInstr::StoreLocal {
            slot: 0,
            src: Reg(0),
        },
        s(),
    ));
    b1.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), s());

    func.blocks = vec![b0, b1];
    func.num_regs = 2;
    func
}

#[test]
fn test_reject_mixed_type_slot_under_reg_collision() {
    let func = make_slot_reg_collision_hides_mixed_type();
    // Slot 0 is Float in block 0 and Int in block 1: a real conflict that
    // must be caught even though slot id 0 collides with register r0.
    let err = check_slot_types(&func, 0, 0, 0).unwrap_err();
    assert!(
        err.contains("mixed-type local slot"),
        "slot/reg key collision must not hide a genuine mixed-type slot: {:?}",
        check_slot_types(&func, 0, 0, 0)
    );
}

/// Build LIR: fn(x) { var s = 0; s = 1.5; return s }
/// Sequential reassignment within a single block — should succeed.
fn make_sequential_reassign() -> LirFunction {
    let mut func = LirFunction::new(Arity::Exact(1));
    func.name = Some("seq_reassign".to_string());
    func.signal = Signal::errors();
    func.num_locals = 1;

    let mut b0 = BasicBlock::new(Label(0));
    // Load param (unused, just for arity)
    b0.instructions.push(SpannedInstr::new(
        LirInstr::LoadCaptureRaw {
            dst: Reg(0),
            index: 0,
        },
        s(),
    ));
    // var s = 0 (Int)
    b0.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(0),
        },
        s(),
    ));
    b0.instructions.push(SpannedInstr::new(
        LirInstr::StoreLocal {
            slot: 0,
            src: Reg(1),
        },
        s(),
    ));
    // s = 1.5 (Float — same block, sequential reassignment)
    b0.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(2),
            value: LirConst::Float(1.5),
        },
        s(),
    ));
    b0.instructions.push(SpannedInstr::new(
        LirInstr::StoreLocal {
            slot: 0,
            src: Reg(2),
        },
        s(),
    ));
    // Load and return s
    b0.instructions.push(SpannedInstr::new(
        LirInstr::LoadLocal {
            dst: Reg(3),
            slot: 0,
        },
        s(),
    ));
    b0.terminator = SpannedTerminator::new(Terminator::Return(Reg(3)), s());

    func.blocks = vec![b0];
    func.num_regs = 4;
    func
}

#[test]
fn test_accept_sequential_reassign() {
    let func = make_sequential_reassign();
    // Should lower successfully — sequential reassignment in same block is fine.
    let mlir_text = lower_to_mlir(&func).expect("sequential reassignment should succeed");
    assert!(
        mlir_text.contains("func.func"),
        "should produce valid MLIR: {}",
        mlir_text
    );
}

#[test]
fn test_execute_sequential_reassign() {
    let func = make_sequential_reassign();
    let result = mlir_call(&func, &[0]).expect("execution should succeed");
    // s was reassigned from 0 (Int) to 1.5 (Float); result is f64 bits
    assert_eq!(result, 1.5f64.to_bits() as i64);
}
