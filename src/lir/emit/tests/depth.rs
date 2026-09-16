// audited: 2026-09-16
// src/lir/AGENTS.md
//! The operand depth an edge leaves: what a jump into a merge owes, and what a
//! jump back into a loop header owes.
//!
//! A block inherits its stack simulation from the first predecessor that
//! reaches it, so that predecessor fixes the block's operand depth and every
//! later edge must arrive at the same depth (src/lir/AGENTS.md § "Merge operand
//! depth"). Only `Terminator::Jump` trims, and the two directions need
//! different trims: a forward edge may drop trailing orphans alone, while a
//! back edge drops every surplus cell.

use super::*;

/// A two-diamond function that pins the forward rule. The entry block leaves
/// ONE orphan on the operand stack, then branches; `L1` is the diamond's other
/// arm and jumps to the merge. The merge branches again, and both of its arms
/// jump to the exit.
///
/// Slot 2 holds the sentinel the function returns. It is the topmost local, so
/// it is the first casualty of a pop that falls through the reserved region:
/// the second (spurious) orphan pop shortens the stack to `num_locals - 1`, and
/// the exit block's `LoadLocal 2` then indexes past its end.
///
/// The orphan is built the way the lowerer builds one at a reassignment
/// (`lower_assign`'s drop-on-overwrite arm): push the new value, push the old
/// value, store the new value — `ensure_on_top` must `DupN` it back to the top
/// past the old one — then consume the old value. What remains is the new
/// value's original cell, which no register names any more.
fn orphan_across_merge_func() -> LirFunction {
    let konst = |dst: Reg, n: i64| LirInstr::Const {
        dst,
        value: LirConst::Int(n),
    };
    let store = |slot: u16, src: Reg| LirInstr::StoreLocal { slot, src };
    let load = |dst: Reg, slot: u16| LirInstr::LoadLocal { dst, slot };

    let mut func = LirFixture::new(Arity::Exact(0))
        .num_locals(3)
        // Entry: park the sentinel in slot 2, then manufacture the orphan.
        .block(
            0,
            vec![
                konst(Reg(0), 42),
                store(2, Reg(0)),
                konst(Reg(1), 7), // the "new" value
                konst(Reg(2), 9), // the "old" value, pushed above it
                store(0, Reg(1)), // DupN past the old value, then store
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
        .block(1, vec![], Terminator::Jump(Label(2)))
        // The merge, which branches again into a second diamond.
        .block(
            2,
            vec![load(Reg(4), 1)],
            Terminator::Branch {
                cond: Reg(4),
                then_label: Label(3),
                else_label: Label(4),
            },
        );

    for label in [3, 4] {
        func = func.block(label, vec![], Terminator::Jump(Label(5)));
    }

    // Exit: read the sentinel back out of the topmost local and return it.
    func.block(5, vec![load(Reg(5), 2)], Terminator::Return(Reg(5)))
        .build()
}

#[test]
fn merge_predecessors_leave_equal_operand_depth() {
    // Locals 0 and 1 hold 7 and 9, so both branches take their `then` arm and
    // the run passes through two jump edges. Each edge may drop the orphan at
    // most once between them; a second drop shortens the stack into the
    // reserved local region and the sentinel in slot 2 stops existing.
    let func = orphan_across_merge_func();
    let mut emitter = Emitter::new();
    let (bytecode, _, _) = emitter.emit(&func);
    let mut vm = crate::vm::VM::new();
    let result = vm.execute(&bytecode);
    assert_eq!(
        result.ok().and_then(|v| v.as_int()),
        Some(42),
        "a jump edge into an already-fixed merge must not pop below the depth \
         the branch edge left, or the frame loses its topmost local"
    );
}

/// A counted loop whose body leaves one orphan UNDER a live value, so the
/// orphan trim cannot reach it: `pop_trailing_orphans_to` stops at the first
/// live cell.
///
/// The body builds the orphan the way `orphan_across_merge_func` does and then
/// leaves the old value standing instead of consuming it. The counter's own
/// load, add and store run above the pair and balance, so the block reaches its
/// back edge exactly two cells deeper than the header was fixed at.
///
/// Slot 0 is the counter, slot 3 the sentinel the function returns.
fn orphan_in_loop_func(iterations: i64) -> LirFunction {
    let konst = |dst: Reg, n: i64| LirInstr::Const {
        dst,
        value: LirConst::Int(n),
    };
    let store = |slot: u16, src: Reg| LirInstr::StoreLocal { slot, src };
    let load = |dst: Reg, slot: u16| LirInstr::LoadLocal { dst, slot };

    LirFixture::new(Arity::Exact(0))
        .num_locals(4)
        .block(
            0,
            vec![
                konst(Reg(0), 0),
                store(0, Reg(0)),
                konst(Reg(1), 42),
                store(3, Reg(1)),
            ],
            Terminator::Jump(Label(1)),
        )
        // The header: keep going while the counter is below `iterations`.
        .block(
            1,
            vec![
                load(Reg(2), 0),
                konst(Reg(3), iterations),
                LirInstr::compare(Reg(4), CmpOp::Lt, Reg(2), Reg(3)),
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
            vec![
                konst(Reg(5), 7), // the "new" value
                konst(Reg(6), 9), // the "old" value, pushed above it
                store(1, Reg(5)), // DupN past the old value, then store
                load(Reg(7), 0),  // the counter, above the pair
                konst(Reg(8), 1),
                LirInstr::binop(Reg(9), BinOp::Add, Reg(7), Reg(8)),
                store(0, Reg(9)),
            ],
            Terminator::Jump(Label(1)),
        )
        .block(3, vec![load(Reg(10), 3)], Terminator::Return(Reg(10)))
        .build()
}

/// The operand depth the fiber is left at after running `orphan_in_loop_func`
/// for `iterations` passes, and the sentinel it returned.
fn run_orphan_loop(iterations: i64) -> (usize, Option<i64>) {
    let func = orphan_in_loop_func(iterations);
    let (bytecode, _, _) = Emitter::new().emit(&func);
    let mut vm = crate::vm::VM::new();
    let result = vm.execute(&bytecode);
    (vm.fiber.stack.len(), result.ok().and_then(|v| v.as_int()))
}

#[test]
fn a_back_edge_leaves_the_operand_depth_its_target_was_fixed_at() {
    // The two runs differ only in trip count, so a depth that differs with it
    // is the loop growing the operand stack — one leak per iteration, for as
    // long as the loop runs (src/lir/AGENTS.md § "Merge operand depth").
    //
    // Counter-factual: with the back edge trimming only trailing orphans, the
    // live value the body leaves standing blocks the trim, and the deeper run
    // ends 2 × (200 - 8) cells above the shallower one. Nothing else fails —
    // the locals sit below the residue, so the sentinel comes back either way,
    // which is why this asserts the depth rather than the answer.
    let (shallow_depth, shallow_value) = run_orphan_loop(8);
    let (deep_depth, deep_value) = run_orphan_loop(200);

    assert_eq!(shallow_value, Some(42), "the loop must still compute");
    assert_eq!(deep_value, Some(42), "the loop must still compute");
    assert_eq!(
        deep_depth, shallow_depth,
        "a loop that pushes nothing per iteration must leave the operand \
         stack at one depth whatever its trip count",
    );
}
