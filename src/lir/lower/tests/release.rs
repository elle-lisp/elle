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
    // result register (docs/impl/region-rules.md Rule 2, "discarded result") —
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
    // value's regions' decref_point to itself (docs/impl/region-rules.md Rule 4),
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

// ── Release order at a shared decref_point (docs/impl/region-rules.md Rule 4) ─────
//
// When several releases land on one decref_point, page-READING releases
// (`DecrefValueRegion` — loads a slot and derefs the value, unwrapping a
// capture cell) must be emitted before page-FREEING releases (`DecrefRegion`).
// The counterfactual is the capture-cell over-release UAF
// (region-capture-cell-noreassign-uaf.lisp): the cell's `DecrefRegion` frees
// the cell's pages, then the init's `DecrefValueRegion` unwraps the freed
// cell. The per-point order must not depend on `HashMap` iteration (random per
// instance); the loop runs many compiles so any nondeterministic unsafe
// ordering fails the test.

#[test]
fn release_order_value_gated_before_plain_in_shared_bucket() {
    // A plain `DecrefRegion` frees pages; a value-gated release
    // (`DecrefValueRegion`/`DecrefCellRegion`) reads them (it derefs the
    // loaded value — unwrapping the capture cell — to find its region). At a
    // shared decref_point every value-gated release must therefore be ordered
    // before every plain FREE. The bucket order must not depend on std
    // HashMap iteration (random per instance) — hence the rounds: one unsafe
    // permutation fails the test.
    //
    // Exception: a store-adopted member's plain `DecrefRegion` is an `Owned` no-op
    // (frees and reads nothing), so it is NOT a page-freeing release — it sorts ahead
    // of the value-gated readers on purpose (it must precede its owner's drop; see
    // `store_adopted_member_release_precedes_owner_in_shared_bucket`). So the
    // "value-gated before plain" invariant is over the plain releases that actually
    // FREE — store-adopted members excluded.
    for round in 0..16 {
        let (lowerer, _hir) = make_lowerer(CAPTURE_CELL_SHAPE);
        let info = &lowerer.region_info;
        let store_adopted: std::collections::HashSet<_> = info
            .owned_adopt_edges
            .values()
            .flatten()
            .map(|&(member, _owner)| member)
            .collect();
        let mut saw_mixed = false;
        for (point, regions) in &lowerer.decrefs_by_decref_point {
            // cell_release_regions ⊆ call_result_regions: membership in
            // call_result_regions is exactly "released value-gated". A store-adopted
            // member is an Owned no-op, not a genuine freer, so exclude it.
            let first_plain = regions
                .iter()
                .position(|r| !info.call_result_regions.contains(r) && !store_adopted.contains(r));
            let last_value_gated = regions
                .iter()
                .rposition(|r| info.call_result_regions.contains(r));
            if let (Some(fp), Some(lv)) = (first_plain, last_value_gated) {
                saw_mixed = true;
                assert!(
                    lv < fp,
                    "round {round}: decref point {point:?} orders a value-gated \
                     release after a plain DecrefRegion ({regions:?}) — the \
                     page-freeing release would tear the page the unwrap reads \
                     (the capture-cell over-release UAF)",
                );
            }
        }
        assert!(
            saw_mixed,
            "round {round}: expected the capture-cell shape to produce at \
             least one decref point holding both a value-gated and a plain \
             release — if region analysis changed, update CAPTURE_CELL_SHAPE \
             so this test keeps biting",
        );
    }
}

#[test]
fn region_analysis_is_deterministic_across_compiles() {
    // The region analysis (decref points, buckets, memberships) must be a
    // pure function of the source, modulo the process-global HirId counter.
    // The counterfactual: a single-pass binding-chain
    // override (hash-ordered, read-while-write) would resolve a random prefix of each
    // binding chain resolved per compile, yielding randomly-too-early
    // decref points — the flaky capture-cell over-release UAF. The fixpoint
    // iteration makes the result unique; this pins it.
    fn snapshot(src: &str) -> String {
        let (lowerer, _hir) = make_lowerer(src);
        let info = &lowerer.region_info;
        // HirIds come from a process-global counter shared across threads, so
        // absolute ids — and even gaps between them — jitter run to run under
        // the parallel test harness. Normalize each id to its RANK among the
        // ids this snapshot mentions: structure survives, jitter doesn't.
        let mut ids: Vec<u32> = info
            .region_data
            .values()
            .map(|d| d.decref_point.0)
            .chain(info.alloc_region.keys().map(|h| h.0))
            .chain(lowerer.decrefs_by_decref_point.keys().map(|h| h.0))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        let rank = |id: u32| ids.binary_search(&id).expect("id collected above") as u32;
        let mut rd: Vec<(u32, u32)> = info
            .region_data
            .iter()
            .map(|(r, d)| (r.0, rank(d.decref_point.0)))
            .collect();
        rd.sort();
        let mut ar: Vec<(u32, u32)> = info
            .alloc_region
            .iter()
            .map(|(h, r)| (rank(h.0), r.0))
            .collect();
        ar.sort();
        let mut cr: Vec<u32> = info.call_result_regions.iter().map(|r| r.0).collect();
        cr.sort();
        let mut buckets: Vec<(u32, Vec<u32>)> = lowerer
            .decrefs_by_decref_point
            .iter()
            .map(|(h, rs)| (rank(h.0), rs.iter().map(|r| r.0).collect()))
            .collect();
        buckets.sort();
        format!(
            "region_data: {rd:?}\nalloc_region: {ar:?}\ncall_result: {cr:?}\nbuckets: {buckets:?}\nxrefs: {:?}",
            info.cross_region_refs
        )
    }
    let first = snapshot(CAPTURE_CELL_SHAPE);
    for round in 0..8 {
        let again = snapshot(CAPTURE_CELL_SHAPE);
        assert_eq!(
            first, again,
            "round {round}: region analysis produced different results for \
             the same source — a hash-iteration order dependence",
        );
    }
}

#[test]
fn release_order_is_deterministic_across_compiles() {
    // Release order may never depend on hash-map iteration: the same source
    // must lower to the identical instruction stream on every compile
    // (docs/impl/region-rules.md Rule 4), up to the process-global static-region
    // counter (canonicalized away above). Two regions sharing a decref_point
    // are enough to expose a hash-ordered emission as a cross-compile diff.
    let first = canonicalize_static_regions(&format!("{:?}", compile_to_lir(CAPTURE_CELL_SHAPE)));
    for round in 0..8 {
        let again =
            canonicalize_static_regions(&format!("{:?}", compile_to_lir(CAPTURE_CELL_SHAPE)));
        assert_eq!(
            first, again,
            "round {round}: lowering the same source produced different \
             instruction streams — release order depends on hash iteration",
        );
    }
}

#[test]
fn preallocated_capture_cells_get_distinct_regions_each_released() {
    // docs/impl/region-model.md, "The per-execution region model": one allocation
    // execution per static slot between drops. `lower_begin` pre-allocates one
    // `MakeCaptureCell` per captured top-level binding; emitting two cells
    // against ONE slot orphans the first cell's physical region (the runtime
    // mints fresh per execution and overwrites the activation mapping, so the
    // slot's single `DecrefRegion` only ever releases the last cell) — the
    // shared-slot capture-cell leak
    // (tests/elle/region-capture-cell-shared-slot-leak.lisp).
    //
    // Shape: TWO captured bindings — `cap-a` (captured by `cap-b`'s inner
    // letrec lambda) and `cap-b` (captured by `cap-d`) — so the Begin pre-pass
    // emits two MakeCaptureCells. Assert each carries its own region slot and
    // each slot has a matching plain `DecrefRegion`.
    let module = compile_to_lir(
        "(begin \
           (def cap-a (fn () 1)) \
           (def cap-b (fn () (cap-a))) \
           (def cap-d (fn () (cap-b))) \
           nil)",
    );
    //
    // These `def`s live in a LOCAL clique (inside the stub letrec body, all discarded:
    // `cap-d ⊇ cap-b ⊇ cap-a`), so the ownership forest now reclaims them as a unit —
    // each cell is capture-adopted into its holding closure (`closure ⊇ cell`) and its
    // content adopted into it (`cell ⊇ content`) via `AdoptCellRegion`, and the outermost
    // closure's subtree drop frees the whole clique. An adopted cell's own decref is
    // therefore SUPPRESSED. So each cell region is released EITHER by its own
    // `DecrefRegion` (the Shared baseline) OR by adoption (an `AdoptCellRegion` links it
    // into a subtree) — never silently dropped, and never sharing a slot.
    fn collect(
        func: &LirFunction,
        cells: &mut Vec<StaticRegion>,
        decrefs: &mut Vec<StaticRegion>,
        adopt_cells: &mut usize,
    ) {
        for b in &func.blocks {
            for i in &b.instructions {
                match &i.instr {
                    LirInstr::MakeCaptureCell { region, .. } => cells.push(*region),
                    LirInstr::DecrefRegion { region_id } => decrefs.push(*region_id),
                    LirInstr::AdoptCellRegion { .. } => *adopt_cells += 1,
                    _ => {}
                }
            }
        }
    }
    let mut cells = Vec::new();
    let mut decrefs = Vec::new();
    let mut adopt_cells = 0usize;
    collect(&module.entry, &mut cells, &mut decrefs, &mut adopt_cells);
    for c in &module.closures {
        collect(c, &mut cells, &mut decrefs, &mut adopt_cells);
    }
    assert!(
        cells.len() >= 2,
        "expected the Begin pre-pass to emit two MakeCaptureCells (cap-a, cap-b); got {cells:?}",
    );
    for (i, a) in cells.iter().enumerate() {
        for b in cells.iter().skip(i + 1) {
            assert_ne!(
                a, b,
                "two MakeCaptureCells share one region slot — the runtime \
                 overwrites the slot's activation mapping per alloc, so the \
                 slot's single DecrefRegion frees only the last cell and every \
                 earlier cell's region leaks (cells={cells:?})",
            );
        }
    }
    // The clique is adopted (a local, non-escaping closure chain), so its cells reclaim
    // via `AdoptCellRegion` + the root's subtree drop rather than per-cell `DecrefRegion`s.
    assert!(
        adopt_cells > 0,
        "the local closure clique cap-d ⊇ cap-b ⊇ cap-a must reclaim by adoption \
         (an AdoptCellRegion links each cell into its holder's subtree); got none",
    );
    for cell in &cells {
        assert!(
            decrefs.contains(cell) || adopt_cells > 0,
            "MakeCaptureCell region {cell:?} is neither released by its own DecrefRegion \
             nor adopted into a subtree — its initial reference would leak \
             (decrefs={decrefs:?}, adopt_cells={adopt_cells})",
        );
    }
}

#[test]
fn store_adopted_member_release_precedes_owner_in_shared_bucket() {
    // A store-adopted member's own `DecrefRegion` is an `Owned` no-op only while the
    // member is still `Owned`, so it must be emitted BEFORE every release that can free
    // the member's owner. At a shared `decref_point` the intra-bucket order is what
    // enforces this (docs/impl/region-model.md § "The lifetime obligation the root
    // carries"). The counterfactual is the `%pair`-into-`@[]` double-free
    // (region-array-push-pair-loop-uaf.lisp): the container is a `Fresh` call-result
    // freed value-based (and, when its push result is discarded, freed a second time by
    // that pass-through result), and the pushed `%pair` is a plain-`DecrefRegion`
    // member sharing the container's `decref_point`. The pre-fix bucket sort
    // (readers-before-freers) placed the member's plain `DecrefRegion` LAST, so the
    // container's rc-zeroing release subtree-dropped the pair before its own decref —
    // which then faulted on the freed region. The members-first bucket class fixes it.
    //
    // Forces the intrinsic (`--checked-intrinsics=off`) path where the pushed pair is a
    // slot-resolved `DecrefRegion` member (checked-on it is a value-resolved `Fresh`
    // release, a different bucket class).
    let _ci = crate::config::test_override::ScopedCheckedIntrinsics::new(false);
    let (lowerer, _hir) = make_lowerer("(let [items @[]] (%array-push items (%pair 1 2)))");
    let has_adopt = !lowerer.region_info.owned_adopt_edges.is_empty();
    assert!(
        has_adopt,
        "expected `(%array-push items (%pair 1 2))` to produce a store-adopt edge \
         (owned_adopt_edges) on the checked-off path; got none — if intrinsic \
         classification changed, update the shape so this test keeps biting",
    );
    let mut saw_shared = false;
    for &(member, owner) in lowerer.region_info.owned_adopt_edges.values().flatten() {
        for regions in lowerer.decrefs_by_decref_point.values() {
            let mi = regions.iter().position(|r| *r == member);
            let oi = regions.iter().position(|r| *r == owner);
            if let (Some(mi), Some(oi)) = (mi, oi) {
                saw_shared = true;
                assert!(
                    mi < oi,
                    "store-adopted member r{} is released AFTER its owner r{} in a \
                     shared decref bucket ({regions:?}) — the owner's rc-zeroing \
                     release subtree-drops the member before its own (no-op) \
                     DecrefRegion fires, which then faults on the freed region \
                     (the %pair-into-@[] double-free)",
                    member.0,
                    owner.0,
                );
            }
        }
    }
    assert!(
        saw_shared,
        "expected the store-adopted member and its owner to share a decref_point \
         bucket (the coincident straight-line case the emit order must handle) — if \
         region analysis changed, update the shape so this test keeps biting",
    );
}

#[test]
fn letrec_init_release_fires_after_cell_store() {
    // A letrec init's region releases must be emitted AFTER the value is
    // stored into the binding's slot/cell, exactly as `lower_let` defers
    // them. The counterfactual is the shadowed-duplicate-definition UAF: a
    // captured binding with no surviving uses (its references resolve to a
    // later duplicate) keeps its closure region's `decref_point` at the init
    // node itself, so without the deferral the `DecrefRegion` lands between
    // `MakeClosure` and the cell store — the closure is freed before
    // `UpdateCapture` increfs it, the cell dangles, and the teardown scan
    // misattributes the reused pages (the stdlib-init phantom-decref panic;
    // stdlib defines `any?`/`all?` twice — same decref_point-at-init shape).
    //
    // The shape: `gg` is captured by the EARLIER lambda `ff` (forward ref), so
    // `gg`'s only use site is structurally before its own init — the
    // binding-chain extension cannot move its region's decref_point past the
    // init node, and only the deferral keeps the release after the store.
    let module = compile_to_lir(
        "(letrec [ff (fn () gg) \
                  gg (fn (x) x)] \
           1)",
    );
    fn check(func: &LirFunction) {
        for b in &func.blocks {
            // Track, per closure-producing register, the MakeClosure's
            // region; flag a plain DecrefRegion of that region appearing
            // before the register is consumed by a store.
            let mut pending: Vec<(Reg, StaticRegion)> = Vec::new();
            for (idx, i) in b.instructions.iter().enumerate() {
                match &i.instr {
                    LirInstr::MakeClosure { dst, region, .. } => {
                        pending.push((*dst, *region));
                    }
                    LirInstr::StoreCaptureCell { value, .. }
                    | LirInstr::StoreLocal { src: value, .. } => {
                        pending.retain(|(r, _)| r != value);
                    }
                    LirInstr::DecrefRegion { region_id } => {
                        assert!(
                            !pending.iter().any(|(_, reg)| reg == region_id),
                            "DecrefRegion({region_id:?}) at instr {idx} fires between a \
                             MakeClosure into that region and the store that consumes \
                             the closure — the value is freed before the cell's \
                             incref (shadowed-duplicate-definition UAF)",
                        );
                    }
                    _ => {}
                }
            }
        }
    }
    check(&module.entry);
    for c in &module.closures {
        check(c);
    }
}
