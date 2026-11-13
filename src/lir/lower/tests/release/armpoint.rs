// audited: 2026-11-12
//! The point each arm of a tail call's branch operand opens: what only a sibling arm hands the call, and nothing the operand allocates.
//!
//! docs/impl/region/operand.md
//! docs/impl/region/replicate.md

use super::operand::pair_region_and_slot;
use super::*;

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

/// The pair is allocated by the operand, inside the arm that hands it over. On
/// the sibling arm its slot is never written, so that arm owes no release of
/// it. Only the copy behind the call, for the native fall-through, reads the
/// slot.
///
/// The counter-factual: the sibling arm admitted the pair's region as one only
/// the pair arm hands the call, and released the pair's slot. The JIT's
/// self-tail-call loop leaves the pair the previous trip moved to the call in
/// that slot, so the release freed a reference the frame no longer owned.
#[test]
fn a_branch_operand_arm_releases_nothing_the_operand_allocates() {
    let module = compile_to_lir(
        "(begin (def keep (fn (c acc) \
                  (if (empty? c) (length acc) \
                    (keep (rest c) (if (%lt 0 (first c)) (%pair (first c) acc) acc))))) \
                (keep (list 1 2) ()))",
    );
    let keep = module
        .closures
        .iter()
        .find(|f| pair_region_and_slot(f).is_some())
        .expect("the closure building the %pair");
    let (_, slot) = pair_region_and_slot(keep).expect("the %pair's slot");
    let mut early = Vec::new();
    for (n, b) in keep.view().blocks().enumerate() {
        if b.instrs().any(|i| matches!(i, InstrRef::List { .. })) {
            continue;
        }
        let cut = tail_call_at(&b).unwrap_or(usize::MAX);
        if value_releases(b.instrs())
            .into_iter()
            .any(|(idx, s)| s == slot && idx < cut)
        {
            early.push(n);
        }
    }
    assert!(
        early.is_empty(),
        "blocks {early:?} release the pair's slot {slot} ahead of any tail call, on a \
         path that never wrote it",
    );
}
