// audited: 2026-10-21
//! The containment facet: which stores hand a value to a holder the frame cannot count.
//!
//! docs/impl/escape.md

use super::*;

/// A `%pair` holds each operand by the counted edge its allocation takes, so the
/// operand is contained only where the pair is. `step` hands its pair to its own
/// tail call and returns `acc` from the base arm, so nothing uncounted ever holds
/// `acc`, though it is still stored.
///
/// The counter-factual: every `%pair` operand seeded containment, so `acc` was
/// never frame-held. The branch-arm window left its release in the base arm, and
/// the recursive arm stranded each accumulator it displaced.
#[test]
fn a_pair_operand_is_not_contained_by_a_pair_that_only_returns() {
    let src = "(def step (fn (i n acc) (if (%lt i n) (step (%add i 1) n (%pair i acc)) acc))) \
               (step 0 4 ())";
    let (hir, arena, escape) = escape_of(src);
    let accs = bindings_named(&hir, &arena, &["acc"]);
    assert!(!accs.is_empty(), "missing `acc` in `{src}`");
    for b in accs {
        assert!(
            escape.binding_escapes_activation(b),
            "`acc` is stored into the pair, so it still escapes its activation"
        );
        assert!(
            !escape.binding_escapes_by_containment(b),
            "`acc` is held only by a pair that returns, which counts its hold"
        );
    }
}

/// The pair's own containment carries its operands with it, as an escaping
/// closure carries its captures. `git` is `Mixed`, an uncounted store of every
/// argument, so `x` inside the pair it receives is contained too.
#[test]
fn a_pair_operand_is_contained_by_a_pair_an_uncounted_store_holds() {
    let src = "(def f (fn (x) (let [p (%pair x x)] (git p) nil))) (f \"s\")";
    let (hir, arena, escape) = escape_of(src);
    let xs = bindings_named(&hir, &arena, &["x"]);
    assert!(!xs.is_empty(), "missing `x` in `{src}`");
    for b in xs {
        assert!(
            escape.binding_escapes_by_containment(b),
            "`x` is reachable through a pair an uncounted store holds"
        );
    }
}
