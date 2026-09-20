// audited: 2026-09-19
// The reference a park mints for a payload the emitting body borrows, and the
// receipt the non-tail dynamic emit's release carries.
//
// docs/impl/region/park.md

use super::*;

/// The `(retains, releases)` a module's park is wrapped in — retains in the block
/// the `Emit` terminates, releases in the resume block it jumps to. Counted per
/// block rather than per function so an unrelated mint elsewhere in the body (a
/// `Return`'s, say) cannot stand in for the park's own pair.
fn park_borrow_ops(module: &crate::lir::LirModule) -> (usize, usize) {
    fn in_func(func: &LirFunction) -> Option<(usize, usize)> {
        let (park, resume_label) =
            func.blocks
                .iter()
                .find_map(|b| match b.terminator.terminator {
                    crate::lir::Terminator::Emit { resume_label, .. } => Some((b, resume_label)),
                    _ => None,
                })?;
        let resume = func.blocks.iter().find(|b| b.label == resume_label)?;
        let count = |b: &crate::lir::BasicBlock, f: fn(&LirInstr) -> bool| {
            b.instructions.iter().filter(|i| f(&i.instr)).count()
        };
        Some((
            count(park, |i| matches!(i, LirInstr::IncrefValueRegion { .. })),
            count(resume, |i| matches!(i, LirInstr::DecrefValueRegion { .. })),
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
fn dynamic_park_borrow_ops(module: &crate::lir::LirModule) -> (usize, usize) {
    fn in_func(func: &LirFunction) -> Option<(usize, usize)> {
        for b in &func.blocks {
            let Some(at) = b
                .instructions
                .iter()
                .position(|i| matches!(i.instr, LirInstr::SuspendingCall { .. }))
            else {
                continue;
            };
            let count = |r: &[crate::lir::types::SpannedInstr], f: fn(&LirInstr) -> bool| {
                r.iter().filter(|i| f(&i.instr)).count()
            };
            return Some((
                count(&b.instructions[..at], |i| {
                    matches!(i, LirInstr::IncrefValueRegion { .. })
                }),
                count(&b.instructions[at + 1..], |i| {
                    matches!(i, LirInstr::DecrefValueRegion { .. })
                }),
            ));
        }
        None
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
    let (func, released): (&LirFunction, Vec<(u16, bool)>) =
        std::iter::once(&module.entry)
            .chain(module.closures.iter())
            .find_map(|f| {
                let b = f.blocks.iter().find(|b| {
                    b.instructions
                        .iter()
                        .any(|i| matches!(i.instr, LirInstr::SuspendingCall { .. }))
                })?;
                let at = b
                    .instructions
                    .iter()
                    .position(|i| matches!(i.instr, LirInstr::SuspendingCall { .. }))?;
                let instrs: Vec<&LirInstr> =
                    b.instructions[at + 1..].iter().map(|i| &i.instr).collect();
                Some((
                    f,
                    (0..instrs.len())
                        .filter_map(|i| {
                            let (
                                LirInstr::LoadLocal { dst, slot },
                                LirInstr::DecrefValueRegion { src },
                            ) = (instrs.get(i)?, instrs.get(i + 1)?)
                            else {
                                return None;
                            };
                            if dst != src {
                                return None;
                            }
                            // The stamp is `StoreLocal slot <nil>`, and
                            // materializing the nil takes an instruction of its
                            // own, so look past it.
                            let stamped = instrs[i + 2..].iter().take(2).any(
                                |n| matches!(n, LirInstr::StoreLocal { slot: back, .. } if back == slot),
                            );
                            Some((*slot, stamped))
                        })
                        .collect(),
                ))
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
            func.frame_release_slots.contains(&slot),
            "slot {slot} carries a stamped value route but is absent from \
             frame_release_slots, so an abandoned frame never runs it",
        );
    }
}
