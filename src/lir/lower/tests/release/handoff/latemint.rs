// audited: 2026-11-13
//! Which borrowed tail arguments the lowerer leaves to the runtime's mint at the `TailCall`, and which it mints ahead of the call.
//!
//! docs/impl/region/handoff.md
//!
//! A borrowed tail argument is minted for a closure callee alone, at the
//! `TailCall`, where the leaf is an immutable upvalue or a heap constant. These
//! pin which leaves the lowerer leaves to the runtime, and that it still names
//! the stash the fall-through releases.

use super::*;

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
