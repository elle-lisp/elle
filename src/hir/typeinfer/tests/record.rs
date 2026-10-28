// audited: 2026-10-28
//! What a grafted call's record gives the definition it stood for, in every round of the fold.
//!
//! docs/impl/graft.md

use super::{compiled, count_emits};

/// A grafted call in an arm the fold deletes keeps the type it recorded. `each`
/// expands to one arm for each kind of sequence, and over an array the fold
/// deletes every arm but the array's. Every arm passed `g` a float, so the same
/// fold deletes the number guard of the `*` in `h`.
///
/// The counter-factual: the next round read each deleted arm's record as
/// unknown, because the `let` it names went with the arm, and the contract
/// rejected the `%mul` whose guard was already gone.
#[test]
fn a_call_in_an_arm_the_fold_deletes_keeps_its_record() {
    for src in [
        "(defn h [g] (* 0.7 g)) (each s in [1 2] (h (+ 0.5 0.5)))",
        "(defn h [g] (* 0.7 g)) (each s in @[1 2] (h (float 1)))",
        "(defn h [g] (* 0.7 g)) (def xs [1 2]) (each s in xs (h (+ 0.5 0.5)))",
        "(defn h [g] (* 0.7 g)) (defn k [] (each s in [1 2] (h (+ 0.5 0.5)))) (k)",
    ] {
        let (hir, _) = compiled(src);
        assert_eq!(count_emits(&hir), 0, "{src}: no guard of * is left");
    }
}
