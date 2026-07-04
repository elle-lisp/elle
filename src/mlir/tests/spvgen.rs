use super::*;

/// Build LIR: fn(x) { return x + 1.5 }  (float constant + mixed promotion)
fn make_float_add() -> LirFunction {
    let mut func = LirFunction::new(Arity::Exact(1));
    func.name = Some("float_add".to_string());
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
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Float(1.5),
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

#[test]
fn test_spirv_float_add() {
    let func = make_float_add();
    let spirv_bytes = lower_to_spirv(&func, 256).expect("float SPIR-V lowering should succeed");
    assert!(spirv_bytes.len() >= 20);
    assert_eq!(&spirv_bytes[0..4], &[0x03, 0x02, 0x23, 0x07]);
}

/// Build LIR: fn(x) { return 2.0 * 3.0 }  (pure float arithmetic)
fn make_float_mul() -> LirFunction {
    let mut func = LirFunction::new(Arity::Exact(1));
    func.name = Some("float_mul".to_string());
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
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Float(2.0),
        },
        s(),
    ));
    block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(2),
            value: LirConst::Float(3.0),
        },
        s(),
    ));
    block.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(3),
            op: BinOp::Mul,
            lhs: Reg(1),
            rhs: Reg(2),
        },
        s(),
    ));
    block.terminator = SpannedTerminator::new(Terminator::Return(Reg(3)), s());
    func.blocks.push(block);
    func.num_regs = 4;
    func
}

#[test]
fn test_spirv_float_mul() {
    let func = make_float_mul();
    let spirv_bytes =
        lower_to_spirv(&func, 256).expect("pure-float SPIR-V lowering should succeed");
    assert!(spirv_bytes.len() >= 20);
    assert_eq!(&spirv_bytes[0..4], &[0x03, 0x02, 0x23, 0x07]);
}

// ── SPIR-V reg/slot namespace collision regression ──────────────
//
// The SPIR-V emitter keyed its `regs`/`reg_types` maps by raw `u32`,
// inserting both register ids (`dst.0`) and local-slot ids
// (`*slot as u32`) into the *same* map. LIR allocates registers and
// local slots from two independent counters that both start at 0
// (`next_reg` and `num_locals` in src/lir/lower/mod.rs), so a slot id
// and a register id routinely collide numerically. When they do, a
// `StoreLocal` to a slot overwrites the entry for the register with the
// same number, corrupting every later use of that register.
//
// These tests build LIR that exercises such a collision and assert on
// the generated MLIR text, so they need neither a GPU nor mlir-translate.

/// Build LIR: fn() { var s; r0 = 10; r1 = 20; s = r1; return r0 + r0 }
///
/// Single block. Slot 0 (the only local) numerically collides with
/// register r0. `StoreLocal slot=0 src=r1` must not disturb r0, which is
/// read by the trailing `r0 + r0`. Correct lowering adds the const-10
/// value to itself; the conflated map adds the const-20 value instead.
fn make_storelocal_clobbers_reg() -> LirFunction {
    let mut func = LirFunction::new(Arity::Exact(0));
    func.name = Some("store_clobber".to_string());
    func.signal = Signal::errors();
    func.num_locals = 1;

    let mut b0 = BasicBlock::new(Label(0));
    // r0 = 10  → SSA name %c0_0
    b0.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Int(10),
        },
        s(),
    ));
    // r1 = 20  → SSA name %c0_1
    b0.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(20),
        },
        s(),
    ));
    // s = r1   (slot 0 ← r1); must leave r0 alone
    b0.instructions.push(SpannedInstr::new(
        LirInstr::StoreLocal {
            slot: 0,
            src: Reg(1),
        },
        s(),
    ));
    // r2 = r0 + r0
    b0.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(2),
            op: BinOp::Add,
            lhs: Reg(0),
            rhs: Reg(0),
        },
        s(),
    ));
    b0.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), s());
    func.blocks.push(b0);
    func.num_regs = 3;
    func
}

#[test]
fn test_spirv_storelocal_does_not_clobber_reg() {
    let func = make_storelocal_clobbers_reg();
    let text = super::spirv::generate_gpu_module(&func, 256)
        .expect("single-block lowering should succeed");
    // r0 = const 10 is named %c0_0; r1 = const 20 is named %c0_1.
    // The add reads r0 twice, so it must reference %c0_0 — not the
    // slot-0 store of r1 (%c0_1).
    assert!(
        text.contains("arith.addi %c0_0, %c0_0"),
        "r0+r0 must add the const-10 value (%c0_0) to itself; \
         slot store of r1 must not clobber r0.\nGenerated:\n{text}"
    );
    assert!(
        !text.contains("arith.addi %c0_1, %c0_1"),
        "slot/reg conflation made r0 read the const-20 value (%c0_1).\nGenerated:\n{text}"
    );
}

/// Build LIR: fn(x) { var s; s=0; if x>0 then s=100 else s=200; return s + x }
///
/// Multi-block, if-converted. The param x lives in r0 (`%arg0`); the
/// single local `s` is slot 0 — numerically colliding with r0. The
/// if-conversion merge writes its result under the slot key, which in a
/// conflated map clobbers r0. The trailing `s + x` must still read the
/// param (`%arg0`) for its right operand, not the merged if-result.
fn make_if_merge_clobbers_param() -> LirFunction {
    let mut func = LirFunction::new(Arity::Exact(1));
    func.name = Some("merge_clobber".to_string());
    func.signal = Signal::errors();
    func.num_locals = 1;

    // Block 0: load x → r0 (%arg0); s=0; cmp x>0; branch
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

    // Block 1: then — s = 100; jump merge
    let mut b1 = BasicBlock::new(Label(1));
    b1.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(3),
            value: LirConst::Int(100),
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

    // Block 2: else — s = 200; jump merge
    let mut b2 = BasicBlock::new(Label(2));
    b2.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(4),
            value: LirConst::Int(200),
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

    // Block 3: merge — s' = load slot 0; return s' + x
    let mut b3 = BasicBlock::new(Label(3));
    b3.instructions.push(SpannedInstr::new(
        LirInstr::LoadLocal {
            dst: Reg(5),
            slot: 0,
        },
        s(),
    ));
    b3.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(6),
            op: BinOp::Add,
            lhs: Reg(5),
            rhs: Reg(0),
        },
        s(),
    ));
    b3.terminator = SpannedTerminator::new(Terminator::Return(Reg(6)), s());

    func.blocks = vec![b0, b1, b2, b3];
    func.num_regs = 7;
    func
}

#[test]
fn test_spirv_if_merge_does_not_clobber_param() {
    let func = make_if_merge_clobbers_param();
    let text =
        super::spirv::generate_gpu_module(&func, 256).expect("multi-block lowering should succeed");
    // The final `s + x` must use %arg0 (the param) as its second operand.
    // Only the correct, un-conflated lowering keeps %arg0 live across the
    // if-conversion merge, where the merged result is written under the
    // slot-0 key (which collides with r0 = %arg0).
    assert!(
        text.contains(", %arg0 : i64"),
        "return s + x must read the param %arg0; the if-merge slot store \
         must not clobber r0.\nGenerated:\n{text}"
    );
}
