use super::*;

// ── ownership inference: the capture-adopt contract (suppress ⊆ adopt) ──────────
//
// `analyze_regions_with` suppresses the own-decref of every `capture_adopt_edges` member
// (it is reclaimed solely by its closure's subtree drop), and `lower_lambda_expr` emits
// the matching `AdoptRegion` by reloading the captured value through the capture's own
// access path — a binding slot for a direct local, the constructing function's
// environment for an upvalue or transitive capture (region/adopt.md § "The capture
// adopt"). The contract is therefore held by EMIT CAPABILITY: an edge is emittable iff
// the closure genuinely captures a binding holding the member's region, which is true of
// every capture containment edge by construction — the `debug_assert` at the emit seam
// in `lower_lambda_expr` is the backstop. What bounds ADMISSION is the lifetime
// obligation alone; for the cross-activation (upvalue) family it refuses by
// construction — the forwarding capture pins the member's tight last-use at/past the
// enclosing lambda's own node, after a nested root's in-body drop — and genuinely must
// (a nested root's region is per-call of its encloser; claiming a member that survives
// across calls would free it under the encloser's live env and re-adopt an Owned region
// on the next call). These pins lock both halves at the inference level.

#[test]
fn capture_adopt_edges_are_emittable() {
    // Every capture adopt edge `compute_adopt_edges` emits must be EMITTABLE: the owning
    // closure's Lambda must capture a binding whose source regions include the member, so
    // `lower_lambda_expr`'s reload (slot or env) can find the value for the
    // value-resolved adopt (suppress ⊆ adopt). Checked across the shape that DOES adopt a
    // capture and the web/upvalue shapes that are refused (vacuous there, but they
    // exercise the path that would regress). Counterfactual: a `compute_adopt_edges`
    // change that chose an owner-edge no capture realizes — e.g. keying a member onto a
    // closure that merely reaches it transitively — fails this directly, before the
    // suppressed-yet-unadopted leak ever reaches the runtime.
    let shapes = [
        // The simple local capture that IS adopted (the one capture edge that fires today).
        "(begin (let [p (%pair 1 2)] (let [c (fn [] (%first p))] (c))) nil)",
        // Closure-webs (refused on the lifetime obligation): mutually-recursive closures
        // over a shared captured value, and a nested closure capturing an outer binding
        // as an upvalue.
        "(begin (let [b (%pair 1 2)] (letrec [f (fn [] (begin (g) (%first b))) g (fn [] (%first b))] (f))) nil)",
        "(begin (let [b (%pair 1 2)] (let [outer (fn [] (let [inner (fn [] (%first b))] (inner)))] (outer))) nil)",
    ];
    for src in shapes {
        let (hir, info, edges) = adopt_edges(src);
        for (member, closure) in edges.capture.values().flatten().copied() {
            assert!(
                closure_captures_region(&hir, &info, member, closure),
                "capture adopt edge (member r{}, closure r{}) is NOT emittable: no capture \
                 of the closure holds the member's region, so analyze suppresses its decref \
                 but the lowerer cannot adopt it — a leak. src={src}",
                member.0,
                closure.0,
            );
        }
    }
}

#[test]
fn owned_subtree_upvalue_capture_owner_refused_on_lifetime() {
    // The cross-activation boundary (region/adopt.md § "The capture adopt"): a nested
    // closure `o` (inside `e`) captures `e` (the letrec recursion) AND the pair `m`, and
    // `e` ALSO captures `m` (the forward every upvalue implies). With the capture edge
    // re-pointed through the cell (`closure ⊇ cell ⊇ content`), external uniqueness ADMITS
    // the subtree `{o, cell_e, e, m}` with `o` as root — the containment is now visible —
    // and the refusal MUST hold at the lifetime obligation: `o`'s region is minted per
    // CALL of `e`, so adopting `m`/`e` (which survive across calls) would free them under
    // `e`'s still-live references and re-adopt an already-Owned region on the next call.
    // The obligation refuses structurally: the forwarding capture resolves `m`'s tight
    // last-use through `e`'s binding chain to a position at/past the enclosing lambda node,
    // which post-dates `o`'s in-body drop in post-order. Three halves:
    //   (1) the subtree is admitted, and the root's capture of `m` is env-loaded (an
    //       upvalue) — the shape genuinely exercises the cross-activation path;
    //   (2) the obligation's refusal CAUSE is pinned: `m`'s tight last-use over-extends
    //       past the root's decref_point (the exact condition it refuses on);
    //   (3) no adopt is emitted — the family stays Shared (the always-legal baseline)
    //       until an owner that outlives every capturer (the activation/fiber node) exists.
    let src = "(begin (let [m (%pair 1 2)] \
                        (letrec [e (fn [] (let [o (fn [] (begin (e) (%first m)))] (o)))] (e))) \
                      nil)";
    let (hir, info, owned) = owned_subtrees_with_effects(src);
    let m = sole_pair_region(&hir, &info);
    // (1) admitted, with an env-loaded (upvalue) owner-capture of `m`.
    let owner = owned
        .iter()
        .find(|(_, s)| s.contains(&m))
        .map(|(r, _)| *r)
        .expect("compute_owned_subtrees must admit the externally-unique upvalue subtree");
    assert!(
        !capture_is_local_slot_loaded(&hir, &info, m, owner),
        "precondition: the pair r{} must be captured by its subtree root r{} through the \
         ENV (an upvalue capture); if slot-loaded, the shape no longer exercises the \
         cross-activation boundary",
        m.0,
        owner.0,
    );
    // (2) the refusal cause: the tight last-use over-extends past the root's drop. A
    // change that makes this pass — a tighter transitive last-use, or a root whose drop
    // moves out of the enclosing body — flips this precondition and forces the author to
    // re-derive the soundness argument (the per-call-root double-adopt) before admitting
    // the family.
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let root_dp = ord(info
        .region_data
        .get(&owner)
        .expect("root has region_data")
        .decref_point);
    let m_tight = info
        .binding_last_use
        .get(&m)
        .map(|&id| ord(id))
        .expect("the captured pair has a tight last-use");
    assert!(
        m_tight > root_dp,
        "precondition (the refusal cause): the upvalue-captured member r{}'s tight \
         last-use ({m_tight}) must over-extend past the per-call root r{}'s decref_point \
         ({root_dp}); if it no longer does, the lifetime obligation would admit a per-call \
         root owning a cross-call member — re-derive the soundness argument before flipping \
         the assertion below",
        m.0,
        owner.0,
    );
    // (3) so no adopt is emitted for the whole program — the family stays Shared.
    let (_, _, edges) = adopt_edges(src);
    assert!(
        edges.store.is_empty() && edges.capture.is_empty(),
        "the upvalue-owner family must emit NO adopts (refused to Shared on the lifetime \
         obligation); got store={:?}, capture={:?}",
        edges.store,
        edges.capture,
    );
}

#[test]
fn owned_subtree_refuses_fiber_member() {
    // A fiber's region is never a member of a region-rooted Owned subtree
    // (region/adopt.md § "The fiber member — refused at the class level"): a fiber
    // value acquires aliases by merely RUNNING — the scheduler's parent/child chain
    // and the `fiber/child`/`fiber/parent` graph reads create references at runtime
    // that no structural post-dominance predicate can see — and adoption freezes the
    // region's RC, so no retain could pin an adopted fiber under those reads. The
    // capture shape below is exactly what external uniqueness would otherwise admit:
    // a closure sole-captures a `fiber/new` result ({closure ⊇ fiber}, nothing else
    // references in). The fiber's region is a declared-`RetType::Fiber` fresh result
    // (`RegionInfo::fiber_result_regions`), a dynamic-lifetime class `not_ownable`
    // refuses — the family stays Shared (the always-legal baseline) and the fiber
    // reclaims on ordinary RC.
    let src = "(let [f (fiber/new (fn [] 1) 0)] \
                 (let [g (fn [] (fiber/resume f))] (begin (g) nil)))";
    let (_, _, _, cedges) = capture_edges(src);
    assert_eq!(
        cedges.len(),
        1,
        "precondition: `g` sole-captures `f` — exactly one capture edge; got {cedges:?}",
    );
    let (_, fiber_r, _) = cedges[0];
    let (_, _, owned) = owned_subtrees_with_effects(src);
    assert!(
        !in_some_owned_subtree(&owned, fiber_r),
        "a fiber's region (r{}) must be a member of NO Owned subtree; got {:?}",
        fiber_r.0,
        owned,
    );

    // The admitting twin: the same shape over an `@array` IS externally unique and
    // adopts — proving the fiber refusal above is the fiber class, not an artifact
    // of the capture shape.
    let twin = "(let [f (@array)] \
                  (let [g (fn [] (length f))] (begin (g) nil)))";
    let (_, _, _, twin_cedges) = capture_edges(twin);
    assert_eq!(
        twin_cedges.len(),
        1,
        "precondition: the twin's `g` sole-captures `f`; got {twin_cedges:?}",
    );
    let (_, array_r, _) = twin_cedges[0];
    let (_, _, twin_owned) = owned_subtrees_with_effects(twin);
    assert!(
        in_some_owned_subtree(&twin_owned, array_r),
        "the @array twin (r{}) must still be adopted — if it refuses too, the fiber \
         assertion above is vacuous; got {:?}",
        array_r.0,
        twin_owned,
    );
}

// ── ownership inference: the capture-cell clique (closure ⊇ cell ⊇ content) ─────
//
// A local `letrec`/`def` closure that captures a sibling's forward cell forms the chain
// `closure ⊇ cell ⊇ content` — the capture edge re-pointed through the cell
// (`capture_containment_edges`) and the walk's `cell ⊇ content` edge together. With both
// visible, `compute_owned_subtrees` admits the local, non-escaping `{closure, cell,
// content}` clique as externally unique (the modeling's headline — the invisible cell
// containment is now seen). It refuses the moment the clique is NOT externally unique: the
// closure escapes (a Shared seed), or the cell is captured by TWO siblings (an interior
// region referenced from outside any single-root subtree). These pin that admission and
// its two boundaries.

/// The sole compiled cell region and its content region (the `(site, content, cell)` walk
/// edge) of a single-cell shape — the clique's interior two members.
fn sole_cell_and_content(info: &RegionInfo) -> (Region, Region) {
    let cells: Vec<Region> = info
        .begin_cell_regions
        .values()
        .flatten()
        .map(|&(_, r)| r)
        .collect();
    assert_eq!(
        cells.len(),
        1,
        "shape must mint exactly one compiled cell; got {cells:?}",
    );
    let cell = cells[0];
    let content = info
        .containment_edges
        .iter()
        .find(|&&(_, _, c)| c == cell)
        .map(|&(_, content, _)| content)
        .unwrap_or_else(|| {
            panic!(
                "the cell r{} must carry a `cell ⊇ content` edge; containment={:?}",
                cell.0, info.containment_edges,
            )
        });
    (cell, content)
}

#[test]
fn owned_subtrees_admits_local_capture_cell_clique() {
    // A local `letrec`: `drive` captures the forward cell of `leaf` (`(fn [m] m)`), which
    // holds its closure. Neither escapes (`(drive n)`'s result — an int — is what leaves).
    // With `closure ⊇ cell` re-pointed and `cell ⊇ content` recorded, the clique
    // `{drive_closure, leaf_cell, leaf_content}` is externally unique → admitted.
    //
    // Counter-factual: before the modeling the `closure ⊇ content` mis-pointing left the
    // content a free-standing top container the scan could not bound through its cell, so no
    // subtree formed over it (the invisible-containment hole this closes).
    let (_hir, info, owned) = owned_subtrees_with_effects(
        "(defn build [n] \
           (letrec [drive (fn [m] (leaf m)) \
                    leaf (fn [m] m)] \
             (begin (drive n) 0)))",
    );
    let (cell, content) = sole_cell_and_content(&info);
    let subtree = owned
        .values()
        .find(|s| s.contains(&cell))
        .unwrap_or_else(|| {
            panic!(
                "compute_owned_subtrees must admit a subtree containing the cell r{} \
                 (the `closure ⊇ cell ⊇ content` clique); owned={:?}",
                cell.0, owned,
            )
        });
    assert!(
        subtree.contains(&content),
        "the admitted subtree must contain the cell's content r{} too (the whole chain \
         `closure ⊇ cell ⊇ content` reclaims as a unit); subtree={:?}",
        content.0,
        subtree.iter().map(|r| r.0).collect::<Vec<_>>(),
    );
}

#[test]
fn owned_subtrees_refuses_escaping_capture_cell_clique() {
    // The escape boundary: the capturing closure `drive` is RETURNED, so it crosses the
    // return frontier (a Shared seed) and its whole `closure ⊇ cell ⊇ content` chain must
    // stay Shared. `compute_owned_subtrees` admits NO subtree over the cell.
    // (region-repeated-call-adopt-uaf.lisp is the runtime witness of this boundary — an
    // escaping top-level chain must never be subtree-dropped.)
    let (_hir, info, owned) = owned_subtrees_with_effects(
        "(defn build [n] \
           (letrec [drive (fn [m] (leaf m)) \
                    leaf (fn [m] m)] \
             drive))",
    );
    let (cell, _content) = sole_cell_and_content(&info);
    assert!(
        !in_some_owned_subtree(&owned, cell),
        "an escaping capturing closure must leave its cell r{} Shared (in no owned \
         subtree); owned={:?}",
        cell.0,
        owned,
    );
}

#[test]
fn owned_subtrees_refuses_two_sibling_captured_cell() {
    // The external-uniqueness boundary: `leaf`'s forward cell is captured by TWO siblings
    // `a` and `b`. For a subtree rooted at either, the OTHER closure holds the cell from
    // outside it (`outside_ref_in`), so neither is externally unique — the cell stays
    // Shared. This is the "captured by two siblings" refusal the modeling must keep.
    let (_hir, info, owned) = owned_subtrees_with_effects(
        "(defn build [n] \
           (letrec [a (fn [m] (leaf m)) \
                    b (fn [m] (leaf m)) \
                    leaf (fn [m] m)] \
             (begin (a n) (b n) 0)))",
    );
    let (cell, _content) = sole_cell_and_content(&info);
    assert!(
        !in_some_owned_subtree(&owned, cell),
        "a cell r{} captured by two siblings must be Shared (no single-root subtree is \
         externally unique); owned={:?}",
        cell.0,
        owned,
    );
}

// ── ownership inference: the re-storable-cell gate (§3 loop hazard) ─────────────
//
// An `@`-mutable captured cell is re-stored every loop iteration; its release is hoisted
// once past the loop, so each stored content's lifetime is `[store, next-rebind]` —
// SHORTER than the cell's. Adopting it into the cell's subtree would free a displaced
// prior under a live cell (`region-capture-cell-loop-uaf.lisp`). So a re-storable cell's
// content is NOT adoptable: `capture_containment_edges` skips the capture (the cell stays
// a borrow), and `compute_adopt_edges`'s `adoptable_cell` refuses its `cell ⊇ content`
// edge even when the walk records it (for external-uniqueness *counting*, §3). The
// immutable letrec cell in the same clique is handled the other way — re-pointed and
// adoptable. These pin both halves of the gate.

#[test]
fn capture_edge_skips_restorable_cell_admits_immutable_in_one_clique() {
    // One clique, both cell kinds: `holder` captures `@acc` (an `@`-mutable local — a
    // re-storable cell) AND `leaf` (an immutable letrec forward cell). The immutable
    // capture is re-pointed `closure ⊇ cell`; the re-storable capture yields NO owner edge
    // — its content reclaims on the per-region-RC baseline, never adopted under the cell.
    let (hir, arena, info, edges) = capture_edges(
        "(defn build [] \
           (letrec [@acc (list 1 2) \
                    leaf (fn [] 1) \
                    holder (fn [] (begin acc (leaf)))] \
             (holder)))",
    );
    // Find `holder`'s two captures by their re-storable classification, so the shape is
    // proven to genuinely mix both kinds (the "one clique with both" premise).
    let mut restorable_binding: Option<Binding> = None;
    let mut immutable_binding: Option<Binding> = None;
    fn find_caps(
        h: &Hir,
        arena: &BindingArena,
        restorable: &mut Option<Binding>,
        immutable: &mut Option<Binding>,
    ) {
        if let HirKind::Lambda { captures, .. } = &h.kind {
            for c in captures {
                let bi = arena.get(c.binding);
                if bi.is_restorable_capture_cell() {
                    *restorable = Some(c.binding);
                } else if bi.needs_capture() {
                    *immutable = Some(c.binding);
                }
            }
        }
        h.for_each_child(|c| find_caps(c, arena, restorable, immutable));
    }
    find_caps(
        &hir,
        &arena,
        &mut restorable_binding,
        &mut immutable_binding,
    );
    let restorable = restorable_binding.expect("holder must capture a re-storable @acc");
    let immutable = immutable_binding.expect("holder must capture an immutable leaf cell");
    // The immutable cell IS re-pointed: `single_cell_region_of(leaf)` names a compiled
    // cell, and a `closure ⊇ cell` edge points at it.
    let leaf_cell = info
        .single_cell_region_of(immutable)
        .expect("the immutable leaf must have a compiled cell");
    assert!(
        edges.iter().any(|&(_, src, _)| src == leaf_cell),
        "the immutable leaf cell r{} must get a `closure ⊇ cell` capture edge; edges={edges:?}",
        leaf_cell.0,
    );
    // The re-storable @acc yields NO owner edge: none of its source regions is a capture
    // edge source (the `is_restorable_capture_cell` skip — the §3 loop hazard). @acc takes
    // the `populate_env` route so it has no compiled cell of its own.
    assert_eq!(
        info.single_cell_region_of(restorable),
        None,
        "the re-storable @acc must have no compiled cell (the populate_env borrow route)",
    );
    let restorable_regions = info
        .binding_source_regions
        .get(&restorable)
        .cloned()
        .unwrap_or_default();
    for r in &restorable_regions {
        assert!(
            !edges.iter().any(|&(_, src, _)| src == *r),
            "the re-storable @acc's region r{} must appear in NO capture edge — its content \
             stays a borrow (the §3 loop hazard); edges={edges:?}",
            r.0,
        );
    }
    // Neither is capture-adopted through the cell-content path (the re-storable is refused;
    // the immutable is reached via its holder's subtree, not a cell-store adopt).
    assert!(
        !info.cell_content_adopt_bindings.contains(&restorable),
        "the re-storable @acc must never be a cell-content adopt binding",
    );
}

#[test]
fn restorable_compiled_cell_records_content_edge_but_is_not_adopted() {
    // The `compute_adopt_edges` half: a TOP-LEVEL `@acc` is an `@`-mutable captured binding,
    // so its cell is a re-storable COMPILED cell. The walk STILL records its `cell ⊇ content`
    // edge (for external-uniqueness counting — the cell holds *a* content, §3), but
    // `adoptable_cell` refuses it, so the cell's binding never reaches
    // `cell_content_adopt_bindings`. The re-storable content is therefore never linked into
    // the cell's subtree — it keeps its own per-rebind release, the safe borrow.
    let (_hir, arena, info, edges) = capture_edges(
        "(def @acc (list 1 2)) \
         (def reader (fn [] acc)) \
         (length (reader))",
    );
    // Precondition: `@acc` is a re-storable compiled cell.
    let restorable_cells: Vec<(Binding, Region)> = info
        .begin_cell_regions
        .values()
        .flatten()
        .copied()
        .filter(|&(b, _)| arena.get(b).is_restorable_capture_cell())
        .collect();
    assert_eq!(
        restorable_cells.len(),
        1,
        "precondition: exactly one re-storable compiled cell (@acc); got {restorable_cells:?}",
    );
    let (acc_binding, acc_cell) = restorable_cells[0];
    // The walk DID record its `cell ⊇ content` edge (the cell holds the list) ...
    assert!(
        info.containment_edges
            .iter()
            .any(|&(_, _, cell)| cell == acc_cell),
        "the re-storable cell r{} must still carry a recorded `cell ⊇ content` edge \
         (external-uniqueness counting, §3); containment={:?}",
        acc_cell.0,
        info.containment_edges,
    );
    // ... yet `adoptable_cell` refuses it: the binding is not a cell-content adopt, and its
    // capture is skipped (no owner edge). The re-storable content stays a borrow.
    assert!(
        !info.cell_content_adopt_bindings.contains(&acc_binding),
        "the re-storable cell's content must NOT be adopted (adoptable_cell refuses) — \
         cell_content_adopt_bindings={:?}",
        info.cell_content_adopt_bindings,
    );
    assert!(
        !edges.iter().any(|&(_, src, _)| src == acc_cell),
        "the re-storable cell r{} must be captured by no `closure ⊇ cell` edge (the \
         capture skip); edges={edges:?}",
        acc_cell.0,
    );
}
