// audited: 2026-11-23
//! A tail-call operand the call retains moves none of the frame's references, so what it names is released before the call.
//!
//! docs/impl/region/operand.md

use super::operand::pair_released_before_the_tail_call;
use super::*;

/// One arm of the operand is a literal, a constant the frame does not own, so
/// the call retains the operand's value whichever arm runs, and the callee's
/// release consumes that retain. The frame's own reference to `a` moves
/// nowhere, so its release runs before the `TailCall` on every path.
///
/// The counter-factual: the exemption read the union over the arms as if the
/// operand took the move, and left `a`'s release behind the `TailCall`, where
/// the closure path never runs it. Each call stranded `a` on every arm: on the
/// literal arm nothing released it, and on the `a` arm the callee consumed the
/// retain rather than the frame's reference.
#[test]
fn a_retained_branch_operand_releases_what_it_names_before_the_tail_call() {
    for operand in ["(if c [7 8] a)", "(cond (%lt x 0) a c [7 8] true a)"] {
        let module = compile_to_lir(&format!(
            "(begin (def sink (fn (h) (block :keep (length h)))) \
                    (def run (fn (x c) (let [a (%pair x nil)] (sink {operand})))) \
                    (run 1 true))"
        ));
        let (released, before) = pair_released_before_the_tail_call(&module);
        assert!(
            released,
            "a is not released before the TailCall that retains its operand; the block \
             up to the call: {before:#?}; operand {operand}",
        );
    }
}
