// audited: 2026-11-05
// src/lir/AGENTS.md
//! The operand depth an edge leaves: what a jump into a merge owes, and what a
//! jump back into a loop header owes.
//!
//! A block inherits its stack simulation from a predecessor, so the first
//! predecessor fixes the block's operand depth and every later edge must arrive
//! at the same depth (src/lir/AGENTS.md). Every edge trims, and the two
//! directions need different trims: a forward edge may drop trailing orphans
//! alone, while a back edge drops every surplus cell. `branch` holds what each
//! edge of a `Branch` owes.

use super::*;

mod branch;

/// Load the integer `n` into `dst`.
fn konst(dst: Reg, n: i64) -> InstrRef<'static> {
    InstrRef::Const {
        dst,
        value: ConstRef::Int(n),
    }
}

/// Store `src` into local `slot`.
fn store(slot: u16, src: Reg) -> InstrRef<'static> {
    InstrRef::StoreLocal { slot, src }
}

/// Load local `slot` into `dst`.
fn load(dst: Reg, slot: u16) -> InstrRef<'static> {
    InstrRef::LoadLocal { dst, slot }
}

/// Leave one orphan on the operand stack, under the "old" value `old`, the way
/// the lowerer does at a reassignment (`lower_assign`'s drop-on-overwrite arm):
/// push the new value, push the old value, then store the new value into
/// `slot`. `ensure_on_top` must `DupN` the new value back to the top past the
/// old one, so its original cell is left behind, and no register names it.
fn orphan_under(new: Reg, old: Reg, slot: u16) -> [InstrRef<'static>; 3] {
    [konst(new, 7), konst(old, 9), store(slot, new)]
}

/// The integer `func` returns when it runs, or `None` when it fails.
fn run_int(func: &LirOwned) -> Option<i64> {
    let mut vm = crate::vm::VM::new();
    run_on(&mut vm, func).ok().and_then(|v| v.as_int())
}

/// A two-diamond function that pins the forward rule. The entry block leaves
/// ONE orphan on the operand stack, then branches; `L1` is the diamond's other
/// arm and jumps to the merge. The merge branches again, and both of its arms
/// jump to the exit.
///
/// Slot 2 holds the sentinel the function returns. It is the topmost local, so
/// it is the first casualty of a pop that falls through the reserved region:
/// the second (spurious) orphan pop shortens the stack to `num_locals - 1`, and
/// the exit block's `LoadLocal 2` then indexes past its end.
fn orphan_across_merge_func() -> LirOwned {
    let [new, old, park] = orphan_under(Reg(1), Reg(2), 0);
    let mut func = LirFixture::new(Arity::Exact(0))
        .num_locals(3)
        // Entry: park the sentinel in slot 2, then manufacture the orphan.
        .block(
            0,
            &[
                konst(Reg(0), 42),
                store(2, Reg(0)),
                new,
                old,
                park,
                store(1, Reg(2)), // consume the old value
                load(Reg(3), 0),  // stack is now [Reg(1)] — one orphan, on top
            ],
            Terminator::Branch {
                cond: Reg(3),
                then_label: Label(1),
                else_label: Label(2),
            },
        )
        // The diamond's other arm: nothing but the jump to the merge.
        .block(1, &[], Terminator::Jump(Label(2)))
        // The merge, which branches again into a second diamond.
        .block(
            2,
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

    // Exit: read the sentinel back out of the topmost local and return it.
    func.block(5, &[load(Reg(5), 2)], Terminator::Return(Reg(5)))
        .build()
}

#[test]
fn merge_predecessors_leave_equal_operand_depth() {
    // Locals 0 and 1 hold 7 and 9, so both branches take their `then` arm and
    // the run passes through two jump edges. Each edge may drop the orphan at
    // most once between them; a second drop shortens the stack into the
    // reserved local region and the sentinel in slot 2 stops existing.
    assert_eq!(
        run_int(&orphan_across_merge_func()),
        Some(42),
        "a jump edge into an already-fixed merge must not pop below the depth \
         the branch edge left, or the frame loses its topmost local"
    );
}

/// A counted loop whose body leaves one orphan UNDER a live value, so the
/// orphan trim cannot reach it: `pop_trailing_orphans_to` stops at the first
/// live cell.
///
/// The body builds the orphan with `orphan_under` and leaves the old value
/// standing instead of consuming it. The counter's own load, add and store run
/// above the pair and balance, so the block reaches its back edge exactly two
/// cells deeper than the header was fixed at.
///
/// Slot 0 is the counter. The sentinel the function returns stays on the
/// operand stack under the loop, so the exit's `Return` takes whatever cell
/// the simulation says is on top.
fn orphan_in_loop_func(iterations: i64) -> LirOwned {
    let [new, old, park] = orphan_under(Reg(5), Reg(6), 1);
    LirFixture::new(Arity::Exact(0))
        .num_locals(2)
        .block(
            0,
            &[konst(Reg(0), 0), store(0, Reg(0)), konst(Reg(1), 42)],
            Terminator::Jump(Label(1)),
        )
        // The header: keep going while the counter is below `iterations`.
        .block(
            1,
            &[
                load(Reg(2), 0),
                konst(Reg(3), iterations),
                InstrRef::compare(Reg(4), CmpOp::Lt, Reg(2), Reg(3)),
            ],
            Terminator::Branch {
                cond: Reg(4),
                then_label: Label(2),
                else_label: Label(3),
            },
        )
        // The body, and the back edge into the header.
        .block(
            2,
            &[
                new,
                old,
                park,
                load(Reg(7), 0), // the counter, above the pair
                konst(Reg(8), 1),
                InstrRef::binop(Reg(9), BinOp::Add, Reg(7), Reg(8)),
                store(0, Reg(9)),
            ],
            Terminator::Jump(Label(1)),
        )
        .block(3, &[], Terminator::Return(Reg(1)))
        .build()
}

#[test]
fn a_back_edge_leaves_the_operand_depth_its_target_was_fixed_at() {
    // Counter-factual: with the back edge trimming only trailing orphans, the
    // live value the body leaves standing blocks the trim, and each pass leaves
    // two cells on the stack, for as long as the loop runs. The exit then
    // returns the top cell, the 9 of the last pass, in place of the sentinel.
    // The VM empties the stack when the frame returns, so the depth after the
    // run is the same either way, and only the answer shows the leak.
    assert_eq!(
        run_int(&orphan_in_loop_func(3)),
        Some(42),
        "a loop that pushes nothing per iteration must reach its exit at the \
         depth its header was fixed at",
    );
}
