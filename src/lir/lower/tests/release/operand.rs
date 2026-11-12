// audited: 2026-11-12
//! What a tail-call operand hands the call: an allocating `%`-op's own region, a borrowed name's containers, never a value its `let` binds.
//!
//! docs/impl/region/relocate.md
//! docs/impl/region/operand.md
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
/// release of that slot that precedes a `TailCall` on some path. Such a release
/// sits in a block's run before its own `TailCall`, or anywhere in an entry
/// block that makes no call, since that block branches to every call.
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
        let cut = match tail_call_at(&b) {
            Some(at) => at,
            None if n == 0 => usize::MAX,
            None => continue,
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
/// at the binder. The relocation carries a release ahead of the call `g` makes,
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
        // A `def` encloses no branch, so the release lands in the arm that
        // reads `fns` itself, and the arm that calls `g` must not read as dead.
        "(begin (def pick (fn (fns c) (begin (def g (%first fns)) \
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

/// The first `%pair` in `func`: the static region it allocates into and the
/// slot its result is stored to, the binding the pair's `let` holds it in.
fn pair_region_and_slot(func: &LirOwned) -> Option<(StaticRegion, u16)> {
    let instrs = flat_instrs(func);
    let (dst, region) = instrs.iter().find_map(|i| match i {
        InstrRef::List { dst, region, .. } => Some((*dst, *region)),
        _ => None,
    })?;
    let slot = instrs.iter().find_map(|i| match i {
        InstrRef::StoreLocal { slot, src } if *src == dst => Some(*slot),
        _ => None,
    })?;
    Some((region, slot))
}

/// Whether the closure of `module` that binds a `%pair` with a `let` releases
/// the pair ahead of the `TailCall` in its tail-call block. Either route counts:
/// a value release of the pair's slot, or a release of its region by id. With
/// it, that block's instructions up to the call, for the message.
fn pair_released_before_the_tail_call(module: &FrozenModule) -> (bool, Vec<String>) {
    let run = module
        .closures
        .iter()
        .find(|f| pair_region_and_slot(f).is_some())
        .expect("the closure binding a to a %pair");
    let (region, slot) = pair_region_and_slot(run).expect("the %pair's region and slot");
    let (block, at) = tail_call_blocks_of(run)
        .next()
        .expect("the call to sink replaces the frame");
    let by_value = value_releases(block.instrs())
        .into_iter()
        .any(|(idx, s)| idx < at && s == slot);
    let by_id = block
        .instrs()
        .take(at)
        .any(|i| matches!(i, InstrRef::DecrefRegion { region_id } if region_id == region));
    let shown = block.instrs().take(at).map(|i| format!("{i:?}")).collect();
    (by_value || by_id, shown)
}

/// `a` is bound once, and each arm hands it to a native whose fresh result is
/// the operand's value. The callee receives that result and nothing of `a`, so
/// the frame's release of `a` is owed before the call.
///
/// The counter-factual: the exemption read every child of the `let`, found
/// `a`'s allocation in its binding, and left the release in the dead block.
/// The closure path never runs it there, so each call strands one region: the
/// closure a worker spawn runs, handed to a `join` that replaces the frame.
#[test]
fn a_let_bound_value_the_arms_hand_a_native_is_released_before_the_tail_call() {
    let module = compile_to_lir(
        "(begin (def sink (fn (h) (block :keep (length h)))) \
                (def run (fn (x c) (sink (let [a (%pair x nil)] \
                                           (if c (list (length a)) (list (length a) 0)))))) \
                (run 1 true))",
    );
    let (released, before) = pair_released_before_the_tail_call(&module);
    assert!(
        released,
        "a is not released before the TailCall; the block up to the call: {before:#?}",
    );
}

/// The arms hand the call `a` itself, so the frame's release of `a` is the
/// ownership move the callee's owned-parameter release consumes, and it stays
/// behind the `TailCall`.
#[test]
fn a_let_bound_value_an_arm_hands_the_call_stays_behind_it() {
    let module = compile_to_lir(
        "(begin (def sink (fn (h) (block :keep (length h)))) \
                (def run (fn (x c) (sink (let [a (%pair x nil)] (if c a a))))) \
                (run 1 true))",
    );
    let (released, before) = pair_released_before_the_tail_call(&module);
    assert!(
        !released,
        "a is released ahead of the TailCall that moves it; the block up to the call: \
         {before:#?}",
    );
}

/// The block of `module`'s closures that builds the first `%pair`, read two
/// ways: the slot the pair read its tail from, and every value route that block
/// runs after the pair. Each route says whether it stamps the slot it read.
fn pair_arm_routes(module: &FrozenModule) -> (u16, Vec<(u16, bool)>) {
    for func in &module.closures {
        for b in func.view().blocks() {
            let instrs: Vec<InstrRef<'_>> = b.instrs().collect();
            let mut from_slot: rustc_hash::FxHashMap<Reg, u16> = rustc_hash::FxHashMap::default();
            for (idx, i) in instrs.iter().enumerate() {
                match *i {
                    InstrRef::LoadLocal { dst, slot } => {
                        from_slot.insert(dst, slot);
                    }
                    InstrRef::List { tail, .. } => {
                        let acc = *from_slot
                            .get(&tail)
                            .expect("the %pair reads acc from its slot");
                        return (acc, stamped_value_routes(&instrs[idx + 1..]));
                    }
                    _ => {}
                }
            }
        }
    }
    panic!("no closure builds a %pair");
}

/// The tail operand is a branch whose pair arm hands the call a pair that holds
/// `acc` by its own count, and whose other arm hands the call `acc` itself. So
/// on the pair arm the frame still owes its release of `acc`. The arm runs it
/// once the pair is built, and stamps the slot so the copy behind the call does
/// nothing on that path.
///
/// The counter-factual: the exemption took the union over the arms and left
/// the release behind the `TailCall` on both paths, where the closure path
/// never runs it. Each kept element stranded the accumulator it displaced,
/// which is how stdlib `filter` over a list leaked.
///
/// The second shape reads `acc` in a later operand, which runs after the arm.
/// A release in the arm would free what that read reaches, so the arm runs none.
#[test]
fn a_branch_operand_arm_releases_what_only_its_sibling_hands_the_call() {
    let module = compile_to_lir(
        "(begin (def keep (fn (c acc) \
                  (if (empty? c) (length acc) \
                    (keep (rest c) (if (%lt 0 (first c)) (%pair (first c) acc) acc))))) \
                (keep (list 1 2) ()))",
    );
    let (acc, routes) = pair_arm_routes(&module);
    assert!(
        routes.contains(&(acc, true)),
        "the pair arm does not release acc (slot {acc}) with its stamp; routes after \
         the pair: {routes:?}",
    );

    let module = compile_to_lir(
        "(begin (def keep (fn (c acc n) \
                  (if (empty? c) n \
                    (keep (rest c) (if (%lt 0 (first c)) (%pair (first c) acc) acc) \
                          (length acc))))) \
                (keep (list 1 2) () 0))",
    );
    let (acc, routes) = pair_arm_routes(&module);
    assert!(
        routes.iter().all(|&(slot, _)| slot != acc),
        "the pair arm releases acc (slot {acc}) ahead of the later operand that reads \
         it; routes after the pair: {routes:?}",
    );
}
