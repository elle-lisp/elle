// audited: 2026-11-05
// src/lir/AGENTS.md
//! Which registers each block reads before it writes them, on some path from its entry.

use super::*;
use crate::lir::emit::live::live_in;

fn konst(dst: u32, n: i64) -> InstrRef<'static> {
    InstrRef::Const {
        dst: Reg(dst),
        value: ConstRef::Int(n),
    }
}

/// A counted loop. The entry defines the limit `Reg(0)` and the answer
/// `Reg(1)`; the header compares the counter in local 0 with the limit, the
/// body bumps the counter and reads neither register, and the exit returns the
/// answer.
fn loop_func() -> LirOwned {
    LirFixture::new(Arity::Exact(0))
        .num_locals(1)
        .block(
            0,
            &[
                konst(0, 10),
                konst(1, 42),
                konst(2, 0),
                InstrRef::StoreLocal {
                    slot: 0,
                    src: Reg(2),
                },
            ],
            Terminator::Jump(Label(1)),
        )
        .block(
            1,
            &[
                InstrRef::LoadLocal {
                    dst: Reg(3),
                    slot: 0,
                },
                InstrRef::compare(Reg(4), CmpOp::Lt, Reg(3), Reg(0)),
            ],
            Terminator::Branch {
                cond: Reg(4),
                then_label: Label(2),
                else_label: Label(3),
            },
        )
        .block(
            2,
            &[
                InstrRef::LoadLocal {
                    dst: Reg(5),
                    slot: 0,
                },
                konst(6, 1),
                InstrRef::binop(Reg(7), BinOp::Add, Reg(5), Reg(6)),
                InstrRef::StoreLocal {
                    slot: 0,
                    src: Reg(7),
                },
            ],
            Terminator::Jump(Label(1)),
        )
        .block(3, &[], Terminator::Return(Reg(1)))
        .build()
}

/// The registers of `regs` live into `label`.
fn live_of(func: &LirOwned, label: u32, regs: &[u32]) -> Vec<u32> {
    let live = live_in(&func.view());
    let set = live.get(&Label(label)).expect("every block has a live set");
    regs.iter()
        .copied()
        .filter(|&r| set.contains(Reg(r)))
        .collect()
}

#[test]
fn a_register_a_block_writes_before_it_reads_is_not_live_into_it() {
    // Counter-factual: a pass that ignored a block's writes would carry the
    // header's read of the limit and the exit's read of the answer back into
    // the entry, which defines both.
    let func = loop_func();
    assert_eq!(live_of(&func, 0, &[0, 1, 2]), Vec::<u32>::new());
    assert_eq!(live_of(&func, 2, &[5, 6, 7]), Vec::<u32>::new());
}

#[test]
fn a_returned_register_is_live_into_the_exit_and_every_block_before_it() {
    // Counter-factual: a pass that skipped the terminator finds the answer live
    // nowhere, and an edge into the exit would pop it as dead.
    let func = loop_func();
    for label in [1, 2, 3] {
        assert_eq!(live_of(&func, label, &[1]), vec![1], "into block {label}");
    }
}

#[test]
fn a_register_the_header_reads_is_live_around_the_back_edge() {
    // The body never reads the limit, yet the header it jumps back to does, so
    // the limit is live into the body. The exit never reads it.
    //
    // Counter-factual: a pass that walked each block alone, or that stopped
    // before the back edge's read reached the body, finds the limit dead in the
    // body, and the body's edges could pop the one cell that holds it.
    let func = loop_func();
    assert_eq!(live_of(&func, 1, &[0]), vec![0], "into the header");
    assert_eq!(live_of(&func, 2, &[0]), vec![0], "into the body");
    assert_eq!(live_of(&func, 3, &[0]), Vec::<u32>::new(), "into the exit");
}
