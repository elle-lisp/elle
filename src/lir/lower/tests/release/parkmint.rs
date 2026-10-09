// audited: 2026-10-06
//! The reference a park mints for a payload the emitting body borrows, and the
//! receipt each site's release of it carries.
//!
//! docs/impl/region/park.md
//! docs/impl/region/unwind.md

use super::*;
use crate::lir::code::BlockRef;

/// The block an `Emit` terminates, and the resume block it jumps to.
fn park_and_resume(func: &LirOwned) -> Option<(BlockRef<'_>, BlockRef<'_>)> {
    let view = func.view();
    let (park, resume_label) = view.blocks().find_map(|b| match b.terminator() {
        crate::lir::Terminator::Emit { resume_label, .. } => Some((b, resume_label)),
        _ => None,
    })?;
    let resume = view.blocks().find(|b| b.label() == resume_label)?;
    Some((park, resume))
}

/// The block holding a function's `SuspendingCall`, and the call's index in it.
fn suspending_call(func: &LirOwned) -> Option<(BlockRef<'_>, usize)> {
    func.view().blocks().find_map(|b| {
        let at = b
            .instrs()
            .position(|i| matches!(i, InstrRef::SuspendingCall { .. }))?;
        Some((b, at))
    })
}

/// The `(retains, releases)` a module's park is wrapped in — retains in the block
/// the `Emit` terminates, releases in the resume block it jumps to. Counted per
/// block rather than per function so an unrelated mint elsewhere in the body (a
/// `Return`'s, say) cannot stand in for the park's own pair.
fn park_borrow_ops(module: &FrozenModule) -> (usize, usize) {
    fn in_func(func: &LirOwned) -> Option<(usize, usize)> {
        let (park, resume) = park_and_resume(func)?;
        Some((
            park.instrs()
                .filter(|i| matches!(i, InstrRef::IncrefValueRegion { .. }))
                .count(),
            resume
                .instrs()
                .filter(|i| matches!(i, InstrRef::DecrefValueRegion { .. }))
                .count(),
        ))
    }
    in_func(&module.entry)
        .or_else(|| module.closures.iter().find_map(in_func))
        .expect("a function terminating in Emit")
}

#[test]
fn park_mints_a_body_reference_for_a_borrowed_payload() {
    // The yielding lambda closes over a value the ENCLOSING lambda allocates and
    // releases, so it owns no reference to strand in the continuation the discard
    // discharge stands in for. `lower_emit` mints one: a retain before the
    // suspend and a release first in the resume block
    // (docs/impl/region/park.md).
    let module = compile_to_lir("(fn () (let [x (string \"a\")] (fn () (emit :yield x))))");
    let (retains, releases) = park_borrow_ops(&module);
    assert!(
        retains >= 1 && releases >= 1,
        "a borrowed yield payload must be retained across the park and released \
         at the resume; got {retains} retain(s) and {releases} release(s)",
    );
}

#[test]
fn park_mints_nothing_for_a_body_allocated_payload() {
    // The contrast: the body allocates what it yields, so its own release is
    // already the one the discharge stands in for. A second reference here would
    // be stranded at every abandoned park.
    let module = compile_to_lir("(fn () (let [x (string \"a\")] (emit :yield x)))");
    let (retains, _) = park_borrow_ops(&module);
    assert_eq!(
        retains, 0,
        "a body-allocated yield payload already carries the body's reference; \
         minting a second strands it per abandoned park",
    );
}

/// The `(retains, releases)` a NON-TAIL dynamic emit is wrapped in — retains
/// before the `SuspendingCall` in its block, releases after it. The park is an
/// ordinary call here, so the resume lands at the next instruction rather than in
/// a block of its own (docs/impl/region/park.md § "What yields is the emit
/// OPERATION, not the `Emit` node").
fn dynamic_park_borrow_ops(module: &FrozenModule) -> (usize, usize) {
    fn in_func(func: &LirOwned) -> Option<(usize, usize)> {
        let (b, at) = suspending_call(func)?;
        Some((
            b.instrs()
                .take(at)
                .filter(|i| matches!(i, InstrRef::IncrefValueRegion { .. }))
                .count(),
            b.instrs()
                .skip(at + 1)
                .filter(|i| matches!(i, InstrRef::DecrefValueRegion { .. }))
                .count(),
        ))
    }
    in_func(&module.entry)
        .or_else(|| module.closures.iter().find_map(in_func))
        .expect("a function containing a SuspendingCall")
}

#[test]
fn dynamic_park_mints_a_body_reference_for_a_borrowed_payload() {
    // A non-literal first argument makes the park an ordinary call, so there is no
    // `Emit` terminator for `lower_emit` to mint at — and no borrowed-argument
    // retain either, the call not being in tail position. The emitting lambda
    // closes over a value the enclosing one allocates and releases, so `lower_call`
    // owes the reference the discard discharge stands in for.
    let module = compile_to_lir(
        "(let [s :yield] (fn () (let [x (string \"a\")] (fn () (begin (emit s x) 0)))))",
    );
    let (retains, releases) = dynamic_park_borrow_ops(&module);
    assert!(
        retains >= 1 && releases >= 1,
        "a borrowed dynamic-emit payload must be retained across the park and \
         released after the call; got {retains} retain(s) and {releases} release(s)",
    );
}

#[test]
fn dynamic_park_mints_nothing_for_a_body_allocated_payload() {
    // The contrast, exactly as for the literal park: the body allocates what it
    // emits, so its own release is already the one the discharge stands in for.
    let module =
        compile_to_lir("(let [s :yield] (fn () (let [x (string \"a\")] (begin (emit s x) 0))))");
    let (retains, _) = dynamic_park_borrow_ops(&module);
    assert_eq!(
        retains, 0,
        "a body-allocated dynamic-emit payload already carries the body's \
         reference; minting a second strands it per abandoned park",
    );
}

/// Every value route in `instrs` — a `LoadLocal` feeding the next
/// instruction's `DecrefValueRegion` — as its slot, and whether a `StoreLocal`
/// stamps that slot within the two instructions after the release. The stamp
/// is `StoreLocal slot <nil>`, and materializing the nil takes an instruction
/// of its own, so the search looks past it.
fn stamped_value_routes(instrs: &[InstrRef<'_>]) -> Vec<(u16, bool)> {
    (0..instrs.len())
        .filter_map(|i| {
            let (InstrRef::LoadLocal { dst, slot }, InstrRef::DecrefValueRegion { src }) =
                (instrs.get(i)?, instrs.get(i + 1)?)
            else {
                return None;
            };
            if dst != src {
                return None;
            }
            let stamped = instrs[i + 2..]
                .iter()
                .take(2)
                .any(|n| matches!(n, InstrRef::StoreLocal { slot: back, .. } if back == slot));
            Some((*slot, stamped))
        })
        .collect()
}

#[test]
fn a_non_tail_dynamic_emit_payload_release_carries_its_receipt() {
    // The site's payload release is a recorded value route, not a bare pair
    // (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
    // still owes"). Two things make it one, and both matter where the signal
    // turns out to be TERMINAL: the nil stamp, so a restart's replay of the
    // continuation cannot run the release the walk already ran, and the table
    // entry, so a fiber nobody restarts reaches the release at all.
    //
    // Counterfactual — with the pair alone the same programs read correct on a
    // restart and strand one region per raise without one, which is what the
    // `emit-dyn-error-discard` probe measures.
    let module = compile_to_lir(
        "(let [s :yield] (fn () (let [x (string \"a\")] (fn () (begin (emit s x) 0)))))",
    );
    // Scoped to what follows the park in its own block — the site's own releases,
    // not some other route's elsewhere in the body, which stamps and records for
    // reasons of its own and would let this pass vacuously.
    let (func, released): (&LirOwned, Vec<(u16, bool)>) = std::iter::once(&module.entry)
        .chain(module.closures.iter())
        .find_map(|f| {
            let (b, at) = suspending_call(f)?;
            let after: Vec<InstrRef<'_>> = b.instrs().skip(at + 1).collect();
            Some((f, stamped_value_routes(&after)))
        })
        .expect("a function containing a SuspendingCall");
    assert!(
        !released.is_empty(),
        "the park's own block must carry the payload release for this to be about",
    );
    for (slot, stamped) in released {
        assert!(
            stamped,
            "the payload release at slot {slot} must stamp the slot it read, or a \
             restart's replay runs it a second time",
        );
        assert!(
            func.view().frame_release_slots().contains(&slot),
            "slot {slot} carries a stamped value route but is absent from \
             frame_release_slots, so an abandoned frame never runs it",
        );
    }
}

#[test]
fn a_borrowed_emit_payload_release_carries_its_receipt() {
    // The literal `Emit` twin of the dynamic site's receipt above
    // (docs/impl/region/unwind.md § "An abandoned frame runs the releases it
    // still owes"). The release is first in the resume block, and it must stamp
    // the slot it read and appear in the frame's release table.
    //
    // Counterfactual — with the pair alone, a park that an abort raises in place
    // over, or that a squelch boundary discards, loses the payload from the signal
    // slot the discharge would have released, and nothing else names the slot: one
    // region per park, which `tests/impl/region-fiber-yield-borrow-uaf.lisp`
    // gauges.
    let module = compile_to_lir("(fn () (let [x (string \"a\")] (fn () (emit :yield x))))");
    let (func, released): (&LirOwned, Vec<(u16, bool)>) = std::iter::once(&module.entry)
        .chain(module.closures.iter())
        .find_map(|f| {
            let (_, resume) = park_and_resume(f)?;
            let instrs: Vec<InstrRef<'_>> = resume.instrs().collect();
            Some((f, stamped_value_routes(&instrs)))
        })
        .expect("a function terminating in Emit");
    assert!(
        !released.is_empty(),
        "the resume block must carry the payload release for this to be about",
    );
    for (slot, stamped) in released {
        assert!(
            stamped,
            "the payload release at slot {slot} must stamp the slot it read, or the \
             walk runs it a second time after a restart's replay",
        );
        assert!(
            func.view().frame_release_slots().contains(&slot),
            "slot {slot} carries the park's payload release but is absent from \
             frame_release_slots, so a park an abort or a squelch boundary ends \
             never runs it",
        );
    }
}
