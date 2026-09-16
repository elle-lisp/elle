// audited: 2026-09-16
// ── The collection a rest pattern built: its route, and its place ─
//
// THE TRAP: the collection's release route is the slot the lowerer parked it
// in, and the call passes the binding's slot. A reading that compares those two
// slots alone finds them different and carries the release ahead of the call.
//
// The tail-call group pins the PLACEMENT of that one release, per slot. The
// counts are the same either way, so only position tells the ownership move
// from a free under the callee's own read. The group above it pins the route
// itself, for the builds no name of the program reaches.
//
// docs/impl/region/anchors.md
// docs/impl/region/relocate.md

use super::*;

/// What the first `TailCall`-bearing block does with the collection a rest
/// pattern built: the call's index, the index of the release routed through the
/// PARKED slot, and the indices of every other value release in that block.
///
/// The parked slot is read off the emission — the `StoreLocal` that takes the
/// result of the building opcode (`ArrayMutSliceFrom` / `StructRest`) — because
/// that is the one thing that tells this release from the scrutinee's, which
/// shares the block and must land on the other side of the call.
fn rest_collection_release_layout(
    module: &crate::lir::LirModule,
) -> Option<(usize, Vec<usize>, Vec<usize>)> {
    let funcs = std::iter::once(&module.entry).chain(module.closures.iter());
    for f in funcs {
        for b in &f.blocks {
            let Some(at) = b
                .instructions
                .iter()
                .position(|i| matches!(i.instr, LirInstr::TailCall { .. }))
            else {
                continue;
            };
            // The register each building opcode produced, then the slot the
            // store parked it in.
            let mut built: rustc_hash::FxHashSet<Reg> = rustc_hash::FxHashSet::default();
            let mut park_slots: rustc_hash::FxHashSet<u16> = rustc_hash::FxHashSet::default();
            let mut from_slot: rustc_hash::FxHashMap<Reg, u16> = rustc_hash::FxHashMap::default();
            let (mut parked, mut other) = (Vec::new(), Vec::new());
            for (idx, i) in b.instructions.iter().enumerate() {
                match &i.instr {
                    LirInstr::ArrayMutSliceFrom { dst, .. } | LirInstr::StructRest { dst, .. } => {
                        built.insert(*dst);
                    }
                    LirInstr::StoreLocal { slot, src } if built.contains(src) => {
                        park_slots.insert(*slot);
                    }
                    LirInstr::LoadLocal { dst, slot } => {
                        from_slot.insert(*dst, *slot);
                    }
                    LirInstr::DecrefValueRegion { src } => match from_slot.get(src) {
                        Some(slot) if park_slots.contains(slot) => parked.push(idx),
                        _ => other.push(idx),
                    },
                    _ => {}
                }
            }
            return Some((at, parked, other));
        }
    }
    None
}

/// How many collections a rest pattern BUILT across the module, and how many
/// releases load the slot one of them was parked in.
///
/// The parked slot is read off the emission — the `StoreLocal` that takes the
/// result of a building opcode — because the lowerer's own slot is the one
/// thing that tells these releases from the scrutinee's.
fn rest_build_and_release_counts(module: &crate::lir::LirModule) -> (usize, usize) {
    let (mut builds, mut releases) = (0, 0);
    for f in std::iter::once(&module.entry).chain(module.closures.iter()) {
        let mut built: rustc_hash::FxHashSet<Reg> = rustc_hash::FxHashSet::default();
        let mut park_slots: rustc_hash::FxHashSet<u16> = rustc_hash::FxHashSet::default();
        let mut from_slot: rustc_hash::FxHashMap<Reg, u16> = rustc_hash::FxHashMap::default();
        for b in &f.blocks {
            for i in &b.instructions {
                match &i.instr {
                    LirInstr::ArrayMutSliceFrom { dst, .. } | LirInstr::StructRest { dst, .. } => {
                        builds += 1;
                        built.insert(*dst);
                    }
                    LirInstr::StoreLocal { slot, src } if built.contains(src) => {
                        park_slots.insert(*slot);
                    }
                    LirInstr::LoadLocal { dst, slot } => {
                        from_slot.insert(*dst, *slot);
                    }
                    LirInstr::DecrefValueRegion { src }
                        if from_slot.get(src).is_some_and(|s| park_slots.contains(s)) =>
                    {
                        releases += 1;
                    }
                    _ => {}
                }
            }
        }
    }
    (builds, releases)
}

#[test]
fn a_wildcard_rest_emits_no_build_at_all() {
    // `_` reads nothing out of the collection, so there is nothing for the
    // build to hand anybody. The fixed element's own strict extraction keeps
    // the type check the skipped opcode would have made.
    let module = compile_to_lir("(begin (def f (fn (t) (let [[x & _] t] x))) (f (list 1 2)))");
    assert_eq!(
        rest_build_and_release_counts(&module),
        (0, 0),
        "a wildcard rest built a collection nothing can read"
    );
}

#[test]
fn a_bare_wildcard_rest_keeps_its_build_and_releases_it() {
    // THE COUNTER-FACTUAL. `ArrayMutSliceFrom` checks the scrutinee is an array
    // as well as building the slice, and this pattern has no fixed element to
    // make that check. The build stays, so the release must be there too — and
    // no name of the program can key it.
    let module = compile_to_lir("(begin (def f (fn (t) (let [[& _] t] t))) (f (list 1 2)))");
    assert_eq!(
        rest_build_and_release_counts(&module),
        (1, 1),
        "the build a bare wildcard rest keeps is released through its parked slot"
    );
}

#[test]
fn a_collection_no_name_reaches_is_parked_and_released() {
    // `[x & [& q]]` builds two collections and `q` holds the INNER one, so the
    // outer is reached by no name at all. Keyed on a name it takes no
    // placeholder, is parked in no slot, and strands; keyed on its position
    // among the pattern's builds it takes a route like the inner one's.
    let module =
        compile_to_lir("(begin (def f (fn (t) (let [[x & [& q]] t] q))) (f (list 1 2 3)))");
    assert_eq!(
        rest_build_and_release_counts(&module),
        (2, 2),
        "two builds, and a release through the parked slot of each"
    );
}

#[test]
fn a_rest_collection_handed_to_the_tail_call_is_released_after_it() {
    // The exemption: `r` IS the tail call's argument, so the release the
    // closure path never runs is the ownership move the callee's owned-param
    // release consumes. Carried back ahead of the call it frees the collection
    // the callee is about to read — `(length r)` deref'ing released pages.
    let module = compile_to_lir(
        "(begin (def s (fn (a) a)) (def f (fn (t) (let [[x & r] t] (s r)))) (f (list 1 2 3)))",
    );
    let (at, parked, _) =
        rest_collection_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        !parked.is_empty(),
        "no release is routed through the parked slot, so this pins nothing",
    );
    assert!(
        parked.iter().all(|&r| r > at),
        "the built collection's release was carried ahead of the TailCall \
         (at={at}, parked={parked:?}) — that release IS the ownership move",
    );
}

#[test]
fn a_struct_rest_collection_handed_to_the_tail_call_is_released_after_it() {
    // `StructRest` builds its collection exactly as `ArrayMutSliceFrom` does,
    // and reaches the relocation through the same parked slot.
    let module = compile_to_lir(
        "(begin (def s (fn (a) a)) \
                (def f (fn (t) (let [{:a one & r} t] (s r)))) \
                (f (list 1 2 3)))",
    );
    let (at, parked, _) =
        rest_collection_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        !parked.is_empty(),
        "no release is routed through the parked slot, so this pins nothing",
    );
    assert!(
        parked.iter().all(|&r| r > at),
        "a struct rest collection's release was carried ahead of the TailCall \
         (at={at}, parked={parked:?})",
    );
}

#[test]
fn a_nested_rest_collection_reached_by_the_tail_call_is_released_after_it() {
    // THE COUNTER-FACTUAL. The call receives `p`, an ELEMENT of the collection,
    // and the caller holds no counted reference on an element to move. Carried
    // back ahead of the call, the collection's release drops its reference on
    // that element and the callee's owned-param release then takes the last one
    // — under its own read. The strand is the answer, so the release stays.
    let module = compile_to_lir(
        "(begin (def s (fn (a) a)) \
                (def f (fn (t) (let [[x & [p q]] t] (s p)))) \
                (f (list 1 2 3)))",
    );
    let (at, parked, _) =
        rest_collection_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        !parked.is_empty(),
        "no release is routed through the parked slot, so this pins nothing",
    );
    assert!(
        parked.iter().all(|&r| r > at),
        "a nested rest collection's release was carried ahead of the TailCall \
         (at={at}, parked={parked:?}) — it frees the element the callee reads",
    );
}

#[test]
fn the_scrutinee_release_still_precedes_the_tail_call() {
    // The counter-factual, and the over-fix this guards: the scrutinee is a
    // region the rest name only NAMES. The call never receives it, so nothing
    // takes its release over and it must still be carried back ahead of the
    // call. Exempting every region a rest name carries would strand one region
    // per call — the leak the slot comparison exists to close.
    let module = compile_to_lir(
        "(begin (def s (fn (a) a)) (def f (fn (t) (let [[x & r] t] (s r)))) (f (list 1 2 3)))",
    );
    let (at, _, other) =
        rest_collection_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        other.iter().any(|&r| r < at),
        "no release reached the block ahead of the TailCall (at={at}, \
         other={other:?}) — the scrutinee is stranded on the closure path",
    );
}
