// audited: 2026-09-15
// ── The collection a rest pattern built, at a tail call ──────────
//
// THE TRAP: the collection's release route is the slot the lowerer parked it
// in, and the call passes the binding's slot. A reading that compares those two
// slots alone finds them different and carries the release ahead of the call.
//
// These pin the PLACEMENT of that one release, per slot. The counts are the
// same either way, so only position tells the ownership move from a free under
// the callee's own read.
//
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
