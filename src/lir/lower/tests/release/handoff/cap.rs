// audited: 2026-11-18
//! The argument position a call's offers stop at: no move past it, and a borrowed tail argument there keeps its mint ahead of the call.
//!
//! docs/impl/region/handoff.md
//!
//! A compiled call hears back what its callee took as one 64-bit word of
//! positions, so no offer names position 64 or past it. Each test pairs a call
//! that is one argument too wide with the widest call the lowerer still offers
//! in full, which shows the shape is one it offers.

use super::latemint::the_borrowing_tail_call;
use super::*;

/// A source whose `w` calls `q` with `n` arguments, each a read ANF names in
/// its own position, so each is offered where its position allows. The callee
/// collects its arguments, because a function takes at most 64 parameters.
fn wide_call(n: usize) -> String {
    let args = (0..n).map(|i| format!("(xs {i})")).collect::<Vec<_>>();
    let items = (0..n).map(|i| i.to_string()).collect::<Vec<_>>();
    format!(
        "(begin (def p (fn (& more) nil)) \
         (def w (fn (q xs) (begin (q {}) 0))) \
         (w p [{}]))",
        args.join(" "),
        items.join(" "),
    )
}

/// A source whose one borrowed tail argument, the capture `s`, is the last of
/// `n` arguments.
fn wide_tail_call(n: usize) -> String {
    let zeros = vec!["0"; n - 1].join(" ");
    format!(
        "(begin (def mk (fn (s) (fn (g) (g {zeros} s)))) \
         ((mk (string \"x\")) (fn (& more) nil)))",
    )
}

/// The positions `w` in `source` offers to move.
fn offered_positions(source: &str) -> Vec<usize> {
    offers_in(named(&compile_to_lir(source), "w"))
        .into_iter()
        .map(|(position, _)| position)
        .collect()
}

#[test]
fn no_call_offers_a_position_past_63() {
    let full: Vec<usize> = (0..64).collect();
    assert_eq!(
        offered_positions(&wide_call(64)),
        full,
        "a 64-argument call does not offer every argument",
    );
    assert_eq!(
        offered_positions(&wide_call(65)),
        full,
        "a 65-argument call offers past position 63",
    );
}

#[test]
fn a_borrowed_tail_argument_past_63_is_minted_ahead_of_the_call() {
    let module = compile_to_lir(&wide_tail_call(64));
    let (_, _, stash, late) = the_borrowing_tail_call(&module);
    assert_eq!(
        late,
        vec![(63, stash[0])],
        "the capture at position 63 is not left to the runtime's mint",
    );

    let module = compile_to_lir(&wide_tail_call(65));
    let (b, at, stash, late) = the_borrowing_tail_call(&module);
    assert_eq!(stash.len(), 1, "the capture is borrowed (stash={stash:?})");
    assert!(
        late.is_empty(),
        "the capture at position 64 was left to the runtime's mint (late={late:?})",
    );
    let retains = positions(&b, |i| matches!(i, InstrRef::IncrefValueRegion { .. }));
    assert!(
        retains.iter().any(|&r| r < at),
        "the capture at position 64 is not retained ahead of the TailCall \
         (at={at}, retains={retains:?})",
    );
}
