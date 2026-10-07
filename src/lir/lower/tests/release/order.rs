// audited: 2026-10-06
//! The order releases take when several land on one `decref_point`: readers before the free, members before owners.
//!
//! docs/impl/region/rules.md
//! docs/impl/region/adopt.md

use super::*;

// ── Release order at a shared decref_point (docs/impl/region/rules.md Rule 4) ─────
//
// When several releases land on one decref_point, page-READING releases
// (`DecrefValueRegion` — loads a slot and derefs the value, unwrapping a capture cell)
// must be emitted before page-FREEING releases (`DecrefRegion`). The counterfactual
// (tests/impl/region-capture-cell-noreassign-uaf.lisp): the cell's `DecrefRegion` frees
// the cell's pages, then the init's `DecrefValueRegion` unwraps the freed cell. The
// per-point order must not depend on `HashMap` iteration (random per instance); the
// loop runs many compiles so any nondeterministic unsafe ordering fails the test.

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
                     page-freeing release would tear the page the unwrap reads",
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
fn store_adopted_member_release_precedes_owner_in_shared_bucket() {
    // A store-adopted member's own `DecrefRegion` is an `Owned` no-op only while the
    // member is still `Owned`, so it must be emitted BEFORE every release that can free
    // the member's owner. At a shared `decref_point` the intra-bucket order is what
    // enforces this (docs/impl/region/adopt.md). The counterfactual
    // (tests/impl/region-array-push-pair-loop-uaf.lisp): the container is a `Fresh`
    // call-result freed value-based (and, when its push result is discarded, freed a
    // second time by that pass-through result), and the pushed `%pair` is a
    // plain-`DecrefRegion` member sharing the container's `decref_point`. Order the
    // member's plain `DecrefRegion` after the container's rc-zeroing release and the
    // container's subtree drop reclaims the pair before its own decref — which then
    // faults on the freed region. The topological order over the adopt edge (member →
    // owner) keeps the member first.
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
                     DecrefRegion fires, which then faults on the freed region",
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
    // tests/impl/region-container-read-borrow-uaf.lisp.
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
fn a_cell_box_release_follows_every_release_that_unwraps_it() {
    // Two releases address one env index: `DecrefValueRegion` loads the box RAW
    // and unwraps it to the content (so it READS the box's page), and
    // `DecrefCellRegion` frees that page. Emitting the free first leaves the
    // unwrap reading reclaimed memory — a stray release of whatever region id
    // the recycled page spells (docs/impl/region/cells.md).
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
                InstrRef::LoadCaptureRaw { dst, index } => {
                    raw_from.insert(*dst, *index);
                }
                InstrRef::DecrefValueRegion { src } => {
                    if let Some(&index) = raw_from.get(src) {
                        let e = unwraps.entry(index).or_insert(pos);
                        *e = (*e).max(pos);
                    }
                }
                InstrRef::DecrefCellRegion { src } => {
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
