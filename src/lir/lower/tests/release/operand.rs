// audited: 2026-10-16
//! What a tail-call operand hands the call: an allocating `%`-op's own region, and the containers a borrowed name reads out of.
//!
//! docs/impl/region/relocate.md
//! docs/impl/region/borrow.md

use super::*;

/// The block of `module`'s closures that makes a frame-replacing tail call:
/// the slots its `%pair` read its tail operand from, and the slots it releases
/// by value before the `TailCall`.
fn pair_tail_and_early_releases(module: &FrozenModule) -> (Option<u16>, Vec<u16>) {
    let (block, _) = module
        .closures
        .iter()
        .flat_map(tail_call_blocks_of)
        .next()
        .expect("the recursive arm lowers to a TailCall");
    let mut from_slot: rustc_hash::FxHashMap<Reg, u16> = rustc_hash::FxHashMap::default();
    let mut pair_tail = None;
    let mut released = Vec::new();
    for i in block.instrs() {
        match i {
            InstrRef::LoadLocal { dst, slot } => {
                from_slot.insert(dst, slot);
            }
            InstrRef::List { tail, .. } => pair_tail = from_slot.get(&tail).copied(),
            InstrRef::DecrefValueRegion { src } => released.extend(from_slot.get(&src)),
            InstrRef::TailCall { .. } => break,
            _ => {}
        }
    }
    (pair_tail, released)
}

/// `step` hands its tail call a fresh pair, and the pair holds `acc` by a count
/// of its own. So the frame's release of `acc` is owed before the call, like
/// any release of a region the call does not receive.
///
/// The counter-factual: the exemption descended the `%pair` into its operands,
/// read `acc` as handed to the call, and left its release after the
/// `TailCall`, where the closure path never runs it. Each call stranded the
/// accumulator it displaced — the oracle's `recur-acc-return` probe, once
/// `pair` grafts to `%pair`.
#[test]
fn an_accumulator_a_pair_holds_is_released_before_the_tail_call() {
    let module = compile_to_lir(
        "(begin (def step (fn (i n acc) (if (%lt i n) (step (%add i 1) n (%pair i acc)) acc))) \
                (step 0 4 ()))",
    );
    let (pair_tail, released) = pair_tail_and_early_releases(&module);
    let acc = pair_tail.expect("the %pair reads acc from its slot");
    assert!(
        released.contains(&acc),
        "acc (slot {acc}) is not released before the TailCall; released before it: \
         {released:?}",
    );
}

/// The slot the first `%first` in `func` reads its pair from, and every
/// release of that slot that precedes a `TailCall` on some path: one in a
/// block's run before its own `TailCall`, or one in the entry block, which
/// branches to every call.
fn read_container_released_before_a_tail_call(func: &LirOwned) -> (Option<u16>, Vec<usize>) {
    let mut container = None;
    for b in func.view().blocks() {
        let mut from_slot: rustc_hash::FxHashMap<Reg, u16> = rustc_hash::FxHashMap::default();
        for i in b.instrs() {
            match i {
                InstrRef::LoadLocal { dst, slot } => {
                    from_slot.insert(dst, slot);
                }
                InstrRef::First { pair, .. } if container.is_none() => {
                    container = from_slot.get(&pair).copied();
                }
                _ => {}
            }
        }
    }
    let Some(slot) = container else {
        return (None, Vec::new());
    };
    let mut early = Vec::new();
    for (n, b) in func.view().blocks().enumerate() {
        let cut = if n == 0 {
            usize::MAX
        } else {
            match tail_call_at(&b) {
                Some(at) => at,
                None => continue,
            }
        };
        for (idx, s) in value_releases(b.instrs()) {
            if s == slot && idx < cut {
                early.push(n);
            }
        }
    }
    (Some(slot), early)
}

/// `g` is a closure read out of `fns` with no count, and the call reaches the
/// closure through that borrow alone. So `fns` must outlive the call: its
/// release stays behind every `TailCall`, on each arm. The arm that calls `g`
/// never names `fns`, so it is no dead arm for a compensating release either.
///
/// The counter-factual: with `g` naming no region, the solver releases `fns`
/// at the binder, the relocation carries a release ahead of the call `g` makes,
/// and a compensating release lands at the head of `g`'s arm. Each frees the
/// pair, and the cascade frees `g`'s closure under the call that enters it.
#[test]
fn a_container_a_tail_callee_is_read_out_of_stays_behind_the_call() {
    for source in [
        "(begin (def pick (fn (fns) (let [g (begin nil (%first fns))] (g 1)))) \
                (pick (f 1)))",
        "(begin (def pick (fn (fns c) (let [g (begin nil (%first fns))] \
                                        (if c (g 1) (length fns))))) \
                (pick (f 1) true))",
    ] {
        let module = compile_to_lir(source);
        let pick = module
            .closures
            .iter()
            .find(|f| func_count(f, |i| matches!(i, InstrRef::First { .. })) == 1)
            .expect("the closure reading g out of fns");
        let (slot, early) = read_container_released_before_a_tail_call(pick);
        let slot = slot.expect("the %first reads fns from its slot");
        assert!(
            early.is_empty(),
            "fns (slot {slot}) is released ahead of a tail call in blocks {early:?}: {source}",
        );
    }
}
