use super::*;

// ── Region-lifecycle: decref/release emission ────────────────────

#[test]
fn decref_region_emitted_for_one_alloc_let() {
    // Under unique-per-alloc the lowerer emits one `DecrefRegion`
    // per region at each region's `decref_point` HirId. The walk also
    // registers regions for `Let`/`Letrec`/`Begin`/`Match`/`Call`
    // nodes (for capture-cell and per-call bookkeeping), so the
    // total count is more than just the one user-visible allocation.
    // Assert there's at least one DecrefRegion — i.e. the new
    // emission path is wired (we'd see zero if `emit_decrefs_for`
    // weren't called).
    let module = compile_to_lir("(fn () (let [x (string \"a\")] x))");
    assert!(
        count_decref_regions(&module) >= 1,
        "expected at least one DecrefRegion to be emitted by emit_decrefs_for",
    );
}

#[test]
fn decref_region_emitted_for_emit_yield() {
    // `(fn () (let [x (string "a")] (emit :yield x)))` — the yielded
    // value's region is decref'd at the Emit's HirId (the value's
    // last use); the runtime incref in `handle_emit`
    // keeps the region alive past the matching DecrefRegion at the
    // resume site.
    let module = compile_to_lir("(fn () (let [x (string \"a\")] (emit :yield x)))");
    assert!(
        count_decref_regions(&module) >= 1,
        "expected at least one DecrefRegion for the emit-yielded value",
    );
}

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
    // (docs/impl/region/owner.md § "Park/unpark symmetry").
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
/// a block of its own (docs/impl/region/owner.md § "What yields is the emit
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
    // still owes"). Two things make it one, and both are load-bearing where the
    // signal turns out to be TERMINAL: the nil stamp, so a restart's replay of the
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

#[test]
fn release_emitted_for_unbound_call_result() {
    // An unbound Call result — `(f "a")` whose result flows
    // directly into Begin's discard position — must have a
    // DecrefValueRegion emitted at its decref_point. Without this,
    // the call's result region survives until fiber teardown
    // (linear leak in loops). `lower_call` allocates a release
    // slot for every Call so emit_decrefs_for can emit
    // `LoadLocal slot` + `DecrefValueRegion` uniformly for
    // both bound and unbound Calls.
    let module = compile_to_lir("(fn () (begin (f \"a\" \"b\") nil))");
    assert!(
        count_decref_value_regions(&module) >= 1,
        "expected at least one DecrefValueRegion for the unbound (f ...) result",
    );
}

#[test]
fn release_emitted_for_let_bound_call_result() {
    // Sanity check: the existing let-bound Call result path
    // also produces a DecrefValueRegion. This guards against
    // a regression where removing the redundant call_region_slot
    // recording in lower_let breaks the bound case.
    let module = compile_to_lir("(fn () (let [x (f \"a\" \"b\")] nil))");
    assert!(
        count_decref_value_regions(&module) >= 1,
        "expected at least one DecrefValueRegion for the let-bound (f ...) result",
    );
}

#[test]
fn release_emitted_for_discarded_let_tail_call_result() {
    // `(begin (let [x 1] (f "a")) nil)` — the discarded Let's body TAIL call.
    // ANF's propagating-tail wrap keys the slot recording on the outer
    // Let's id, not the tail Call's, so the call-result placeholder
    // reaches its decref_point (the Call node itself) with no slot bound.
    // The release must then be emitted by VALUE off the freshly-lowered
    // result register (docs/impl/region/rules.md Rule 2, "discarded result") —
    // before that rule the lowerer skipped it ("leak until fiber
    // teardown"): one leaked object per loop iteration, the
    // tests/elle/arena-count.lisp class.
    let module = compile_to_lir("(fn () (begin (let [x 1] (f \"a\")) nil))");
    assert!(
        count_decref_value_regions(&module) >= 1,
        "expected a DecrefValueRegion for the discarded let-tail (f ...) result",
    );
}

#[test]
fn named_param_release_follows_destructure_field_reads() {
    // `(fn [&named frame] 42)` compiles a prologue of
    // `(destructure {:frame frame} (var __named_param))`. The collected
    // keyword struct's region must be released AFTER the destructure's
    // field reads (`StructGetOrNil`) — the Destructure node extends the
    // value's regions' decref_point to itself (docs/impl/region/rules.md Rule 4),
    // exactly as Return extends a returned region. Pre-fix, with `frame`
    // unused, the struct's last USE was the inner Var, so the
    // `DecrefValueRegion` was emitted before the field read — a freed-page
    // read at runtime (tests/elle/region-named-param-uaf.lisp, the
    // lib/http2/stream.lisp import segv).
    let module = compile_to_lir("(fn [&named frame] 42)");
    let mut checked = false;
    for func in std::iter::once(&module.entry).chain(module.closures.iter()) {
        let instrs: Vec<&LirInstr> = func
            .blocks
            .iter()
            .flat_map(|b| b.instructions.iter())
            .map(|si| &si.instr)
            .collect();
        let last_get = instrs
            .iter()
            .rposition(|i| matches!(i, LirInstr::StructGetOrNil { .. }));
        let first_decref = instrs
            .iter()
            .position(|i| matches!(i, LirInstr::DecrefValueRegion { .. }));
        if let (Some(get), Some(dec)) = (last_get, first_decref) {
            checked = true;
            assert!(
                dec > get,
                "the &named collected struct's DecrefValueRegion (idx {dec}) must \
                 follow the destructure's StructGetOrNil field reads (last idx {get})"
            );
        }
    }
    assert!(
        checked,
        "expected a function with both StructGetOrNil and DecrefValueRegion \
         (the &named prologue)"
    );
}

#[test]
fn release_emitted_for_eval_result() {
    // `(fn () (begin (eval 1) nil))` — the Eval's result is
    // discarded. Eval's result region is a placeholder in the
    // outer compilation (the actual value lives in the inner
    // compilation's region). The regions walk registers Eval's
    // placeholder in `call_result_regions`, mirroring Call, and
    // `lower_eval` wraps the result with
    // `wrap_call_with_release_slot`. `emit_decrefs_for` then
    // emits `LoadLocal slot + DecrefValueRegion(expected)` at
    // the Eval's decref_point; the runtime gate skips the decref when
    // `region_of(value)` doesn't match the placeholder — safe by
    // construction.
    //
    // Without this wiring (pre-fix), the walk's `alloc_here` for
    // Eval's HirId would land in the else branch of
    // `emit_decrefs_for`, which emits raw `DecrefRegion(rid)` for
    // a region the runtime never allocated into — counter
    // underflow or conflation with a neighbouring region id.
    let module = compile_to_lir("(fn () (begin (eval 1) nil))");
    assert!(
        count_decref_value_regions(&module) >= 1,
        "expected at least one DecrefValueRegion for the (eval ...) result",
    );
}

#[test]
#[ignore = "region merging not yet implemented"]
fn decref_region_emitted_once_for_merged_pair() {
    // `(let [x (string "a") y (string "b")] (g x y))` has two
    // allocations with identical decref_point and no cross-region
    // edges, so the merge pass collapses them into one region.
    // The lowerer emits exactly one `DecrefRegion` for the
    // merged group.
    let module = compile_to_lir("(let [x (string \"a\") y (string \"b\")] (g x y))");
    assert_eq!(
        count_decref_regions(&module),
        1,
        "merged x and y should share one DecrefRegion",
    );
}

// The native-tail ReturnValue retain (the `IncrefValueRegion` the post-
// `TailCall` block emits on the native-completion fall-through) is guarded by
// Elle corpus tests, not a Rust LIR-structural assertion: the non-splice path
// by region-native-tail-return-uaf.lisp (a true UAF witness, RED before the
// fix under guardfree) and the splice/`apply` path by
// region-splice-tail-return.lisp (a correctness guard — the splice UAF is
// masked by the args-array leak, so it asserts the result value instead).

// ── The abandoned-frame release tables ───────────────────────────
//
// A frame abandoned by an error runs the releases it still owes, off the two
// tables the emitter records as it emits each route
// (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
// still owes"). The tables are only sound because they name exactly the routes
// the emitter WROTE — a route it declined has no entry and so can never be run
// twice — which is what these pin.

/// Every `(slot, DecrefValueRegion)` pair a function emits: the slot a
/// `LoadLocal` fed straight into the release. The "unbound call result" route
/// releases off a register no `LoadLocal` produced and is deliberately absent.
fn emitted_value_route_slots(func: &LirFunction) -> Vec<u16> {
    let mut out = Vec::new();
    for block in &func.blocks {
        let instrs: Vec<&LirInstr> = block.instructions.iter().map(|i| &i.instr).collect();
        for (i, instr) in instrs.iter().enumerate() {
            let LirInstr::DecrefValueRegion { src } = instr else {
                continue;
            };
            if let Some(LirInstr::LoadLocal { dst, slot }) = i.checked_sub(1).map(|p| instrs[p]) {
                if dst == src {
                    out.push(*slot);
                }
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Every static region slot a function's `DecrefRegion`s name.
fn emitted_slot_route_regions(func: &LirFunction) -> Vec<u32> {
    let mut out: Vec<u32> = func
        .blocks
        .iter()
        .flat_map(|b| b.instructions.iter())
        .filter_map(|i| match &i.instr {
            LirInstr::DecrefRegion { region_id } => Some(region_id.get()),
            _ => None,
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

#[test]
fn frame_release_tables_name_exactly_the_routes_emitted() {
    // The walk's whole premise: a table entry IS a release the function emits,
    // so running it at an abandoned exit runs that instruction and no other.
    // Counterfactual — a table built from the region set rather than from the
    // emit site would carry the routes the emitter declined (a mutated slot, a
    // cell box, a transfer adopt) and release a reference nobody owes.
    let module = compile_to_lir("(let [x (string \"a\") y (string \"b\")] (g x y))");
    for func in std::iter::once(&module.entry).chain(module.closures.iter()) {
        let mut recorded = func.frame_release_slots.clone();
        recorded.sort_unstable();
        assert_eq!(
            recorded,
            emitted_value_route_slots(func),
            "frame_release_slots must be exactly the slots a value route loaded from",
        );
        let mut regions: Vec<u32> = func.frame_release_regions.iter().map(|r| r.get()).collect();
        regions.sort_unstable();
        assert_eq!(
            regions,
            emitted_slot_route_regions(func),
            "frame_release_regions must be exactly the slots a DecrefRegion named",
        );
    }
}

#[test]
fn a_reassigned_binding_records_no_value_route() {
    // A reassigned binding's slot is not a release route at all — its occupant
    // at the release point is whatever was stored last — so `emit_decref_for_region`
    // skips it (docs/impl/region/bindings.md § "a mutated slot is not a release
    // route"). The table is written where the route is EMITTED, so the skip
    // carries into it and the walk can never load that slot.
    let module = compile_to_lir("(begin (var x (string \"a\")) (assign x (string \"b\")) x)");
    // The shape has a reassigned binding holding heap values, so there IS a
    // release to skip; without this the equality below could hold vacuously.
    assert!(
        count_decref_regions(&module) >= 1,
        "the shape must carry a release for the skip to be about",
    );
    for func in std::iter::once(&module.entry).chain(module.closures.iter()) {
        let mut recorded = func.frame_release_slots.clone();
        recorded.sort_unstable();
        assert_eq!(
            recorded,
            emitted_value_route_slots(func),
            "a skipped route must leave no entry behind",
        );
    }
}
