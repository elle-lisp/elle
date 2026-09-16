// audited: 2026-09-16
// src/lir/AGENTS.md
//! What a loop leaves on the activation's operand stack.
//!
//! The emitter simulates the operand stack per block and reaches a back edge at
//! whatever depth the block's instructions left. Rule 11 (src/lir/AGENTS.md
//! § "Merge operand depth") says that depth must be the one the loop header was
//! fixed at; a body that arrives deeper makes the header run one notch lower
//! every pass, and the activation's stack grows for as long as the loop does.
//!
//! No region gauge sees that growth: the cells live in the `Fiber`'s own
//! `SmallVec`, not in a region, so `arena/count` and `arena/region-count` stay
//! flat while RSS climbs. These pins read the depth directly instead.
//!
//! The emitter's own pin is `lir::emit::tests`; these run real Elle source
//! through the full stdlib, because which shapes need a `DupN` is decided by
//! the lowerer and the stdlib both.

use super::*;
use crate::pipeline::compile_file_repl;

/// Run `src` on the root fiber and answer the operand depth it left behind.
///
/// `VM::execute` runs a top-level body on `vm.fiber.stack` without saving or
/// restoring it, so what is left there afterwards is exactly what the program's
/// own activation left — the frame's locals, plus any residue.
fn operand_depth_after(src: &str) -> usize {
    let mut rt = Runtime::new();
    let result = {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file_repl(src, symbols, cctx, "<embed>")
            .expect("compiles")
            .0
    };
    let (vm, _symbols, _cctx) = rt.parts();
    vm.execute(&result.bytecode).expect("runs");
    vm.fiber.stack.len()
}

/// A `while` over a mutable binding a closure also captures — the shape every
/// `(protect …)`, `(try …)` and `(fn [] …)` inside a counted loop writes.
///
/// The capture cells the binding needs put the loop's own `assign` through
/// `StoreCaptureCell`, whose two operands the emitter reaches with a `DupN`;
/// the copy's original then sits under a live cell, where the orphan trim
/// cannot reach it.
fn captured_counter_loop(trips: i64) -> String {
    format!(
        "(var i 0) \
         (def keeper (fn [] i)) \
         (while (< i {trips}) (assign i (+ i 1))) \
         i"
    )
}

#[test]
fn a_captured_counter_loop_leaves_one_operand_depth_whatever_its_trip_count() {
    // Counter-factual: with the back edge trimming only trailing orphans, the
    // 400-trip run ends hundreds of cells above the 8-trip one and the two
    // depths differ by a multiple of the trip-count gap. Nothing else fails —
    // the loop still counts to `trips` and the locals still sit below the
    // residue — which is why this asserts the depth rather than the answer.
    let shallow = operand_depth_after(&captured_counter_loop(8));
    let deep = operand_depth_after(&captured_counter_loop(400));
    assert_eq!(
        deep, shallow,
        "a loop whose body pushes nothing per iteration must leave the operand \
         stack at one depth whatever its trip count",
    );
}

#[test]
fn a_protect_in_a_captured_counter_loop_leaves_one_operand_depth() {
    // `protect` runs its body in a child fiber, so the loop body's closure
    // captures the counter and the whole park/resume path runs per trip. The
    // residue is the caller activation's, and a park carries it: the frame's
    // saved stack is the one that grows.
    let src = |trips: i64| {
        format!(
            "(var i 0) \
             (while (< i {trips}) (protect (error i)) (assign i (+ i 1))) \
             i"
        )
    };
    let shallow = operand_depth_after(&src(8));
    let deep = operand_depth_after(&src(400));
    assert_eq!(
        deep, shallow,
        "a `protect` whose body captures the loop counter must leave the \
         operand stack at one depth whatever its trip count",
    );
}
