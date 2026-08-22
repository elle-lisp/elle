use super::*;

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

#[test]
fn a_cell_box_release_follows_every_release_that_unwraps_it() {
    // Two releases address one env index: `DecrefValueRegion` loads the box RAW
    // and unwraps it to the content (so it READS the box's page), and
    // `DecrefCellRegion` frees that page. Emitting the free first leaves the
    // unwrap reading reclaimed memory — a stray release of whatever region id
    // the recycled page spells (docs/impl/region/bindings.md § "A cell's release
    // lands at or after every release routed through that cell").
    //
    // The shape is a `def` inside a lambda captured by a sibling closure: `p` is
    // env-celled, its init is a call so it owns a value region of its own, and
    // the capture is `p`'s last binding-use — which puts the box's release at
    // the capture and the value's release at the enclosing `let`.
    let module = compile_to_lir(
        "((fn [] \
            (def p (f 1)) \
            (let [r (fn [] (g p))] :built)))",
    );
    let mut checked = 0usize;
    for func in std::iter::once(&module.entry).chain(module.closures.iter()) {
        let instrs = flat_instrs(func);
        // Which env index each register was loaded raw from. A register id is
        // reused across a function, so the map records the LATEST load, which is
        // the one the release right after it names.
        let mut raw_from: rustc_hash::FxHashMap<Reg, u16> = rustc_hash::FxHashMap::default();
        // Per env index: where its box is freed, and where its content is last
        // unwrapped out of that box.
        let mut frees: rustc_hash::FxHashMap<u16, usize> = rustc_hash::FxHashMap::default();
        let mut unwraps: rustc_hash::FxHashMap<u16, usize> = rustc_hash::FxHashMap::default();
        for (pos, instr) in instrs.iter().enumerate() {
            match instr {
                LirInstr::LoadCaptureRaw { dst, index } => {
                    raw_from.insert(*dst, *index);
                }
                LirInstr::DecrefValueRegion { src } => {
                    if let Some(&index) = raw_from.get(src) {
                        let e = unwraps.entry(index).or_insert(pos);
                        *e = (*e).max(pos);
                    }
                }
                LirInstr::DecrefCellRegion { src } => {
                    if let Some(&index) = raw_from.get(src) {
                        let e = frees.entry(index).or_insert(pos);
                        *e = (*e).max(pos);
                    }
                }
                _ => {}
            }
        }
        for (index, free_at) in &frees {
            checked += 1;
            let Some(&unwrap_at) = unwraps.get(index) else {
                continue;
            };
            assert!(
                *free_at > unwrap_at,
                "cap[{index}]'s DecrefCellRegion at #{free_at} frees the box before \
                 the DecrefValueRegion at #{unwrap_at} unwraps that same box — the \
                 unwrap reads a reclaimed page. instrs={instrs:?}",
            );
        }
    }
    assert!(
        checked > 0,
        "expected the captured-`def` shape to emit at least one DecrefCellRegion \
         off a LoadCaptureRaw — if lowering changed, update the shape so this test \
         keeps biting",
    );
}
