// audited: 2026-11-13
//! Which arguments a call offers to move into its callee, and which borrowed tail arguments wait for the runtime's mint.
//!
//! docs/impl/region/handoff.md
//!
//! - this file: the shapes the lowerer offers, and the operands that refuse it.
//! - `refusal` — each release a move would reorder or double, edited into the
//!   analysis of a shape that is offered.
//! - `latemint` — the borrowed tail arguments the runtime mints at the
//!   `TailCall`.

use super::*;

mod latemint;
mod refusal;

/// A call's offers, as `(position, slot)`.
type OfferList = Vec<(u8, u16)>;

/// The function in `module` whose name is `name`.
fn named<'m>(module: &'m FrozenModule, name: &str) -> &'m LirOwned {
    functions(module)
        .find(|f| f.view().name() == Some(name))
        .unwrap_or_else(|| panic!("a function named {name}"))
}

/// Every non-tail call in `func`, in emission order, as the block it sits in,
/// its index there, and its offers as `(position, slot)`.
fn non_tail_calls(func: &LirOwned) -> Vec<(BlockRef<'_>, usize, OfferList)> {
    let mut out = Vec::new();
    for b in func.view().blocks() {
        for (at, i) in b.instrs().enumerate() {
            match i {
                InstrRef::Call { moves, .. } | InstrRef::SuspendingCall { moves, .. } => {
                    out.push((b, at, moves.iter().map(|o| (o.position, o.slot)).collect()));
                }
                _ => {}
            }
        }
    }
    out
}

/// Every offer any non-tail call in `func` makes.
fn offers_in(func: &LirOwned) -> OfferList {
    non_tail_calls(func)
        .into_iter()
        .flat_map(|(_, _, moves)| moves)
        .collect()
}

#[test]
fn a_call_that_is_its_arguments_last_use_offers_the_slot_its_release_loads() {
    // `t` is read nowhere after `(q t)`, so the release of its region follows
    // the call. The offer is that release's slot: the runtime stamps the slot
    // `nil` when the callee takes the move, so the release that follows loads
    // an immediate. An offer naming any other slot leaves the release armed,
    // and the callee's release of its parameter is then the second one.
    let module = compile_to_lir(
        "(begin (def p (fn (g) nil)) \
         (def w (fn (q xs) (let [t (xs 0)] (begin (q t) 0)))) \
         (w p [1]))",
    );
    let w = named(&module, "w");
    let calls = non_tail_calls(w);
    let (b, at, moves) = calls
        .iter()
        .find(|(_, _, moves)| !moves.is_empty())
        .unwrap_or_else(|| panic!("`(q t)` offers nothing (calls={:?})", offers_in(w)));
    assert_eq!(moves.len(), 1, "one argument, one offer (moves={moves:?})");
    let (position, slot) = moves[0];
    assert_eq!(position, 0, "the offer names the argument's position");
    let released: Vec<u16> = value_released(b.instrs().skip(at + 1));
    assert!(
        released.contains(&slot),
        "the offered slot {slot} is not a slot the call's own releases load \
         (released={released:?})",
    );
}

#[test]
fn an_argument_named_in_its_own_position_is_offered() {
    // The trap: ANF names a call written straight into an argument with a
    // `let` inside the argument, `(q (let [t (xs 0)] t))`, rather than around
    // the call. The argument's value is still the read of `t`'s slot, which the
    // call node releases, so it is offered as the first test's `t` is.
    let module = compile_to_lir(
        "(begin (def p (fn (g) nil)) \
         (def w (fn (q xs) (begin (q (xs 0)) 0))) \
         (w p [1]))",
    );
    let w = named(&module, "w");
    let offers = offers_in(w);
    assert_eq!(
        offers
            .iter()
            .map(|(position, _)| *position)
            .collect::<Vec<_>>(),
        vec![0],
        "the argument ANF named in place is not offered (offers={offers:?})",
    );
}

#[test]
fn two_arguments_named_in_place_are_both_offered() {
    // `(q (xs 0) (xs 1))`: each argument is a `let` ANF wraps in place, and
    // neither names the other's region. Evaluating the second after the first
    // is loaded runs nothing that releases the first, so both are offered.
    let module = compile_to_lir(
        "(begin (def p (fn (a b) nil)) \
         (def w (fn (q xs) (begin (q (xs 0) (xs 1)) 0))) \
         (w p [1 2]))",
    );
    let offers = offers_in(named(&module, "w"));
    assert_eq!(
        offers
            .iter()
            .map(|(position, _)| *position)
            .collect::<Vec<_>>(),
        vec![0, 1],
        "two arguments named in place are not both offered (offers={offers:?})",
    );
}

#[test]
fn a_call_to_a_primitive_offers_nothing() {
    // A primitive is a native, and a native takes no move, so the call carries
    // no offer for the runtime to decline.
    let module = compile_to_lir(
        "(begin (def w (fn (xs) (let [t (xs 0)] (begin (string t) 0)))) \
         (w [1]))",
    );
    let offers = offers_in(named(&module, "w"));
    assert!(
        offers.is_empty(),
        "a call to a primitive offers its argument (offers={offers:?})",
    );
}

#[test]
fn an_argument_read_after_the_call_is_not_offered() {
    // The frame still reads `t` after the call, so its release is elsewhere
    // and the frame's reference is not the callee's to take.
    let module = compile_to_lir(
        "(begin (def p (fn (g) nil)) \
         (def w (fn (q xs) (let [t (xs 0)] (begin (q t) (length t))))) \
         (w p [1]))",
    );
    let offers = offers_in(named(&module, "w"));
    assert!(
        offers.is_empty(),
        "an argument the frame reads after the call was offered (offers={offers:?})",
    );
}

#[test]
fn a_value_in_two_argument_positions_is_not_offered() {
    // The frame holds one reference to `t`'s region and the callee releases
    // each parameter once. Offered in both positions, the one reference could
    // be taken twice, and the callee's second release frees the value under
    // the first parameter.
    let module = compile_to_lir(
        "(begin (def p (fn (a b) nil)) \
         (def w (fn (q xs) (let [t (xs 0)] (begin (q t t) 0)))) \
         (w p [1]))",
    );
    let offers = offers_in(named(&module, "w"));
    assert!(
        offers.is_empty(),
        "a value named by two arguments was offered (offers={offers:?})",
    );
}

#[test]
fn a_callee_passed_to_itself_is_not_offered() {
    // `(h h)`: the callee runs in the closure the argument names. Moved, its
    // release of the parameter frees the closure it is still running.
    let module = compile_to_lir(
        "(begin (def mk (fn () (fn (f) 0))) \
         (def w (fn () (let [h (mk)] (begin (h h) 0)))) \
         (w))",
    );
    let offers = offers_in(named(&module, "w"));
    assert!(
        offers.is_empty(),
        "a closure passed to itself was offered (offers={offers:?})",
    );
}

#[test]
fn a_value_another_argument_borrows_from_is_not_offered() {
    // `(first t)` is a borrow out of `t`'s pages, so the call names `t`'s
    // region twice: once as the value and once as the container of another
    // argument. The callee could release `t` and then read the borrow.
    let module = compile_to_lir(
        "(begin (def p (fn (a b) nil)) \
         (def w (fn (q xs) (let [t (xs 0)] (begin (q t (first t)) 0)))) \
         (w p [[1 2]]))",
    );
    let w = named(&module, "w");
    let offered_first: OfferList = offers_in(w)
        .into_iter()
        .filter(|(position, _)| *position == 0)
        .collect();
    assert!(
        offered_first.is_empty(),
        "the container of another argument's borrow was offered \
         (offers={offered_first:?})",
    );
}
