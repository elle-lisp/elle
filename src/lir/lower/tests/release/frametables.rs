// audited: 2026-11-12
//! That the abandoned-frame release tables name exactly the routes the emitter wrote, and nothing it declined.
//!
//! docs/impl/region/unwind.md

use super::*;

/// Every `(slot, DecrefValueRegion)` pair a function emits: the slot a
/// `LoadLocal` fed straight into the release. The "unbound call result" route
/// releases off a register no `LoadLocal` produced and is deliberately absent.
fn emitted_value_route_slots(func: &LirOwned) -> Vec<u16> {
    let mut out = Vec::new();
    for block in func.view().blocks() {
        let instrs: Vec<InstrRef<'_>> = block.instrs().collect();
        for (i, instr) in instrs.iter().enumerate() {
            let InstrRef::DecrefValueRegion { src } = instr else {
                continue;
            };
            if let Some(InstrRef::LoadLocal { dst, slot }) = i.checked_sub(1).map(|p| instrs[p]) {
                if dst == *src {
                    out.push(slot);
                }
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Every static region slot a function's `DecrefRegion`s name.
fn emitted_slot_route_regions(func: &LirOwned) -> Vec<u32> {
    let mut out: Vec<u32> = flat_instrs(func)
        .into_iter()
        .filter_map(|i| match i {
            InstrRef::DecrefRegion { region_id } => Some(region_id.get()),
            _ => None,
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Every splice args-array slot a function's call instructions carry. Their
/// release is the runtime's rather than an emitted `DecrefRegion`, so the frame
/// still owes it while the array exists and the call has not run.
fn splice_args_regions(func: &LirOwned) -> Vec<u32> {
    let mut out: Vec<u32> = flat_instrs(func)
        .into_iter()
        .filter_map(|i| match i {
            InstrRef::CallArrayMut { args_region, .. }
            | InstrRef::TailCallArrayMut { args_region, .. } => Some(args_region.get()),
            _ => None,
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

#[test]
fn frame_release_tables_name_exactly_the_routes_emitted() {
    // The walk's whole premise: a table entry IS a release the frame still owes,
    // so running it at an abandoned exit runs that release and no other.
    // Counterfactual — a table built from the region set rather than from the
    // emit site carries the routes the emitter declined. A mutated slot, a cell
    // box and a transfer adopt are such routes, and each releases a reference
    // nobody owes.
    let module = compile_to_lir("(let [x (string \"a\") y (string \"b\")] (g x y))");
    for func in std::iter::once(&module.entry).chain(module.closures.iter()) {
        let mut recorded = func.view().frame_release_slots().to_vec();
        recorded.sort_unstable();
        assert_eq!(
            recorded,
            emitted_value_route_slots(func),
            "frame_release_slots must be exactly the slots a value route loaded from",
        );
        let mut regions: Vec<u32> = func
            .view()
            .frame_release_regions()
            .iter()
            .map(|r| r.get())
            .collect();
        regions.sort_unstable();
        assert_eq!(
            regions,
            emitted_slot_route_regions(func),
            "frame_release_regions must be exactly the slots a DecrefRegion named",
        );
    }
}

#[test]
fn a_borrowed_tail_argument_release_carries_its_receipt() {
    // A tail call that suspends leaves its borrowed-argument retain standing, so
    // the release past the `TailCall` is the retain's one consumer
    // (docs/impl/region/signalexit.md). A park nobody resumes reaches that release
    // only through the table, and the nil stamp keeps a walk after a replay from
    // running it a second time.
    //
    // Counterfactual — the bare `LoadLocal`/`DecrefValueRegion` pair. It reads
    // correct on a resume, and strands the argument's region at every park an
    // abort or a drop ends.
    for (label, src) in [
        (
            "a captured upvalue",
            "(begin (def f (fn (x) (let [g (fn () (length x))] (g)))) (f (list 1 2)))",
        ),
        (
            "a destructured leaf",
            "(begin (def f (fn (t) (let [[a b] t] (length b)))) (f [(list 1) (list 2)]))",
        ),
    ] {
        let module = compile_to_lir(src);
        let mut named = 0;
        for func in functions(&module) {
            for (b, at) in tail_call_blocks_of(func) {
                let instrs: Vec<InstrRef<'_>> = b.instrs().collect();
                let InstrRef::TailCall {
                    borrowed_arg_slots, ..
                } = instrs[at]
                else {
                    unreachable!("tail_call_blocks_of names a TailCall's index");
                };
                let routes = stamped_value_routes(&instrs[at + 1..]);
                for slot in borrowed_arg_slots.iter() {
                    named += 1;
                    assert!(
                        routes.contains(&(slot, true)),
                        "{label}: the release of the retain stashed at slot {slot} \
                         must stamp the slot it read (routes={routes:?})",
                    );
                    assert!(
                        func.view().frame_release_slots().contains(&slot),
                        "{label}: the retain stashed at slot {slot} is absent from \
                         frame_release_slots, so a park nobody resumes strands it",
                    );
                }
            }
        }
        assert!(
            named >= 1,
            "{label}: the shape must name a borrowed tail argument for this to be about",
        );
    }
}

#[test]
fn a_splice_args_array_is_owed_by_the_frame_until_the_call_takes_it() {
    // The second contributor to the region table: a spliced call's args array has
    // no binding, so no `DecrefRegion` names it and the call reclaims it at
    // runtime instead (docs/impl/region/mechanism.md). Between the array's
    // construction and the call the frame still owes that release. An
    // `ArrayMutExtend` over a non-sequence raises exactly there. So the slot is
    // in the table, and the call's own take keeps it from running twice.
    let module = compile_to_lir("(fn (xs) (let [n (g ;xs)] n))");
    let func = std::iter::once(&module.entry)
        .chain(module.closures.iter())
        .find(|f| !splice_args_regions(f).is_empty())
        .expect("a function lowering a spliced call");
    let mut regions: Vec<u32> = func
        .view()
        .frame_release_regions()
        .iter()
        .map(|r| r.get())
        .collect();
    regions.sort_unstable();
    for slot in splice_args_regions(func) {
        assert!(
            regions.contains(&slot),
            "the splice args slot {slot} must be a release the abandoned frame owes",
        );
        assert!(
            !emitted_slot_route_regions(func).contains(&slot),
            "the splice args slot {slot} must have no emitted DecrefRegion — the call \
             takes it, and a second release would over-free",
        );
    }
}

#[test]
fn a_spliced_call_allocates_its_args_array_outside_the_call_region() {
    // A static region slot names ONE allocation execution between drops
    // (docs/impl/region/model.md). The args array and the call are two, so
    // sharing the call's slot orphans the array's physical region the moment the
    // call maps its own mint over it — the leak `tests/impl/region-splice-args.lisp`
    // gauges. Counterfactual: with one slot for both, this assertion reads them
    // equal.
    let module = compile_to_lir("(fn (xs) (g ;xs))");
    let func = std::iter::once(&module.entry)
        .chain(module.closures.iter())
        .find(|f| !splice_args_regions(f).is_empty())
        .expect("a function lowering a spliced call");
    let array_slots: Vec<u32> = flat_instrs(func)
        .into_iter()
        .filter_map(|i| match i {
            InstrRef::MakeArrayMut { region, .. } => Some(region.get()),
            _ => None,
        })
        .collect();
    let call_slots: Vec<u32> = flat_instrs(func)
        .into_iter()
        .filter_map(|i| match i {
            InstrRef::CallArrayMut { region, .. } | InstrRef::TailCallArrayMut { region, .. } => {
                Some(region.get())
            }
            _ => None,
        })
        .collect();
    assert!(!array_slots.is_empty(), "the splice path builds an @array");
    for slot in &array_slots {
        assert!(
            !call_slots.contains(slot),
            "the args array's slot {slot} must not be the call's own",
        );
        assert!(
            splice_args_regions(func).contains(slot),
            "the args array's slot {slot} must be the one the call carries",
        );
    }
}

#[test]
fn a_reassigned_binding_records_no_value_route() {
    // A reassigned binding's slot is not a release route at all — its occupant at
    // the release point is whatever was stored last — so `emit_decref_for_region`
    // skips it (docs/impl/region/bindings.md). The table is written where the route
    // is EMITTED, so the skip carries into it and the walk can never load that slot.
    let module = compile_to_lir("(begin (var x (string \"a\")) (assign x (string \"b\")) x)");
    // The shape has a reassigned binding holding heap values, so there IS a
    // release to skip; without this the equality below could hold vacuously.
    assert!(
        count_decref_regions(&module) >= 1,
        "the shape must carry a release for the skip to be about",
    );
    for func in std::iter::once(&module.entry).chain(module.closures.iter()) {
        let mut recorded = func.view().frame_release_slots().to_vec();
        recorded.sort_unstable();
        assert_eq!(
            recorded,
            emitted_value_route_slots(func),
            "a skipped route must leave no entry behind",
        );
    }
}
