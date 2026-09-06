// audited: 2026-09-06
// docs/impl/spirv.md
//! What the SPIR-V emitter makes of float arithmetic, and of a local slot whose
//! id equals a register's.

use super::*;

/// Build LIR: fn(x) { return x + 1.5 }  (float constant + mixed promotion)
fn make_float_add() -> LirFunction {
    LirFixture::new(Arity::Exact(1))
        .name("float_add")
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
                    value: LirConst::Float(1.5),
                },
                LirInstr::binop(Reg(2), BinOp::Add, Reg(0), Reg(1)),
            ],
            Terminator::Return(Reg(2)),
        )
        .build()
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
    LirFixture::new(Arity::Exact(1))
        .name("float_mul")
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
                    value: LirConst::Float(2.0),
                },
                LirInstr::Const {
                    dst: Reg(2),
                    value: LirConst::Float(3.0),
                },
                LirInstr::binop(Reg(3), BinOp::Mul, Reg(1), Reg(2)),
            ],
            Terminator::Return(Reg(3)),
        )
        .build()
}

#[test]
fn test_spirv_float_mul() {
    let func = make_float_mul();
    let spirv_bytes =
        lower_to_spirv(&func, 256).expect("pure-float SPIR-V lowering should succeed");
    assert!(spirv_bytes.len() >= 20);
    assert_eq!(&spirv_bytes[0..4], &[0x03, 0x02, 0x23, 0x07]);
}

// ── Registers and local slots are separate namespaces ───────────
//
// The trap: LIR draws register ids and local-slot ids from two counters that
// both start at 0 (`next_reg` and `num_locals`), so the two collide routinely.
// A `u32`-keyed map holding both lets a store to a slot overwrite the register
// wearing the same number.
//
// Both tests assert on the generated MLIR text, so neither needs a GPU or
// mlir-translate.

/// Build LIR: fn() { var s; r0 = 10; r1 = 20; s = r1; return r0 + r0 }
///
/// Slot 0 shares its number with r0. Correct lowering adds the const-10 value
/// to itself; a conflated map adds the const-20 value instead.
fn make_storelocal_clobbers_reg() -> LirFunction {
    LirFixture::new(Arity::Exact(0))
        .name("store_clobber")
        .signal(Signal::errors())
        .num_locals(1)
        .block(
            0,
            vec![
                // r0 = 10  → SSA name %c0_0
                LirInstr::Const {
                    dst: Reg(0),
                    value: LirConst::Int(10),
                },
                // r1 = 20  → SSA name %c0_1
                LirInstr::Const {
                    dst: Reg(1),
                    value: LirConst::Int(20),
                },
                // s = r1   (slot 0 ← r1); must leave r0 alone
                LirInstr::StoreLocal {
                    slot: 0,
                    src: Reg(1),
                },
                // r2 = r0 + r0
                LirInstr::binop(Reg(2), BinOp::Add, Reg(0), Reg(0)),
            ],
            Terminator::Return(Reg(2)),
        )
        .build()
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
/// The same collision across an if-conversion: param `x` is r0, local `s` is
/// slot 0, and the merge writes its result under the slot key. The trailing
/// `s + x` must still read `%arg0`.
fn make_if_merge_clobbers_param() -> LirFunction {
    LirFixture::new(Arity::Exact(1))
        .name("merge_clobber")
        .signal(Signal::errors())
        .num_locals(1)
        // Block 0: load x → r0 (%arg0); s=0; cmp x>0; branch
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
                LirInstr::StoreLocal {
                    slot: 0,
                    src: Reg(1),
                },
                LirInstr::compare(Reg(2), CmpOp::Gt, Reg(0), Reg(1)),
            ],
            Terminator::Branch {
                cond: Reg(2),
                then_label: Label(1),
                else_label: Label(2),
            },
        )
        // Block 1: then — s = 100; jump merge
        .block(
            1,
            vec![
                LirInstr::Const {
                    dst: Reg(3),
                    value: LirConst::Int(100),
                },
                LirInstr::StoreLocal {
                    slot: 0,
                    src: Reg(3),
                },
            ],
            Terminator::Jump(Label(3)),
        )
        // Block 2: else — s = 200; jump merge
        .block(
            2,
            vec![
                LirInstr::Const {
                    dst: Reg(4),
                    value: LirConst::Int(200),
                },
                LirInstr::StoreLocal {
                    slot: 0,
                    src: Reg(4),
                },
            ],
            Terminator::Jump(Label(3)),
        )
        // Block 3: merge — s' = load slot 0; return s' + x
        .block(
            3,
            vec![
                LirInstr::LoadLocal {
                    dst: Reg(5),
                    slot: 0,
                },
                LirInstr::binop(Reg(6), BinOp::Add, Reg(5), Reg(0)),
            ],
            Terminator::Return(Reg(6)),
        )
        .build()
}

#[test]
fn test_spirv_if_merge_does_not_clobber_param() {
    let func = make_if_merge_clobbers_param();
    let text =
        super::spirv::generate_gpu_module(&func, 256).expect("multi-block lowering should succeed");
    assert!(
        text.contains(", %arg0 : i64"),
        "return s + x must read the param %arg0; the if-merge slot store \
         must not clobber r0.\nGenerated:\n{text}"
    );
}
