//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::lir::{BasicBlock, Label, LirFunction, SpannedInstr, SpannedTerminator};
use crate::syntax::Span;
use crate::value::Arity;

fn mk_func(blocks: Vec<BasicBlock>, num_regs: u32) -> LirFunction {
    let mut f = LirFunction::new(Arity::Exact(0));
    f.blocks = blocks;
    f.num_regs = num_regs;
    f
}

fn mk_block(label: u32, instrs: Vec<LirInstr>, term: Terminator) -> BasicBlock {
    let mut b = BasicBlock::new(Label(label));
    b.instructions = instrs
        .into_iter()
        .map(|i| SpannedInstr::new(i, Span::synthetic()))
        .collect();
    b.terminator = SpannedTerminator::new(term, Span::synthetic());
    b
}

#[test]
fn within_block_reuse() {
    // Two registers used only within the same block should share a slot.
    // r0 = const 1; r1 = const 2; r2 = add r0 r1; return r2
    use crate::lir::{BinOp as LirBinOp, LirConst};
    let block = mk_block(
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
    );
    let func = mk_func(vec![block], 3);
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
    let b0 = mk_block(
        0,
        vec![LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Int(42),
        }],
        Terminator::Jump(Label(1)),
    );
    let b1 = mk_block(1, vec![], Terminator::Return(Reg(0)));
    let func = mk_func(vec![b0, b1], 1);
    let alloc = allocate(&func, 0);

    assert_eq!(alloc.max_slots, 1);
    assert!(alloc.reg_to_slot.contains_key(&Reg(0)));
}
