// audited: 2026-09-06
// docs/impl/mlir.md
//! What `check_slot_types` accepts: a local slot rewritten within one block, but
//! never one holding a float on one path and an int on another.

use super::*;

// ── Mixed-type slot rejection ─────────────────────────────────

/// Build LIR: fn(x) { var s = 0; if x > 0 then s = 1.5 else s = 2; return s }
/// This has a mixed-type local slot (Int in one branch, Float in another).
fn make_mixed_type_slot() -> LirFunction {
    LirFixture::new(Arity::Exact(1))
        .name("mixed_slot")
        .signal(Signal::errors())
        .num_locals(1)
        // Block 0: entry — load param, store 0 to slot, compare, branch
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
        // Block 1: then — store 1.5 (Float) to slot, jump to merge
        .block(
            1,
            vec![
                LirInstr::Const {
                    dst: Reg(3),
                    value: LirConst::Float(1.5),
                },
                LirInstr::StoreLocal {
                    slot: 0,
                    src: Reg(3),
                },
            ],
            Terminator::Jump(Label(3)),
        )
        // Block 2: else — store 2 (Int) to slot, jump to merge
        .block(
            2,
            vec![
                LirInstr::Const {
                    dst: Reg(4),
                    value: LirConst::Int(2),
                },
                LirInstr::StoreLocal {
                    slot: 0,
                    src: Reg(4),
                },
            ],
            Terminator::Jump(Label(3)),
        )
        // Block 3: merge — load slot, return
        .block(
            3,
            vec![LirInstr::LoadLocal {
                dst: Reg(5),
                slot: 0,
            }],
            Terminator::Return(Reg(5)),
        )
        .build()
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
/// Slot 0 is float in block 0 and int in block 1 — the same conflict the test
/// above rejects, but arranged so that slot id 0 and register r0 share a number.
///
/// The trap: one `u32`-keyed map for slots and registers alike lets the float
/// store to slot 0 overwrite what r0 holds, after which block 1 reads r0 as a
/// float and the checker reports no conflict at all.
fn make_slot_reg_collision_hides_mixed_type() -> LirFunction {
    LirFixture::new(Arity::Exact(0))
        .name("collision_hides_mixed")
        .signal(Signal::errors())
        .num_locals(1)
        // Block 0: r0 = 5 (Int); r1 = 2.5 (Float); s = r1 (Float)
        .block(
            0,
            vec![
                LirInstr::Const {
                    dst: Reg(0),
                    value: LirConst::Int(5),
                },
                LirInstr::Const {
                    dst: Reg(1),
                    value: LirConst::Float(2.5),
                },
                LirInstr::StoreLocal {
                    slot: 0,
                    src: Reg(1),
                },
            ],
            Terminator::Jump(Label(1)),
        )
        // Block 1: s = r0 (Int) — conflicts with the Float store in block 0
        .block(
            1,
            vec![LirInstr::StoreLocal {
                slot: 0,
                src: Reg(0),
            }],
            Terminator::Return(Reg(0)),
        )
        .build()
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
    LirFixture::new(Arity::Exact(1))
        .name("seq_reassign")
        .signal(Signal::errors())
        .num_locals(1)
        .block(
            0,
            vec![
                // Load param (unused, just for arity)
                LirInstr::LoadCaptureRaw {
                    dst: Reg(0),
                    index: 0,
                },
                // var s = 0 (Int)
                LirInstr::Const {
                    dst: Reg(1),
                    value: LirConst::Int(0),
                },
                LirInstr::StoreLocal {
                    slot: 0,
                    src: Reg(1),
                },
                // s = 1.5 (Float — same block, sequential reassignment)
                LirInstr::Const {
                    dst: Reg(2),
                    value: LirConst::Float(1.5),
                },
                LirInstr::StoreLocal {
                    slot: 0,
                    src: Reg(2),
                },
                // Load and return s
                LirInstr::LoadLocal {
                    dst: Reg(3),
                    slot: 0,
                },
            ],
            Terminator::Return(Reg(3)),
        )
        .build()
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
