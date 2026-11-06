// audited: 2026-11-05
// src/lir/AGENTS.md
//! What each edge of a `Branch` owes the operand stack: the fall-through edge
//! and the taken edge each trim on their own path, forward or back.

use super::*;

/// The shape an `and` lowers to, with an orphan on the stack when it starts.
/// The entry branches on `entry_cond`: its taken edge runs straight to the
/// merge `L2`, and its fall-through edge runs into `L1`, a nested diamond whose
/// arms jump through `L5` to the same merge. The merge jumps to the exit.
///
/// Slot 2 holds the sentinel the function returns. It is the topmost local, so
/// a pop that falls through the reserved region takes it first, and the exit's
/// `LoadLocal 2` then reads past the end of the stack.
fn and_merge_func(entry_cond: bool) -> LirOwned {
    let [new, old, park] = orphan_under(Reg(1), Reg(2), 0);
    let mut func = LirFixture::new(Arity::Exact(0))
        .num_locals(3)
        .block(
            0,
            &[
                konst(Reg(0), 42),
                store(2, Reg(0)),
                new,
                old,
                park,
                store(1, Reg(2)), // consume the old value
                InstrRef::Const {
                    dst: Reg(3),
                    value: ConstRef::Bool(entry_cond),
                },
            ],
            Terminator::Branch {
                cond: Reg(3),
                then_label: Label(1),
                else_label: Label(2),
            },
        )
        .block(
            1,
            &[load(Reg(4), 1)],
            Terminator::Branch {
                cond: Reg(4),
                then_label: Label(3),
                else_label: Label(4),
            },
        );
    for label in [3, 4] {
        func = func.block(label, &[], Terminator::Jump(Label(5)));
    }
    func.block(5, &[], Terminator::Jump(Label(2)))
        .block(2, &[], Terminator::Jump(Label(6)))
        .block(6, &[load(Reg(5), 2)], Terminator::Return(Reg(5)))
        .build()
}

#[test]
fn a_branch_edge_into_a_merge_drops_its_orphan() {
    // Counter-factual: with the branch leaving its orphan in place, the taken
    // edge fixes the merge one cell deep, while the nested diamond's jumps pop
    // the orphan on the fall-through path. The merge's jump to the exit then
    // pops it again on that path, the sentinel's slot goes, and the run does
    // not return 42. The taken path pops the orphan once either way, so it
    // checks the pad the taken edge lands on rather than the defect.
    for entry_cond in [true, false] {
        let value = run_int(&and_merge_func(entry_cond));
        assert_eq!(
            value,
            Some(42),
            "entry_cond {entry_cond}: every path into the merge must arrive at \
             the depth it was fixed at, or the frame loses its topmost local"
        );
    }
}

/// A loop whose back edge is a `Branch`: the body runs, then branches back to
/// itself while the counter is below `iterations`. The body leaves one orphan
/// under a live value, so the edge reaches the header two cells deeper than the
/// header was fixed at.
///
/// Slot 0 is the counter. The sentinel the function returns stays on the
/// operand stack under the loop, so the exit's `Return` reads it at the offset
/// the simulation names.
fn branch_back_edge_func(iterations: i64) -> LirOwned {
    let [new, old, park] = orphan_under(Reg(2), Reg(3), 1);
    LirFixture::new(Arity::Exact(0))
        .num_locals(2)
        .block(
            0,
            &[konst(Reg(0), 0), store(0, Reg(0)), konst(Reg(1), 42)],
            Terminator::Jump(Label(1)),
        )
        .block(
            1,
            &[
                new,
                old,
                park,
                load(Reg(4), 0), // the counter, above the pair
                konst(Reg(5), 1),
                InstrRef::binop(Reg(6), BinOp::Add, Reg(4), Reg(5)),
                store(0, Reg(6)),
                load(Reg(7), 0),
                konst(Reg(8), iterations),
                InstrRef::compare(Reg(9), CmpOp::Lt, Reg(7), Reg(8)),
            ],
            Terminator::Branch {
                cond: Reg(9),
                then_label: Label(1),
                else_label: Label(2),
            },
        )
        .block(2, &[], Terminator::Return(Reg(1)))
        .build()
}

#[test]
fn a_branch_back_edge_leaves_the_operand_depth_its_target_was_fixed_at() {
    // Counter-factual: with the branch's back edge trimming nothing, each pass
    // leaves its orphan and its old value on the stack. The exit's `DupN`
    // counts from the top, so it reaches into the last pass's residue and
    // returns a 9 in place of the sentinel.
    assert_eq!(
        run_int(&branch_back_edge_func(3)),
        Some(42),
        "a loop whose back edge is a branch must reach its exit at the depth \
         its header was fixed at",
    );
}
