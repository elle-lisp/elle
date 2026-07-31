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

// ── Release order at a shared decref_point (docs/impl/region/rules.md Rule 4) ─────
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
    // (docs/impl/region/rules.md Rule 4), up to the process-global static-region
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
    // docs/impl/region/model.md, "The per-execution region model": one allocation
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
    // enforces this (docs/impl/region/adopt.md § "The lifetime obligation the root
    // carries"). The counterfactual is the `%pair`-into-`@[]` double-free
    // (region-array-push-pair-loop-uaf.lisp): the container is a `Fresh` call-result
    // freed value-based (and, when its push result is discarded, freed a second time by
    // that pass-through result), and the pushed `%pair` is a plain-`DecrefRegion`
    // member sharing the container's `decref_point`. Order the member's plain
    // `DecrefRegion` after the container's rc-zeroing release and the container's subtree
    // drop reclaims the pair before its own decref — which then faults on the freed
    // region. The topological order over the adopt edge (member → owner) keeps the
    // member first.
    //
    // `%pair` is an inline intrinsic, so the pushed pair is a slot-resolved
    // `DecrefRegion` member; the `%array-push` funnel call's recovered containment
    // supplies the store-adopt edge.
    let (lowerer, _hir) = make_lowerer("(let [items @[]] (%array-push items (%pair 1 2)))");
    let has_adopt = !lowerer.region_info.owned_adopt_edges.is_empty();
    assert!(
        has_adopt,
        "expected `(%array-push items (%pair 1 2))` to produce a store-adopt edge \
         (owned_adopt_edges); got none — if intrinsic classification changed, update \
         the shape so this test keeps biting",
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
fn container_read_alias_release_precedes_container_in_shared_bucket() {
    // A container element READ hands back a value that still lives inside the container,
    // and its release is value-resolved: `DecrefValueRegion` reads the value's own page
    // to find the runtime region. The borrowing-read lifetime pin (region/rules.md Rule 4)
    // extends the container's release to the reader, which lands both releases on ONE
    // `decref_point` — so the intra-bucket order is what keeps the reader's page-reading
    // release ahead of the container's demise. Inverted, the container's release frees (or
    // subtree-drops) the page the alias's decref then reads — the subtree-drop face of
    // `region_container_read_borrow_uaf`.
    //
    // The alias → container edges ride `counted_read_aliases` into the same topological
    // sort as the adopt edges; the id-only tie-break cannot be relied on here (the alias is
    // minted after its container, so it sorts LAST among equal-class regions). The shape
    // hands the read's result AND the container to one consumer, which is what lands both
    // releases on that consumer's node — the coincident case where only the order decides.
    let (lowerer, hir) = make_lowerer(
        "(let [c (@array) r (string \"s\")] \
           (begin (%array-push c r) ((fn [a b] 1) (get c 0) c)))",
    );
    let info = &lowerer.region_info;
    let mut saw_shared = false;
    for &(_site, alias, container) in &info.counted_read_aliases {
        for regions in lowerer.decrefs_by_decref_point.values() {
            let ai = regions.iter().position(|r| *r == alias);
            let ci = regions.iter().position(|r| *r == container);
            if let (Some(ai), Some(ci)) = (ai, ci) {
                saw_shared = true;
                assert!(
                    ai < ci,
                    "the read alias r{} is released AFTER the container r{} it reads \
                     out of, in a shared decref bucket ({regions:?}) — the container's \
                     release tears the page the alias's DecrefValueRegion then reads",
                    alias.0,
                    container.0,
                );
            }
        }
    }
    assert!(
        saw_shared,
        "expected the read alias and its container to share a decref_point bucket (the \
         coincident case the borrow pin creates) — if region analysis changed, update the \
         shape so this test keeps biting (hir @{})",
        hir.id.0,
    );
}

#[test]
fn nested_adopt_members_release_innermost_first() {
    // A store/capture-adopted member's own `DecrefRegion` is an `Owned` no-op only
    // while the member is still `Owned`; once its owner's subtree drop reclaims it,
    // that decref faults. So at a shared `decref_point` a member must be released
    // before its owner (store_adopted_member_release_precedes_owner_in_shared_bucket).
    // With NESTED adoption — inner ⊂ mid ⊂ root all sharing one point — the constraint
    // is transitive: innermost first. A single flat priority key cannot express this: it
    // would put inner AND mid in one members class tie-broken by region id, so a mid whose
    // id is smaller than its own member sorts BEFORE it — the member's decref then faults
    // on the region mid's drop already reclaimed. Only a topological sort over the adopt
    // edges (region/rules.md Rule 4) orders a chain by construction.
    //
    // Injected via the RegionInfo seam because inference does not mint a 3-deep adopt
    // chain for any small shape. Region ids are chosen to CONTRADICT containment (root
    // SMALLEST), so an id-only tie-break inverts the order and this assert bites.
    let inner = crate::hir::region::Region(9003);
    let mid = crate::hir::region::Region(9002);
    let root = crate::hir::region::Region(9001);
    let point = crate::hir::HirId(9_000_001);
    let site = crate::hir::HirId(9_000_002);
    let (lowerer, _hir) = make_lowerer_with("42", |info, _hir| {
        for r in [inner, mid, root] {
            info.region_data
                .insert(r, crate::hir::region::RegionData::at(point));
        }
        info.owned_adopt_edges
            .insert(site, vec![(inner, mid), (mid, root)]);
    });
    let bucket = lowerer
        .decrefs_by_decref_point
        .get(&point)
        .expect("the injected shared decref_point");
    let pos = |r| {
        bucket
            .iter()
            .position(|x| *x == r)
            .expect("region in bucket")
    };
    assert!(
        pos(inner) < pos(mid) && pos(mid) < pos(root),
        "nested adopt members must release innermost-first (inner ⊂ mid ⊂ root); got {:?}",
        bucket.iter().map(|r| r.0).collect::<Vec<_>>(),
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

// ── The frame-exit release ───────────────────────────────────────
// Everything the lowerer emits after a `TailCall` runs only on the NATIVE
// fall-through — a native pushes no bytecode frame and the dispatch loop
// continues into that block, while a closure callee replaces the frame and never
// arrives. For the call's own arguments that is the ownership move; for anything
// else it strands the frame's reference, so the release is carried back ahead of
// the `TailCall` (docs/impl/region/mechanism.md § "A release past a
// frame-replacing tail call is not a release"). These pin the PLACEMENT: the
// counts are unchanged either way, so only position can tell the two apart.

/// Position of the first `TailCall` in the function that contains one, with the
/// indices of that block's `DecrefValueRegion`s. `None` if no block has a
/// `TailCall`.
fn tail_call_release_layout(module: &crate::lir::LirModule) -> Option<(usize, Vec<usize>)> {
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
            let releases = b
                .instructions
                .iter()
                .enumerate()
                .filter(|(_, i)| matches!(i.instr, LirInstr::DecrefValueRegion { .. }))
                .map(|(idx, _)| idx)
                .collect();
            return Some((at, releases));
        }
    }
    None
}

#[test]
fn stranded_param_release_precedes_the_frame_replacing_tail_call() {
    // `x` is used nowhere, so its release is the unused-parameter fallback the
    // lowerer emits at the end of the body — the dead block. It must be carried
    // back ahead of the `TailCall`, or the moved-in argument is stranded once per
    // call (the `tail-frame-exit-unused` probe).
    let module = compile_to_lir("(begin (def s (fn () 0)) (def f (fn (x) (s))) (f (list 1 2)))");
    let (at, releases) = tail_call_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        releases.iter().any(|&r| r < at),
        "the unused parameter's release is still emitted after the TailCall \
         (at={at}, releases={releases:?}) — dead on the closure path",
    );
}

#[test]
fn moved_argument_release_stays_after_the_tail_call() {
    // The exemption, and the over-free face of the same placement: `x` IS the
    // tail call's argument, so its never-executed release is the transfer the
    // callee's owned-param release consumes. Hoisting it would drop the
    // reference the callee now owns.
    let module = compile_to_lir("(begin (def s (fn (a) a)) (def f (fn (x) (s x))) (f (list 1 2)))");
    let (at, releases) = tail_call_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        !releases.is_empty() && releases.iter().all(|&r| r > at),
        "a moved argument's release was hoisted ahead of the TailCall \
         (at={at}, releases={releases:?}) — that release IS the ownership move",
    );
}

#[test]
fn captured_param_release_precedes_the_frame_replacing_tail_call() {
    // The tail callee reaches `x` through its CAPTURED environment, which no
    // argument names — and the release is hoisted anyway, because building the
    // env took a counted reference through the allocation funnel, so the frame's
    // own is still the only one this drops (docs/impl/region/mechanism.md §
    // "Lexical capture is not a second holder to fear"; the
    // `tail-frame-exit-captured` probe).
    let module =
        compile_to_lir("(begin (def f (fn (x) (let [g (fn () (%int? x))] (g)))) (f (list 1 2)))");
    let (at, releases) = tail_call_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        releases.iter().any(|&r| r < at),
        "the captured parameter's release is still emitted after the TailCall \
         (at={at}, releases={releases:?}) — dead on the closure path",
    );
}

#[test]
fn capture_handed_back_by_the_callee_precedes_the_tail_call() {
    // The tail callee hands `x` BACK, so the caller's owning reference is minted
    // by the CALLEE's `Return`, after this release runs. The release is hoisted
    // anyway, because the same capture that lets `g` read `x` is a counted edge
    // that outlives the mint — it falls away only with `g`'s region, at the
    // callee's completion (docs/impl/region/mechanism.md § "The callee's return
    // mint, and the edge that funds the gap"; the `tail-frame-exit-handback`
    // probe). This is the stdlib walker's accumulator.
    let module = compile_to_lir("(begin (def f (fn (x) (let [g (fn () x)] (g)))) (f (list 1 2)))");
    let (at, releases) = tail_call_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        releases.iter().any(|&r| r < at),
        "the handed-back capture's release is still emitted after the TailCall \
         (at={at}, releases={releases:?}) — dead on the closure path",
    );
}

#[test]
fn handback_the_callee_does_not_capture_stays_after_the_tail_call() {
    // The decline face, and the residual the admission leaves. `x` reaches a
    // return through the OTHER arm, so it is on the return frontier — but the arm
    // that leaves through a frame-replacing callee calls one that captures
    // nothing, so no counted edge stands at that point to span the gap to a mint.
    // The release keeps its place in the dead block: a leak, never an over-free.
    let module = compile_to_lir(
        "(begin (def s (fn () (list 3))) (def f (fn (x c) (if c x (s)))) (f (list 1 2) false))",
    );
    let (at, releases) = tail_call_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        releases.iter().all(|&r| r > at),
        "a release was hoisted at a point whose callee captures nothing \
         (at={at}, releases={releases:?}) — no edge funds the caller's mint",
    );
}

/// Position of the first `TailCall` in the function that contains one, with the
/// indices of that block's `DecrefRegion`s — the slot-resolved twin of
/// [`tail_call_release_layout`], which reads the value route. A self-recursive
/// closure's region is released by id, so only this reading sees it.
fn tail_call_region_release_layout(module: &crate::lir::LirModule) -> Option<(usize, Vec<usize>)> {
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
            let releases = b
                .instructions
                .iter()
                .enumerate()
                .filter(|(_, i)| matches!(i.instr, LirInstr::DecrefRegion { .. }))
                .map(|(idx, _)| idx)
                .collect();
            return Some((at, releases));
        }
    }
    None
}

#[test]
fn region_an_argument_only_called_is_released_before_the_tail_call() {
    // The exemption reads an operand's VALUE, not its syntax
    // (docs/impl/region/mechanism.md § "What an operand names is its VALUE, not its
    // syntax"). `go` is named nowhere in the tail call — its ARGUMENT calls `go`,
    // so what `helper` is handed is that call's RESULT, and `go`'s own closure
    // region was read and finished with beforehand. Its release sits at the
    // letrec's scope end, past the `TailCall`, and must be carried back.
    let module = compile_to_lir(
        "(begin (def f (fn (n) \
         (letrec [helper (fn (x) (%sub x 1)) \
                   go (fn (m) (if (%lt m 1) 0 (go (%sub m 1))))] \
           (helper (go n))))) (f 3))",
    );
    let (at, releases) =
        tail_call_region_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        releases.iter().any(|&r| r < at),
        "the region an argument's own call named is still released after the \
         TailCall (at={at}, releases={releases:?}) — dead on the closure path",
    );
}

#[test]
fn container_of_an_opcode_read_argument_stays_after_the_tail_call() {
    // The over-free face of the same reading. An inline `%`-opcode mints no region
    // of its own, so `(%first v)` hands the callee a borrow living IN `v`'s region —
    // which is why Rule 4 extends `v`'s own release to the reader, landing it in the
    // dead block. The descent passes THROUGH the opcode to `v`, so `v` stays exempt;
    // hoisting its release would free the pair the callee is handed.
    let module = compile_to_lir(
        "(begin (def p (fn (s) (%add 1 (length s)))) \
         (def q (fn (n) (let [v (%pair (string \"ab\" n) nil)] (p (%first v))))) \
         (q 1))",
    );
    // Named by the pair's OWN slot: the block legitimately releases other regions
    // ahead of the call (the materialized string's), so "some release precedes it"
    // says nothing about which.
    let (at, releases) = tail_call_slot_release_layout(&module, |i| match i {
        LirInstr::List { region, .. } => Some(*region),
        _ => None,
    })
    .expect("the body lowers to a TailCall over a cons cell");
    assert!(
        releases.iter().all(|&r| r > at),
        "the container of an opcode read's borrow was hoisted ahead of the \
         TailCall (at={at}, releases={releases:?}) — the moved value lives in it",
    );
}

/// Every `TailCall`'s `defer_callee_release` flag across the module, in emission
/// order. Reading the flag rather than a release position is what makes the
/// deferral pins specific: the release this channel supplies is emitted by the
/// RUNTIME at the callee's completion, so no instruction in the caller records it.
fn tail_call_deferrals(module: &crate::lir::LirModule) -> Vec<bool> {
    let funcs = std::iter::once(&module.entry).chain(module.closures.iter());
    funcs
        .flat_map(|f| f.blocks.iter())
        .flat_map(|b| b.instructions.iter())
        .filter_map(|i| match &i.instr {
            LirInstr::TailCall {
                defer_callee_release,
                ..
            } => Some(*defer_callee_release),
            _ => None,
        })
        .collect()
}

#[test]
fn a_letrec_member_the_body_tail_calls_defers_its_own_release() {
    // `helper` is captured by `go`, so it is allocated per call and its uses span
    // the whole letrec — which puts its demise at the letrec's SCOPE END, not at
    // the call node the dies-here reading looks at. The relocation must leave that
    // release alone (the call is about to enter the closure it would free), so the
    // exemption's premise that the new activation takes it over holds only if this
    // channel runs it (docs/impl/region/mechanism.md § "What the exemption keeps, a
    // channel must still run"; the `tail-frame-exit-callee-member` probe).
    //
    // EXACTLY one deferral is the other half of the pin. `go`'s own body tail-calls
    // `helper` too, and a second deferral there would drop the frame's one
    // reference twice — which the marking's placement after the inits and the
    // non-upvalue guard each rule out on their own.
    let module = compile_to_lir(
        "(begin (def f (fn (n) \
         (letrec [helper (fn (x) (%sub x 1)) \
                   go (fn (m) (helper m))] \
           (helper (go n))))) (f 3))",
    );
    let deferrals = tail_call_deferrals(&module);
    assert_eq!(
        deferrals.iter().filter(|d| **d).count(),
        1,
        "the letrec member the body tail-calls must defer its release exactly \
         once (deferrals={deferrals:?}) — none strands one closure per call, two \
         drop the frame's single reference twice",
    );
}

/// Position of the first `TailCall` in the function that contains one, with the
/// indices of that block's `DecrefRegion`s naming the region `of` picks out of the
/// same block's allocating instructions.
///
/// Reading by REGION rather than by instruction count is what makes a decline pin
/// specific: a block releases several regions around its tail call, so "some
/// release precedes it" says nothing about which one did.
fn tail_call_slot_release_layout(
    module: &crate::lir::LirModule,
    of: impl Fn(&LirInstr) -> Option<StaticRegion>,
) -> Option<(usize, Vec<usize>)> {
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
            let Some(want) = b.instructions.iter().find_map(|i| of(&i.instr)) else {
                continue;
            };
            let releases = b
                .instructions
                .iter()
                .enumerate()
                .filter(|(_, i)| {
                    matches!(i.instr, LirInstr::DecrefRegion { region_id } if region_id == want)
                })
                .map(|(idx, _)| idx)
                .collect();
            return Some((at, releases));
        }
    }
    None
}

/// Position of the first `TailCall` in the function that contains one, with the
/// indices of that block's `DecrefCellRegion`s — the env-cell twin of
/// [`tail_call_release_layout`], which reads the value route.
fn tail_call_cell_release_layout(module: &crate::lir::LirModule) -> Option<(usize, Vec<usize>)> {
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
            let releases = b
                .instructions
                .iter()
                .enumerate()
                .filter(|(_, i)| matches!(i.instr, LirInstr::DecrefCellRegion { .. }))
                .map(|(idx, _)| idx)
                .collect();
            return Some((at, releases));
        }
    }
    None
}

#[test]
fn reassigned_env_cell_release_precedes_the_frame_replacing_tail_call() {
    // `c` is a captured local, so `populate_env` mints its cell box once per
    // activation and the box's `DecrefCellRegion` lands in the dead block. It is
    // hoisted even though `c` is REASSIGNED: the mutated refusal is compensation's
    // release-ROUTE one, and this release names the box (`LoadCaptureRaw`), which
    // an `assign` never repoints — it writes the cell's content
    // (docs/impl/region/mechanism.md § "A mutated holder poisons its value route,
    // not its cell box"; the `fresh-env-cell` probe).
    let module = compile_to_lir(
        "(begin (def f (fn () (def @c 0) \
         (let [g (fn () (assign c (%add c 1)) c)] (g)))) (f))",
    );
    let (at, releases) =
        tail_call_cell_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        releases.iter().any(|&r| r < at),
        "the reassigned env cell's release is still emitted after the TailCall \
         (at={at}, releases={releases:?}) — dead on the closure path, one box \
         stranded per activation",
    );
}

#[test]
fn escaping_holder_env_cell_release_stays_after_the_tail_call() {
    // The decline face: the closure holding the cell is RETURNED by the sibling
    // arm, so escape's capture facet marks `c` escaping and the sole-holder
    // admission refuses the box. Only the mutated refusal is scoped to the value
    // route; every escape facet still refuses, and the release keeps its place in
    // the dead block.
    let module = compile_to_lir(
        "(begin (def s (fn () 0)) \
         (def f (fn (t) (def @c 0) \
         (let [g (fn () (assign c (%add c 1)) c)] (if t g (s))))) (f false))",
    );
    let (at, releases) =
        tail_call_cell_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        releases.iter().all(|&r| r > at),
        "an escaping holder's env cell was hoisted ahead of the TailCall \
         (at={at}, releases={releases:?}) — the closure leaves carrying the cell",
    );
}

/// For the first function with two `TailCall`-bearing blocks — a branch whose
/// arms each make one — the local slots each block releases BEFORE its call and
/// those it releases after.
///
/// Reading by SLOT rather than by instruction count is what makes these pins
/// specific: an arm carries the replicated release of *every* region the merge
/// releases, so "some release precedes the call" says nothing about which.
fn branch_arm_release_slots(module: &crate::lir::LirModule) -> Vec<(Vec<u16>, Vec<u16>)> {
    let funcs = std::iter::once(&module.entry).chain(module.closures.iter());
    for f in funcs {
        let mut arms = Vec::new();
        for b in &f.blocks {
            let Some(at) = b
                .instructions
                .iter()
                .position(|i| matches!(i.instr, LirInstr::TailCall { .. }))
            else {
                continue;
            };
            let mut from_slot: std::collections::HashMap<Reg, u16> =
                std::collections::HashMap::new();
            let (mut before, mut after) = (Vec::new(), Vec::new());
            for (idx, i) in b.instructions.iter().enumerate() {
                match &i.instr {
                    LirInstr::LoadLocal { dst, slot } => {
                        from_slot.insert(*dst, *slot);
                    }
                    LirInstr::DecrefValueRegion { src } => {
                        if let Some(&slot) = from_slot.get(src) {
                            if idx < at {
                                before.push(slot);
                            } else {
                                after.push(slot);
                            }
                        }
                    }
                    _ => {}
                }
            }
            arms.push((before, after));
        }
        if arms.len() >= 2 {
            return arms;
        }
    }
    Vec::new()
}

#[test]
fn stranded_param_release_is_replicated_into_every_branch_arm() {
    // The release lands past the MERGE, which each arm leaves through a
    // frame-replacing tail call — so the merge copy alone reaches neither path.
    // The merge's inherited relocation points put a copy ahead of each arm's
    // `TailCall` (docs/impl/region/mechanism.md § "The relocation point outlives
    // the block"; the `tail-frame-exit-arms` probe). `x` is the first parameter,
    // hence local slot 0.
    let module = compile_to_lir(
        "(begin (def s (fn () 0)) (def s2 (fn () 1)) \
         (def f (fn (x t) (if t (s) (s2)))) (f (list 1 2) true))",
    );
    let arms = branch_arm_release_slots(&module);
    assert_eq!(arms.len(), 2, "the body lowers to one TailCall per arm");
    for (before, after) in &arms {
        assert!(
            before.contains(&0),
            "an arm's copy of the stranded parameter's release is missing \
             (before={before:?}, after={after:?}) — dead on that arm's closure path",
        );
    }
}

#[test]
fn moved_argument_takes_no_replica_in_the_arm_that_moves_it() {
    // The exemption is read PER point. `x` (local slot 0) is the then-arm call's
    // argument, so that arm takes no replica of `x`'s release — the callee's
    // owned-parameter release is what frees it there. The same arm still takes a
    // replica of `t`'s release, and the merge's other point, whose call names
    // nothing, takes one of `x`'s.
    let module = compile_to_lir(
        "(begin (def s (fn (a) a)) (def s2 (fn () 1)) \
         (def f (fn (x t) (if t (s x) (s2)))) (f (list 1 2) true))",
    );
    let arms = branch_arm_release_slots(&module);
    assert_eq!(arms.len(), 2, "the body lowers to one TailCall per arm");
    let (moving_before, moving_after) = &arms[0];
    assert!(
        !moving_before.contains(&0),
        "the moved argument's release was replicated ahead of the arm's TailCall \
         (before={moving_before:?}) — that release IS the ownership move",
    );
    assert!(
        !moving_after.contains(&0),
        "the moved argument's release was left in the arm's dead block \
         (after={moving_after:?}) — nothing there runs",
    );
    assert!(
        moving_before.contains(&1),
        "the arm took no replica at all (before={moving_before:?}) — the exemption \
         is read per REGION at each point, not per point",
    );
    let (other_before, _) = &arms[1];
    assert!(
        other_before.contains(&0),
        "the sibling arm, whose call names nothing, did not take the replica \
         (before={other_before:?})",
    );
}

/// The local slots each block of `func` releases by value, tagged with whether
/// that block ends in a frame-replacing `TailCall`.
///
/// A branch whose arms do not all tail-call is read here rather than through
/// `branch_arm_release_slots`, which needs one `TailCall` per arm: what these pins
/// ask is whether the release reaches the arm that FALLS THROUGH, so the merge
/// block — which makes no tail call at all — is the block that has to carry it.
fn released_slots_by_block(func: &LirFunction) -> Vec<(bool, Vec<u16>)> {
    func.blocks
        .iter()
        .map(|b| {
            let exits = b
                .instructions
                .iter()
                .any(|i| matches!(i.instr, LirInstr::TailCall { .. }));
            let mut from_slot: rustc_hash::FxHashMap<Reg, u16> = rustc_hash::FxHashMap::default();
            let mut slots = Vec::new();
            for i in &b.instructions {
                match &i.instr {
                    LirInstr::LoadLocal { dst, slot } => {
                        from_slot.insert(*dst, *slot);
                    }
                    LirInstr::DecrefValueRegion { src } => {
                        if let Some(&slot) = from_slot.get(src) {
                            slots.push(slot);
                        }
                    }
                    _ => {}
                }
            }
            (exits, slots)
        })
        .collect()
}

/// The first function whose blocks include both a `TailCall`-bearing one and one
/// without — a branch where only some arms leave through a callee.
fn mixed_exit_function(module: &crate::lir::LirModule) -> Vec<(bool, Vec<u16>)> {
    std::iter::once(&module.entry)
        .chain(module.closures.iter())
        .map(released_slots_by_block)
        .find(|blocks| {
            blocks.iter().any(|(exits, _)| *exits)
                && blocks.iter().any(|(exits, s)| !*exits && !s.is_empty())
        })
        .unwrap_or_default()
}

// ── A tail-calling arm does not hold back its falling-through siblings ───────
//
// The branch-arm release window anchors a region's one release at the branch's
// consuming node, the point every arm reaches. One arm shape does not reach it: a
// tail call to a closure replaces the frame. Declining the whole branch for that
// strands the region on every OTHER arm — the `append`/`concat` dispatch shape,
// where the list arm hands the argument to `append-list` and every other arm pays
// the argument's whole object graph. The relocation covers the frame-exiting arm
// instead, by replica or by the ownership-move exemption
// (docs/impl/region/mechanism.md § "An arm that leaves through a callee takes a
// replica, not the anchor").
//
// The counterfactual is declining the branch whole: then `x`'s only release sits
// in the tail-calling arm and no block outside it names slot 0. End-to-end
// witnesses: tests/elle/region-branch-arm-window.lisp (rows g/h) and
// tests/elle/region-branch-arm-window-uaf.lisp.

#[test]
fn fallthrough_arm_releases_though_a_sibling_tail_call_exits() {
    // `x` (the first parameter, hence local slot 0) is named by both arms, so its
    // `decref_point` lands in the later one — which tail-calls. The falling-through
    // arm must still reach a release.
    let module = compile_to_lir(
        "(begin (def s (fn (a) (length a))) \
         (def f (fn (x t) (if t (length x) (s x)))) (f (list 1 2) true))",
    );
    let blocks = mixed_exit_function(&module);
    assert!(
        !blocks.is_empty(),
        "expected a function with both a tail-calling and a falling-through block",
    );
    assert!(
        blocks
            .iter()
            .any(|(exits, slots)| !*exits && slots.contains(&0)),
        "the stranded parameter's release is emitted only where the frame is \
         replaced (blocks={blocks:?}) — the falling-through arm frees nothing",
    );
}

#[test]
fn tail_call_argument_release_stays_the_ownership_move() {
    // The complement of the pin above, on the same shape: the arm that tail-calls
    // with `x` keeps its release AFTER the `TailCall`. That release is the
    // ownership move the callee's owned-parameter release consumes, so a replica
    // ahead of the call would drop the callee's reference.
    let module = compile_to_lir(
        "(begin (def s (fn (a) (length a))) \
         (def f (fn (x t) (if t (length x) (s x)))) (f (list 1 2) true))",
    );
    let func = std::iter::once(&module.entry)
        .chain(module.closures.iter())
        .find(|f| {
            let blocks = released_slots_by_block(f);
            blocks.iter().any(|(exits, _)| *exits)
                && blocks.iter().any(|(exits, s)| !*exits && !s.is_empty())
        })
        .expect("a function with both a tail-calling and a falling-through block");
    for b in &func.blocks {
        let Some(at) = b
            .instructions
            .iter()
            .position(|i| matches!(i.instr, LirInstr::TailCall { .. }))
        else {
            continue;
        };
        let mut from_slot: rustc_hash::FxHashMap<Reg, u16> = rustc_hash::FxHashMap::default();
        for (idx, i) in b.instructions.iter().enumerate() {
            match &i.instr {
                LirInstr::LoadLocal { dst, slot } => {
                    from_slot.insert(*dst, *slot);
                }
                LirInstr::DecrefValueRegion { src } if from_slot.get(src) == Some(&0) => {
                    assert!(
                        idx > at,
                        "the tail call's own argument was released ahead of it \
                         — that release IS the ownership move",
                    );
                }
                _ => {}
            }
        }
    }
}

// ── A re-storable capture cell's slot is not a release route ─────────────────
//
// A binding defined outside any lambda and captured by a closure lives in a
// compiled `MakeCaptureCell` held in the binding's own slot. A value-routed
// release against that slot (`LoadLocal slot` + `DecrefValueRegion`) unwraps the
// cell — `result_region_of` sees through a capture cell — and frees the region of
// whatever content the cell holds when the release FIRES. For a cell an `assign`
// repoints, that is a different, live value: the capture-cell reassign UAF
// (docs/impl/region/bindings.md § "Captured reassigned cells"). The init's
// producer reference is dropped at the define instead
// (`store_captured_cell_init`), so no such route may be emitted at all.
//
// The counterfactual is reading the reassign off the ASSIGN SITE's scope: every
// shape below writes the cell from inside a closure, which classifies as fn-local
// and leaves the route in place. End-to-end witness:
// tests/integration/fixtures/region-capture-cell-closure-reassign-uaf.lisp.

/// The local slots that hold a compiled `MakeCaptureCell` in `func` — the cell
/// boxes a `StoreLocal` parks right after the mint.
fn compiled_cell_slots(func: &LirFunction) -> Vec<u16> {
    let instrs: Vec<&LirInstr> = func
        .blocks
        .iter()
        .flat_map(|b| b.instructions.iter())
        .map(|si| &si.instr)
        .collect();
    let mut slots = Vec::new();
    for (i, instr) in instrs.iter().enumerate() {
        let LirInstr::MakeCaptureCell { dst, .. } = instr else {
            continue;
        };
        for later in &instrs[i + 1..] {
            if let LirInstr::StoreLocal { slot, src } = later {
                if src == dst {
                    slots.push(*slot);
                    break;
                }
            }
        }
    }
    slots
}

/// The local slots `func` loads and then releases by value (`LoadLocal slot`
/// feeding a `DecrefValueRegion` on the same register).
fn value_released_slots(func: &LirFunction) -> Vec<u16> {
    let instrs: Vec<&LirInstr> = func
        .blocks
        .iter()
        .flat_map(|b| b.instructions.iter())
        .map(|si| &si.instr)
        .collect();
    let mut loaded: rustc_hash::FxHashMap<Reg, u16> = rustc_hash::FxHashMap::default();
    let mut slots = Vec::new();
    for instr in &instrs {
        match instr {
            LirInstr::LoadLocal { dst, slot } => {
                loaded.insert(*dst, *slot);
            }
            LirInstr::DecrefValueRegion { src } => {
                if let Some(&slot) = loaded.get(src) {
                    slots.push(slot);
                }
            }
            _ => {}
        }
    }
    slots
}

fn assert_no_cell_slot_value_release(source: &str, what: &str) {
    let module = compile_to_lir(source);
    for func in std::iter::once(&module.entry).chain(module.closures.iter()) {
        let cells = compiled_cell_slots(func);
        if cells.is_empty() {
            continue;
        }
        let released = value_released_slots(func);
        for slot in &cells {
            assert!(
                !released.contains(slot),
                "{what}: slot {slot} holds a compiled capture cell yet carries a \
                 value-routed release — `DecrefValueRegion` unwraps the cell and \
                 frees whatever content it holds when the release fires, which a \
                 reassignment has already repointed (the capture-cell reassign UAF)",
            );
        }
    }
}

#[test]
fn closure_reassign_leaves_no_cell_slot_release() {
    // The cell's content at the frame exit is the value the closure last stored,
    // and it is also the value the frame hands back.
    assert_no_cell_slot_value_release(
        "(begin (var results (list)) \
         (def collect (fn (n) (begin (assign results (list n results)) results))) \
         (collect 5))",
        "a cell repointed by a closure",
    );
}

#[test]
fn heap_init_closure_reassign_leaves_no_cell_slot_release() {
    // A heap init exercises the drop the define owes: the producer's reference
    // dies off the value register, leaving the cell's counted one.
    assert_no_cell_slot_value_release(
        "(begin (var acc (list 0)) \
         (def push (fn (x) (assign acc (list x acc)))) \
         (push 1) (push 2) acc)",
        "a heap-initialized cell repointed by a closure",
    );
}

#[test]
fn nested_closure_reassign_leaves_no_cell_slot_release() {
    // The write site moves two closures deep; the binding is still the outer
    // scope's, so the classification must not follow the write site.
    assert_no_cell_slot_value_release(
        "(begin (var slot (list)) \
         (def inner (fn (x) (assign slot (list x slot)))) \
         (def outer (fn (x) (inner x))) \
         (outer 7) (outer 8) slot)",
        "a cell repointed two closures deep",
    );
}
