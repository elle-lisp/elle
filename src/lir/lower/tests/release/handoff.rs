// audited: 2026-11-12
//! Which arguments a call offers to move into its callee, and which borrowed tail arguments wait for the runtime's mint.
//!
//! docs/impl/region/handoff.md

use super::*;

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

#[test]
fn a_reassigned_binding_is_not_offered() {
    // A reassigned binding's slot is not a release route: by the release it
    // holds whatever was last assigned (docs/impl/region/bindings.md), and the
    // stamp a taken move leaves there would zero a live value.
    //
    // The trap: functionalization turns an `assign` in straight-line code into
    // a fresh immutable binding, which owns its slot and may be offered, and a
    // loop's reassigned binding reached no call bucket in any shape tried. So
    // the test marks every `let` binding reassigned at the `make_lowerer_with`
    // seam, over the shape the first test sees offered. Without the refusal,
    // `t` is offered here exactly as it is there.
    fn let_bindings(h: &Hir, out: &mut Vec<Binding>) {
        if let HirKind::Let { bindings, .. } = &h.kind {
            out.extend(bindings.iter().map(|(b, _)| *b));
        }
        h.for_each_child(|c| let_bindings(c, out));
    }
    let (mut lowerer, hir) = make_lowerer_with(
        "(begin (def p (fn (g) nil)) \
         (def w (fn (q xs) (let [t (xs 0)] (begin (q t) 0)))) \
         (w p [1]))",
        |info, hir| {
            let mut lets = Vec::new();
            let_bindings(hir, &mut lets);
            info.reassigned_local_bindings.extend(lets);
        },
    );
    let module = lowerer.lower(&hir).expect("lower");
    let offers = offers_in(named(&module, "w"));
    assert!(
        offers.is_empty(),
        "a reassigned binding was offered (offers={offers:?})",
    );
}

// ── The late mint ────────────────────────────────────────────────
// A borrowed tail argument is minted for a closure callee alone, at the
// `TailCall`, where the leaf is an immutable upvalue or a heap constant. These
// pin which leaves the lowerer leaves to the runtime, and that it still names
// the stash the fall-through releases.

/// Every `TailCall` in `func`, as its block, its index there, its borrowed
/// stash slots, and its late mints as `(position, slot)`.
fn tail_calls(func: &LirOwned) -> Vec<(BlockRef<'_>, usize, Vec<u16>, OfferList)> {
    tail_call_blocks_of(func)
        .filter_map(|(b, at)| match b.instrs().nth(at)? {
            InstrRef::TailCall {
                borrowed_arg_slots,
                late_mints,
                ..
            } => Some((
                b,
                at,
                borrowed_arg_slots.iter().collect(),
                late_mints.iter().map(|o| (o.position, o.slot)).collect(),
            )),
            _ => None,
        })
        .collect()
}

/// The one function in `module` that holds a `TailCall` naming a borrowed
/// argument.
fn the_borrowing_tail_call(module: &FrozenModule) -> (BlockRef<'_>, usize, Vec<u16>, OfferList) {
    let mut found: Vec<_> = functions(module)
        .flat_map(tail_calls)
        .filter(|(_, _, stash, _)| !stash.is_empty())
        .collect();
    assert_eq!(found.len(), 1, "one tail call names a borrowed argument");
    found.remove(0)
}

#[test]
fn an_immutable_upvalue_is_minted_at_the_tail_call() {
    // `s` is an immutable capture, held by the closure's environment for as
    // long as the closure runs. Nothing the frame does before the call can
    // release it, so the mint can wait for the callee: `identical?` is a native
    // and would release the mint in the fall-through at once.
    let module = compile_to_lir(
        "(begin (def mk (fn (s) (fn (g) (identical? g s)))) \
         ((mk (string \"x\")) (string \"y\")))",
    );
    let (b, at, stash, late) = the_borrowing_tail_call(&module);
    assert_eq!(
        late,
        vec![(1, stash[0])],
        "the upvalue is not left to the runtime's mint at its stash \
         (stash={stash:?}, late={late:?})",
    );
    let retains = positions(&b, |i| matches!(i, InstrRef::IncrefValueRegion { .. }));
    assert!(
        retains.iter().all(|&r| r > at),
        "a late-minted argument is still retained ahead of the TailCall \
         (at={at}, retains={retains:?})",
    );
    let released = value_released(b.instrs().skip(at + 1));
    assert!(
        released.contains(&stash[0]),
        "the fall-through no longer releases the stash (released={released:?})",
    );
}

#[test]
fn a_heap_constant_is_minted_at_the_tail_call() {
    // A quoted list is a constant the frame holds no reference to, and the
    // constant pool keeps it for as long as the code.
    let module = compile_to_lir("(begin (def c '(1 2)) (def f (fn (h) (h c))) (f (fn (x) x)))");
    let (_, _, stash, late) = the_borrowing_tail_call(&module);
    assert_eq!(
        late,
        vec![(0, stash[0])],
        "the heap constant is not left to the runtime's mint \
         (stash={stash:?}, late={late:?})",
    );
}

#[test]
fn a_pattern_leaf_is_minted_ahead_of_the_tail_call() {
    // A pattern leaf lives in its scrutinee's pages, and the frame may release
    // the scrutinee before the `TailCall`. Its mint has to come first.
    let module = compile_to_lir(
        "(begin (def k (fn (a b) b)) \
         (def m (fn (t) (let [[s f] t] (let [g f] (k s g))))) \
         (m [1 (fn (x) x)]))",
    );
    let (_, _, stash, late) = the_borrowing_tail_call(&module);
    assert_eq!(stash.len(), 2, "both leaves are borrowed (stash={stash:?})");
    assert!(
        late.is_empty(),
        "a pattern leaf was left to the runtime's mint (late={late:?})",
    );
}

#[test]
fn a_reassigned_capture_is_minted_ahead_of_the_tail_call() {
    // `x` is a cell another call may reassign before this one runs, which drops
    // the value it held. Its mint has to come first.
    let module = compile_to_lir(
        "(begin (def f (fn () (begin (def @x (list 1)) \
         (let [h (fn () (length x))] (begin (assign x (list 2)) (h)))))) \
         (f))",
    );
    let (_, _, stash, late) = the_borrowing_tail_call(&module);
    assert_eq!(stash.len(), 1, "the capture is borrowed (stash={stash:?})");
    assert!(
        late.is_empty(),
        "a reassigned capture was left to the runtime's mint (late={late:?})",
    );
}
