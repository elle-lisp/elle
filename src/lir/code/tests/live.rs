// audited: 2026-11-27
//! Which local slots `SlotLiveness` finds live: past a store, across blocks, around a loop, into an emit's resume, and at a celled local.
//!
//! src/lir/overview.md

use super::*;
use crate::lir::Label;
use crate::signals::Signal;

/// The empty set, as `slots` reads one.
const NONE: [u16; 0] = [];

fn slots(set: &LiveSlots) -> Vec<u16> {
    set.iter().collect()
}

fn store(slot: u16, src: u32) -> InstrRef<'static> {
    InstrRef::StoreLocal { slot, src: r(src) }
}

fn load(dst: u32, slot: u16) -> InstrRef<'static> {
    InstrRef::LoadLocal { dst: r(dst), slot }
}

fn constant(dst: u32) -> InstrRef<'static> {
    InstrRef::Const {
        dst: r(dst),
        value: ConstRef::Int(1),
    }
}

/// Two locals in one block: slot 0 stored then read, slot 1 stored and never
/// read.
#[test]
fn a_slot_is_live_from_its_store_to_its_last_read() {
    // Counter-factual: a liveness that kept every stored slot live would hold
    // slot 1, which nothing reads, at every point past its store.
    let func = LirFixture::new(Arity::Exact(0))
        .num_locals(2)
        .block(
            0,
            &[
                constant(0),
                store(0, 0),
                store(1, 0),
                constant(1),
                load(2, 0),
                constant(3),
            ],
            Terminator::Return(r(2)),
        )
        .build();
    let view = func.view();
    let block = SlotLiveness::of(&view).block(&view, 0);
    assert_eq!(
        slots(block.before(1)),
        NONE,
        "the store starts slot 0's life"
    );
    assert_eq!(slots(block.after(1)), vec![0]);
    assert_eq!(slots(block.after(2)), vec![0], "slot 1 is never read");
    assert_eq!(slots(block.after(3)), vec![0]);
    assert_eq!(slots(block.after(4)), NONE, "the read was slot 0's last");
}

/// A store ends the life of the value the slot held before it.
#[test]
fn a_store_ends_the_life_of_the_value_before_it() {
    let func = LirFixture::new(Arity::Exact(0))
        .num_locals(1)
        .block(
            0,
            &[
                constant(0),
                store(0, 0),
                constant(1),
                store(0, 1),
                load(2, 0),
            ],
            Terminator::Return(r(2)),
        )
        .build();
    let view = func.view();
    let block = SlotLiveness::of(&view).block(&view, 0);
    assert_eq!(slots(block.after(1)), NONE, "the second store replaces it");
    assert_eq!(slots(block.after(3)), vec![0]);
}

/// Slot 0 is stored in the first block and read in the third.
#[test]
fn a_slot_read_in_a_later_block_is_live_through_the_blocks_between() {
    let func = LirFixture::new(Arity::Exact(0))
        .num_locals(1)
        .block(0, &[constant(0), store(0, 0)], Terminator::Jump(Label(1)))
        .block(1, &[constant(1)], Terminator::Jump(Label(2)))
        .block(2, &[load(2, 0)], Terminator::Return(r(2)))
        .build();
    let view = func.view();
    let live = SlotLiveness::of(&view);
    assert_eq!(slots(live.live_in(Label(0))), NONE);
    assert_eq!(slots(live.live_in(Label(1))), vec![0]);
    assert_eq!(slots(live.live_in(Label(2))), vec![0]);
    assert_eq!(slots(live.block(&view, 0).after(1)), vec![0]);
}

/// A branch whose one arm reads slot 0 and whose other does not.
#[test]
fn a_branch_keeps_a_slot_that_either_arm_reads() {
    let func = LirFixture::new(Arity::Exact(0))
        .num_locals(1)
        .block(
            0,
            &[constant(0), store(0, 0)],
            Terminator::Branch {
                cond: r(0),
                then_label: Label(1),
                else_label: Label(2),
            },
        )
        .block(1, &[load(1, 0)], Terminator::Return(r(1)))
        .block(2, &[constant(2)], Terminator::Return(r(2)))
        .build();
    let view = func.view();
    let live = SlotLiveness::of(&view);
    assert_eq!(slots(live.block(&view, 0).after(1)), vec![0]);
    assert_eq!(slots(live.live_in(Label(1))), vec![0]);
    assert_eq!(slots(live.live_in(Label(2))), NONE);
}

/// fn(n): count slot 0 up to the parameter. The back edge from the body to
/// the test carries slot 0, which the test reads.
#[test]
fn a_loop_keeps_a_slot_live_around_its_back_edge() {
    // Counter-factual: a liveness that walked the blocks once, in reverse,
    // would meet the body before the test that reads slot 0 across the back
    // edge, and would find slot 0 dead after the body's store.
    let func = LirFixture::new(Arity::Exact(1))
        .num_locals(1)
        .block(
            0,
            &[
                InstrRef::LoadCapture {
                    dst: r(0),
                    index: 0,
                },
                constant(1),
                store(0, 1),
            ],
            Terminator::Jump(Label(1)),
        )
        .block(
            1,
            &[load(2, 0), InstrRef::compare(r(3), CmpOp::Lt, r(2), r(0))],
            Terminator::Branch {
                cond: r(3),
                then_label: Label(2),
                else_label: Label(3),
            },
        )
        .block(
            2,
            &[
                load(4, 0),
                constant(5),
                InstrRef::binop(r(6), BinOp::Add, r(4), r(5)),
                store(0, 6),
            ],
            Terminator::Jump(Label(1)),
        )
        .block(3, &[load(7, 0)], Terminator::Return(r(7)))
        .build();
    let view = func.view();
    let live = SlotLiveness::of(&view);
    assert_eq!(slots(live.live_in(Label(0))), NONE);
    assert_eq!(slots(live.live_in(Label(1))), vec![0]);
    assert_eq!(slots(live.live_in(Label(2))), vec![0]);
    assert_eq!(
        slots(live.block(&view, 2).after(3)),
        vec![0],
        "the store feeds the test across the back edge"
    );
}

/// An emit flows into its resume block, which reads slot 1 and not slot 0.
#[test]
fn an_emit_carries_what_its_resume_block_reads() {
    let func = LirFixture::new(Arity::Exact(0))
        .signal(Signal::yields())
        .num_locals(2)
        .block(
            0,
            &[constant(0), store(0, 0), store(1, 0)],
            Terminator::Emit {
                signal: crate::value::fiber::SIG_YIELD,
                value: r(0),
                resume_label: Label(1),
            },
        )
        .block(
            1,
            &[InstrRef::LoadResumeValue { dst: r(1) }, load(2, 1)],
            Terminator::Return(r(2)),
        )
        .build();
    let view = func.view();
    let live = SlotLiveness::of(&view);
    assert_eq!(slots(live.live_in(Label(1))), vec![1]);
    assert_eq!(slots(live.block(&view, 0).after(2)), vec![1]);
}

/// Slot 0 is the one local past no parameter, and a cell holds it. A store
/// to it counts as a read, so it ends no life and starts none.
#[test]
fn a_store_to_a_celled_local_reads_it() {
    // Counter-factual: a store that replaced a celled slot would leave the
    // slot dead before it, and the side exit there would drop the cell the
    // frame's closures share.
    let func = LirFixture::new(Arity::Exact(0))
        .num_locals(1)
        .head(|h| h.capture_locals_mask = crate::value::CaptureMask::from_u64(0b1))
        .block(0, &[constant(0), store(0, 0)], Terminator::Return(r(0)))
        .build();
    let view = func.view();
    let block = SlotLiveness::of(&view).block(&view, 0);
    assert_eq!(slots(block.before(1)), vec![0]);
    assert_eq!(slots(block.after(1)), NONE);
}
