// audited: 2026-09-19
// That the abandoned-frame release tables name exactly the routes the emitter
// wrote, and nothing it declined.
//
// docs/impl/region/mechanism.md

use super::*;

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

/// Every splice args-array slot a function's call instructions carry. Their
/// release is the runtime's rather than an emitted `DecrefRegion`, so the frame
/// still owes it while the array exists and the call has not run.
fn splice_args_regions(func: &LirFunction) -> Vec<u32> {
    let mut out: Vec<u32> = func
        .blocks
        .iter()
        .flat_map(|b| b.instructions.iter())
        .filter_map(|i| match &i.instr {
            LirInstr::CallArrayMut { args_region, .. }
            | LirInstr::TailCallArrayMut { args_region, .. } => Some(args_region.get()),
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
fn a_splice_args_array_is_owed_by_the_frame_until_the_call_takes_it() {
    // The second contributor to the region table: a spliced call's args array has
    // no binding, so no `DecrefRegion` names it and the call reclaims it at
    // runtime instead (docs/impl/region/mechanism.md § "A spliced call's arguments
    // come out of an array the convention owns"). Between the array's
    // construction and the call the frame still owes that release — an
    // `ArrayMutExtend` over a non-sequence raises exactly there — so the slot is
    // in the table, and the call's own take is what keeps it from running twice.
    let module = compile_to_lir("(fn (xs) (let [n (g ;xs)] n))");
    let func = std::iter::once(&module.entry)
        .chain(module.closures.iter())
        .find(|f| !splice_args_regions(f).is_empty())
        .expect("a function lowering a spliced call");
    let mut regions: Vec<u32> = func.frame_release_regions.iter().map(|r| r.get()).collect();
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
    // (docs/impl/region/model.md § "The per-execution region model"). The args
    // array and the call are two, so sharing the call's slot orphans the array's
    // physical region the moment the call maps its own mint over it — the leak
    // `tests/elle/region-splice-args.lisp` gauges. Counterfactual: with one slot
    // for both, this assertion reads them equal.
    let module = compile_to_lir("(fn (xs) (g ;xs))");
    let func = std::iter::once(&module.entry)
        .chain(module.closures.iter())
        .find(|f| !splice_args_regions(f).is_empty())
        .expect("a function lowering a spliced call");
    let array_slots: Vec<u32> = func
        .blocks
        .iter()
        .flat_map(|b| b.instructions.iter())
        .filter_map(|i| match &i.instr {
            LirInstr::MakeArrayMut { region, .. } => Some(region.get()),
            _ => None,
        })
        .collect();
    let call_slots: Vec<u32> =
        func.blocks
            .iter()
            .flat_map(|b| b.instructions.iter())
            .filter_map(|i| match &i.instr {
                LirInstr::CallArrayMut { region, .. }
                | LirInstr::TailCallArrayMut { region, .. } => Some(region.get()),
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
