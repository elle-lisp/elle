// audited: 2026-10-06
//! The case order, the dispatch values and the branches a `Layout` answers.
//!
//! docs/impl/wasm.md

use super::*;

/// Three blocks; the first two each end at a park site.
fn three_blocks_two_sites() -> Layout {
    Layout::new(3, [0, 1])
}

#[test]
fn a_resume_entry_follows_the_block_its_site_ends() {
    assert_eq!(
        three_blocks_two_sites().cases(),
        &[
            Case::Block(0),
            Case::Resume(0),
            Case::Block(1),
            Case::Resume(1),
            Case::Block(2),
        ],
    );
}

#[test]
fn the_dispatch_value_is_the_position_the_br_table_reaches() {
    // `br_table` value `v` branches to depth `v` from inside every block, and
    // depth `v` is the `end` in front of the case at position `v`. The
    // counter-factual is a dispatch counted from the other end, which resumes
    // a frame in the wrong case.
    let layout = three_blocks_two_sites();
    assert_eq!(layout.table(), vec![0, 1, 2, 3, 4]);
    assert_eq!(layout.dispatch(Case::Block(0)), 0);
    assert_eq!(layout.dispatch(Case::Resume(0)), 1);
    assert_eq!(layout.dispatch(Case::Block(2)), 4);
}

#[test]
fn a_block_branches_over_the_resume_entry_after_it() {
    // Case 0 sits inside the blocks whose `end`s are 1 and 2, so `br 1` lands
    // on case 2. A depth of 0 would land on the resume entry and deliver a
    // restart value nobody sent.
    let layout = three_blocks_two_sites();
    assert_eq!(layout.goto(0, Case::Block(1), 0), Goto::Forward(1));
}

#[test]
fn a_resume_entry_falls_into_the_block_after_it() {
    let layout = three_blocks_two_sites();
    assert_eq!(layout.goto(1, Case::Block(1), 0), Goto::Forward(0));
}

#[test]
fn a_branch_inside_a_structured_block_counts_it() {
    // The arm of a conditional branch sits inside its `if`.
    let layout = three_blocks_two_sites();
    assert_eq!(layout.goto(0, Case::Block(2), 1), Goto::Forward(4));
}

#[test]
fn a_branch_back_goes_round_the_loop() {
    // The last case sits inside the loop alone, so the loop is depth 0 there.
    // The first case sits inside the four blocks whose `end`s follow it.
    let layout = three_blocks_two_sites();
    assert_eq!(
        layout.goto(4, Case::Block(0), 0),
        Goto::Loop {
            dispatch: 0,
            depth: 0
        },
    );
    assert_eq!(
        layout.goto(0, Case::Block(0), 1),
        Goto::Loop {
            dispatch: 0,
            depth: 5
        },
        "a block that branches to itself goes round the loop",
    );
}

#[test]
fn a_block_with_no_site_has_no_resume_entry() {
    let layout = Layout::new(2, []);
    assert_eq!(layout.cases(), &[Case::Block(0), Case::Block(1)]);
    assert_eq!(layout.goto(0, Case::Block(1), 0), Goto::Forward(0));
}
