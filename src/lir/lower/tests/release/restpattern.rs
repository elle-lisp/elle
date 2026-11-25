// audited: 2026-11-24
//! The collection a rest pattern built: the slot its release is routed through, and where that release lands.
//!
//! docs/impl/region/anchors.md
//! docs/impl/region/relocate.md

// THE TRAP: the collection's release route is the slot the lowerer parked it
// in, and the call passes the binding's slot. A reading that compares those two
// slots alone finds them different and carries the release ahead of the call.
//
// The tail-call group pins the PLACEMENT of that one release, per slot. The
// counts are the same either way, so only position tells the ownership move
// from a free under the callee's own read. The group above it pins the route
// itself, for the builds no name of the program reaches.

use super::*;
use rustc_hash::{FxHashMap, FxHashSet};

/// How many collections a rest pattern builds in a function, and the slots it
/// parks them in: each a `StoreLocal` fills from a building opcode, the one
/// thing that tells these releases from the scrutinee's. Each source here
/// destructures a parameter of unknown kind, so an array rest takes both
/// slices into one slot, and `ArrayMutSliceFrom` counts each rest once.
fn rest_builds(func: &LirOwned) -> (usize, FxHashSet<u16>) {
    let (mut built, mut park_slots) = (FxHashSet::default(), FxHashSet::default());
    let mut builds = 0;
    for i in flat_instrs(func) {
        match i {
            InstrRef::ArrayMutSliceFrom { dst, .. } | InstrRef::StructRest { dst, .. } => {
                builds += 1;
                built.insert(dst);
            }
            InstrRef::StoreLocal { slot, src } if built.contains(&src) => {
                park_slots.insert(slot);
            }
            _ => {}
        }
    }
    (builds, park_slots)
}

/// Each `DecrefValueRegion` in a run of instructions, as its index and whether
/// it loads one of the `parked` slots.
fn value_releases<'a>(
    instrs: impl IntoIterator<Item = InstrRef<'a>>,
    parked: &FxHashSet<u16>,
) -> Vec<(usize, bool)> {
    let mut from_slot = FxHashMap::default();
    let mut releases = Vec::new();
    for (idx, i) in instrs.into_iter().enumerate() {
        match i {
            InstrRef::LoadLocal { dst, slot } => {
                from_slot.insert(dst, slot);
            }
            InstrRef::DecrefValueRegion { src } => {
                releases.push((idx, from_slot.get(&src).is_some_and(|s| parked.contains(s))));
            }
            _ => {}
        }
    }
    releases
}

/// What the first `TailCall`-bearing block does with the collection a rest
/// pattern built: the call's index, the index of the release routed through the
/// PARKED slot, and the indices of every other value release in that block —
/// the scrutinee's among them, which shares the block and must land on the
/// other side of the call.
fn rest_collection_release_layout(
    module: &FrozenModule,
) -> Option<(usize, Vec<usize>, Vec<usize>)> {
    let func = functions(module).find(|f| tail_call_blocks_of(f).next().is_some())?;
    let (b, at) = tail_call_blocks_of(func).next()?;
    let (_, parked_slots) = rest_builds(func);
    let releases = value_releases(b.instrs(), &parked_slots);
    let (parked, other): (Vec<_>, Vec<_>) = releases.into_iter().partition(|&(_, p)| p);
    let idx = |rs: Vec<(usize, bool)>| rs.into_iter().map(|(i, _)| i).collect();
    Some((at, idx(parked), idx(other)))
}

/// How many collections a rest pattern BUILT across the module, and how many
/// releases load the slot one of them was parked in.
fn rest_build_and_release_counts(module: &FrozenModule) -> (usize, usize) {
    let (mut builds, mut releases) = (0, 0);
    for f in functions(module) {
        let (built, parked_slots) = rest_builds(f);
        builds += built;
        releases += value_releases(flat_instrs(f), &parked_slots)
            .iter()
            .filter(|&&(_, parked)| parked)
            .count();
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
    // THE COUNTER-FACTUAL. Each array slice checks the scrutinee is an array
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
    // `StructRest` builds its collection exactly as an array slice does,
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
