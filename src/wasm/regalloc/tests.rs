//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::lir::testkit::LirFixture;
use crate::lir::{LirInstr, Terminator};
use crate::value::Arity;

#[test]
fn within_block_reuse() {
    // Two registers used only within the same block should share a slot.
    // r0 = const 1; r1 = const 2; r2 = add r0 r1; return r2
    use crate::lir::{BinOp as LirBinOp, LirConst};
    let func = LirFixture::new(Arity::Exact(0))
        .block(
            0,
            vec![
                LirInstr::Const {
                    dst: Reg(0),
                    value: LirConst::Int(1),
                },
                LirInstr::Const {
                    dst: Reg(1),
                    value: LirConst::Int(2),
                },
                LirInstr::BinOp {
                    dst: Reg(2),
                    op: LirBinOp::Add,
                    lhs: Reg(0),
                    rhs: Reg(1),
                },
            ],
            Terminator::Return(Reg(2)),
        )
        .build();
    let alloc = allocate(&func, 0);

    // r0 and r1 are last used at idx 2 (BinOp), r2 is defined at idx 2.
    // Defs are allocated before frees at the same instruction, so r2
    // gets its own slot before r0/r1 are freed. Total: 3 slots.
    assert_eq!(alloc.max_slots, 3);
}

#[test]
fn cross_block_dedicated() {
    // r0 defined in block 0, used in block 1 → cross-block, gets dedicated slot.
    use crate::lir::LirConst;
    let func = LirFixture::new(Arity::Exact(0))
        .block(
            0,
            vec![LirInstr::Const {
                dst: Reg(0),
                value: LirConst::Int(42),
            }],
            Terminator::Jump(Label(1)),
        )
        .block(1, vec![], Terminator::Return(Reg(0)))
        .build();
    let alloc = allocate(&func, 0);

    assert_eq!(alloc.max_slots, 1);
    assert!(alloc.reg_to_slot.contains_key(&Reg(0)));
}
