use super::*;

// ── 1-slot-container gate: sole-held AND not-returned ───────────────
//
// docs/impl/region/bindings.md "Reassigned mutable bindings are 1-slot containers":
// the drop-on-overwrite + suppression model may be applied only when
// every region the cell may hold is sole-held by the binding AND not
// claimed by ownership transfer at a return/tail boundary (two static
// owners of one initial reference is a double-free). Runtime-counted
// escapes — container stores, captures, opaque-call cliques — are
// value-based-balanced and MUST keep the gate: refusing them regresses
// region-mutable-reassign-{selfref,branch,flow} and
// region-toplevel-{mutable-reassign,reassign-thunk-uaf} straight into
// UAFs. The `keeps` tests below are the counterfactual pins against
// that over-exclusion.

/// Boundary pin (counterfactual against over-exclusion): a TOP-LEVEL
/// reassigned mutable whose prior value is stored into another
/// container before the overwrite KEEPS the container model. The store
/// is runtime-counted (the push funnel increfs; the keeper's free
/// cascade decrefs), so the cell's drop-on-overwrite releases exactly
/// the cell's reference — balanced. Analyzed under the real
/// classification so the push resolves to its `Funnel` effect.
#[test]
fn reassign_gate_keeps_container_stored_value_top_level() {
    let (hir, info) = analyze_full(
        "(var keeper @[])\n\
         (var x (%pair 1 2))\n\
         (%array-push keeper x)\n\
         (assign x (%pair 3 4))\n\
         (%first x)",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of x");
    // Precondition anchoring the boundary: the funnel push records its stored
    // value at a site that is NOT one of x's assign sites.
    let assign_ids: Vec<HirId> = sites.iter().map(|(id, _)| *id).collect();
    assert!(
        info.funnel_store_sites
            .keys()
            .any(|site| !assign_ids.contains(site)),
        "precondition: the push must record a funnel store at a non-assign site"
    );
    assert!(
        sites
            .iter()
            .any(|(site, _)| info.drop_on_overwrite_sites.contains(site)),
        "a runtime-counted container store must not refuse the gate"
    );
    assert!(
        !info.suppressed_decref_regions.is_empty(),
        "a runtime-counted container store must keep the suppression"
    );
}

/// Boundary pin (counterfactual against over-exclusion): the FN-LOCAL
/// variant — the init value stored into a parameter container before a
/// conditional overwrite keeps the model. (The conditional keeps the
/// Assign alive through functionalization; a straight-line fn-local
/// reassign is rewritten into a shadowing let and never reaches the
/// gate.)
#[test]
fn reassign_gate_keeps_container_stored_value_fn_local() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (c keeper)\n\
           (begin (var x (%pair 1 2))\n\
                  (%array-push keeper x)\n\
                  (if c (assign x (%pair 3 4)) nil)\n\
                  nil)))\n\
         (h 1 @[])",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of x");
    assert!(
        sites
            .iter()
            .any(|(site, _)| info.drop_on_overwrite_sites.contains(site)),
        "a runtime-counted fn-local container store must not refuse the gate"
    );
    assert!(
        !info.suppressed_decref_regions.is_empty(),
        "a runtime-counted fn-local container store must keep the suppression"
    );
}

/// A reassigned binding a conditional `assign` gives a PHI is refused: the phi
/// is a second name for the value, so the store-site pin — which moves a
/// producer release earlier — would leave that name reading a freed value. The
/// refusal is `sole_held`'s alone; being read at the tail decides nothing, since
/// the `Return`'s mint is a reference the callee did not hold a moment earlier
/// (docs/impl/region/bindings.md § "Returned fn-local reassigned mutables").
/// Nothing may be suppressed on the fallback either: the unsuppressed baseline
/// is what releases each value's producer reference there.
#[test]
fn reassign_gate_refuses_returned_value() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (c)\n\
           (begin (var x (%pair 1 2))\n\
                  (if c (assign x (%pair 3 4)) nil)\n\
                  x)))\n\
         (h 1)",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of x");
    for (site, _) in &sites {
        assert!(
            !info.drop_on_overwrite_sites.contains(site),
            "returned value: the gate must refuse drop-on-overwrite at @{}",
            site.0
        );
    }
    assert!(
        info.suppressed_decref_regions.is_empty(),
        "returned value: no decref may be suppressed (got {:?})",
        info.suppressed_decref_regions
    );
}

/// A sole-held fn-local mutable accumulated in a LOOP and read at the tail
/// (returned) must have its own loop-carried region SUPPRESSED while its
/// assign-value region is KEPT. The loop gives the binding a loop-carried
/// region distinct from the per-iteration assign-value region, but both alias
/// the one returned value; the unsuppressed baseline would emit a value-route
/// decref for EACH at the `Return`, double-freeing the callee's single
/// reference — the second frees the caller's minted reference before the
/// caller's read (`region_capture_cell_string_accum_uaf`). Suppressing the
/// binding's own region keeps the single assign-value decref (the callee's one
/// release) and lets the `Return` mint carry ownership to the caller. Contrast
/// `reassign_gate_refuses_returned_value` (an `if`-shaped reassign is
/// phi-aliased ⇒ not sole ⇒ no model at all) and a single-assign cell, whose
/// binding and assign-value regions coalesce so there is nothing to suppress.
#[test]
fn reassign_gate_splits_returned_loop_carried_region() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (n)\n\
           (let [@acc (%pair 0 0)]\n\
             (var i 0)\n\
             (while (%lt i n)\n\
               (begin (assign acc (%pair i 7))\n\
                      (assign i (%add i 1))))\n\
             acc)))\n\
         (h 3)",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of acc");
    // `acc` is the heap-carrying reassigned mutable: it has ≥2 source regions
    // (the loop-carried binding region plus the per-iteration assign-value
    // region); the immediate `i` counter carries none.
    let acc = sites
        .iter()
        .map(|(_, b)| *b)
        .find(|b| {
            info.binding_source_regions
                .get(b)
                .is_some_and(|rs| rs.len() >= 2)
        })
        .expect("acc: a returned heap mutable with a loop-carried + assign-value region");
    let acc_regs = &info.binding_source_regions[&acc];
    let suppressed = acc_regs
        .iter()
        .filter(|r| info.suppressed_decref_regions.contains(r))
        .count();
    let kept = acc_regs.len() - suppressed;
    // The split: the loop-carried binding region is suppressed (else it
    // double-frees the returned value at the Return)…
    assert!(
        suppressed >= 1,
        "returned loop-reassigned mutable must suppress its loop-carried binding \
         region (regs={:?}, suppressed={:?})",
        acc_regs,
        info.suppressed_decref_regions
    );
    // …while at least one assign-value region is KEPT (the callee's one release,
    // which the return mint balances against the caller).
    assert!(
        kept >= 1,
        "returned loop-reassigned mutable must KEEP its assign-value region decref \
         (got all {} regions suppressed: {:?})",
        acc_regs.len(),
        acc_regs
    );
}

/// A RETURNED fn-local reassigned mutable takes the same container model an
/// unreturned one takes. The return claims the reference the `Return`'s mint
/// creates, which the callee did not hold a moment earlier; the cell's own
/// reference is the counted store's and claims nothing from anyone. So being
/// returned decides nothing about the container half — and withholding it
/// strands every value the loop displaces, one region per trip
/// (docs/impl/region/bindings.md § "Returned fn-local reassigned mutables — the
/// return claims the MINT's reference, not the cell's";
/// `tests/elle/region-loop-acc-return.lisp` measures the strand).
#[test]
fn reassign_gate_counts_a_returned_loop_accumulator() {
    let (hir, info) = returned_loop_accumulator();
    let (acc, cell) = returned_accumulator_cell(&hir, &info);
    assert!(
        !cell.forwards_content,
        "nothing forwards on from the returned link, so it keeps the content \
         drop — the release of the cell's own reference, which the `Return` mint \
         has already replaced for the caller"
    );
    assert!(
        cell.stores.value_regions().next().is_some(),
        "precondition: the loop's assign records the value it stores ({acc:?})"
    );
    for (site, b) in find_reassign_sites(&hir) {
        if b != acc {
            continue;
        }
        assert!(
            info.drop_on_overwrite_sites.contains(&site),
            "a returned cell keeps drop-on-overwrite at @{} — the channel that \
             releases each value the loop displaces",
            site.0
        );
        assert!(
            !info.donated_overwrite_sites.contains(&site),
            "a fn-local cell's store is COUNTED, whatever the tail does with the \
             content: donating at @{} would leave the producer's reference with \
             no release",
            site.0
        );
    }
}

/// A `Return` is a reader of the cell's content, and the cell's reference
/// protects it: the stored value's producer release stays pinned to its STORE
/// site rather than riding the returned-region extension out to the `Return`.
///
/// The extension exists so a returned region's release orders after the mint.
/// A cell-stored value needs nothing from it — the cell holds a counted
/// reference of its own from the store onward, and drops it at the content drop
/// the same `Return` node carries, after the mint. Left extended, the one
/// release names whatever the producer's ANF slot holds LAST, so every earlier
/// value of a loop is stranded (docs/impl/region/bindings.md § "A `Return` is a
/// reader of the cell's content").
#[test]
fn reassign_return_does_not_extend_a_cell_stored_value() {
    let (hir, info) = returned_loop_accumulator();
    let (acc, cell) = returned_accumulator_cell(&hir, &info);
    let stores: Vec<HirId> = cell.stores.sites().collect();
    assert_eq!(
        stores.len(),
        1,
        "precondition: the loop body assigns {acc:?} exactly once (got {stores:?})"
    );
    let returns = find_returns(&hir);
    assert!(
        !returns.is_empty(),
        "precondition: the shape reads the accumulator at the tail"
    );
    for r in cell.stores.value_regions() {
        let dp = info
            .region_data
            .get(&r)
            .unwrap_or_else(|| panic!("stored region {r:?} has no decref point"))
            .decref_point;
        assert!(
            !returns.contains(&dp),
            "the stored value's release rode the returned-region extension out to \
             the `Return` (@{}): one release there names only the last iteration's \
             value, stranding every earlier one",
            dp.0
        );
        assert_eq!(
            dp, stores[0],
            "a cell-stored value's producer release is pinned to the store that \
             took it (region {r:?} landed at @{})",
            dp.0
        );
    }
}

/// The returned loop accumulator: a fn-local mutable a `while` reassigns, whose
/// final content the frame hands back. Shared by the two tests above so the
/// container-model admission and the release placement are read off one shape.
fn returned_loop_accumulator() -> (Hir, RegionInfo) {
    let (hir, _, info) = pipeline(
        "(def @h (fn (n)\n\
           (let [@acc (%pair 0 0)]\n\
             (var i 0)\n\
             (while (%lt i n)\n\
               (begin (assign acc (%pair i 7))\n\
                      (assign i (%add i 1))))\n\
             acc)))\n\
         (h 3)",
    );
    (hir, info)
}

/// The accumulator binding of [`returned_loop_accumulator`] and its recorded
/// container. `acc` is the heap-carrying reassigned mutable — the immediate `i`
/// counter carries no region.
fn returned_accumulator_cell<'a>(
    hir: &Hir,
    info: &'a RegionInfo,
) -> (Binding, &'a crate::hir::region::CellContainer) {
    let acc = find_reassign_sites(hir)
        .into_iter()
        .map(|(_, b)| b)
        .find(|b| {
            info.binding_source_regions
                .get(b)
                .is_some_and(|rs| !rs.is_empty())
        })
        .expect("acc: the heap-carrying reassigned mutable");
    let cell = info.cell_containers.get(&acc).unwrap_or_else(|| {
        panic!("a returned fn-local reassigned mutable must record a container ({acc:?})")
    });
    (acc, cell)
}

/// Every `Return` node in the tree — the points `lower_return` mints at.
fn find_returns(hir: &Hir) -> Vec<HirId> {
    let mut out = Vec::new();
    fn walk(hir: &Hir, out: &mut Vec<HirId>) {
        if matches!(&hir.kind, HirKind::Return { .. }) {
            out.push(hir.id);
        }
        hir.for_each_child(|c| walk(c, out));
    }
    walk(hir, &mut out);
    out
}

/// Stay-GREEN control: the plain sole-held, not-returned top-level
/// reassign keeps the container model — drop-on-overwrite at the assign
/// and suppression of the cell's value regions. Guards the exclusion
/// against over-refusal: the cell-store edge recorded AT the assign
/// site itself must not count against the gate.
#[test]
fn reassign_gate_applies_to_sole_held_unescaped() {
    let (hir, _, info) = pipeline(
        "(var x (%pair 1 2))\n\
         (assign x (%pair 3 4))\n\
         (%first x)",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of x");
    assert!(
        sites
            .iter()
            .any(|(site, _)| info.drop_on_overwrite_sites.contains(site)),
        "sole-held unescaped reassign must keep drop-on-overwrite"
    );
    assert!(
        !info.suppressed_decref_regions.is_empty(),
        "sole-held unescaped reassign must suppress the cell's value-region decrefs"
    );
}

/// The mutated-slot backstop (docs/impl/region/bindings.md "The fallback's
/// value route is not unconditionally safe"). A top-level reassigned binding's
/// init + assign-value regions are recorded in `mutated_binding_value_regions`
/// UNCONDITIONALLY — even when the suppression gate succeeds — so the lowerer
/// never value-routes their release through the mutated cell slot. This is the
/// safety net for the no-alias-corruption UAF (region-mutable-reassign-flow
/// facet 3), where a `(deref-cell x)` read is solved to the cell's init region
/// and would otherwise route the init's decref through the reassigned slot.
#[test]
fn mutated_slot_backstop_records_top_level_reassign_value_regions() {
    let (hir, _, info) = pipeline(
        "(var x (%pair 1 2))\n\
         (assign x (%pair 3 4))\n\
         (%first x)",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of x");
    // Every region the cell's static decrefs are suppressed for must ALSO be in
    // the backstop set: the backstop is the lowerer-side guarantee that even if
    // the suppression were absent (the gate-fail fallback), the value route is
    // never emitted through the mutated slot.
    assert!(
        !info.mutated_binding_value_regions.is_empty(),
        "top-level reassign must record its value regions in the backstop set"
    );
    assert!(
        info
            .suppressed_decref_regions
            .iter()
            .all(|r| info.mutated_binding_value_regions.contains(r)),
        "every suppressed region must also be in the backstop set (got backstop={:?}, suppressed={:?})",
        info.mutated_binding_value_regions,
        info.suppressed_decref_regions,
    );
}

/// The producer-reference donation split (docs/impl/region/bindings.md
/// "Reassigned mutable bindings are 1-slot containers"). A
/// MODULE-SCOPE (file-letrec) 1-slot container suppresses its assign-value
/// region's ordinary decref, donating the producer's reference to the cell — so
/// the lowerer must NOT incref-on-store, and the site is recorded in
/// `donated_overwrite_sites`. Without that marker an unbalanced incref-on-store
/// holds every displaced prior to teardown (the reassign-in-loop over-keep).
/// Every donated site is also a drop-on-overwrite site (donation is a refinement
/// of the container model, not a separate path).
#[test]
fn donated_overwrite_marks_module_scope_reassign() {
    let (hir, _, info) = pipeline(
        "(var x (%pair 1 2))\n\
         (assign x (%pair 3 4))\n\
         (%first x)",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of x");
    assert!(
        sites
            .iter()
            .any(|(site, _)| info.donated_overwrite_sites.contains(site)),
        "a module-scope 1-slot container's overwrite must be marked donated \
         (cell adopts the producer reference — no incref-on-store)"
    );
    assert!(
        info.donated_overwrite_sites
            .iter()
            .all(|s| info.drop_on_overwrite_sites.contains(s)),
        "every donated site must also be a drop-on-overwrite site (got donated={:?}, \
         drop={:?})",
        info.donated_overwrite_sites,
        info.drop_on_overwrite_sites,
    );
}

/// A fn-local 1-slot container takes a COUNTED store and gets its own content
/// drop, whatever produced the stored value (docs/impl/region/bindings.md §
/// "Reassigned mutable bindings are 1-slot containers"). Donation is the
/// module-scope discipline alone: there the cell's reference outlives every
/// program point and the file-letrec frame teardown reclaims it, so handing the
/// producer's single reference to the cell is enough. A fn-local cell's scope
/// exits, so it needs a release of its own — which means it must hold a
/// reference of its own, which means the store is counted.
///
/// Both content producers are checked because donating either one strands or
/// over-frees: the cell would hold an uncounted reference that the producer's
/// release (pinned to the store) drops out from under it.
///
/// The conditional keeps the Assign alive through functionalization (a
/// straight-line fn-local reassign is rewritten to a shadowing let and never
/// reaches the gate).
#[test]
fn fn_local_cell_counts_its_store_for_either_content_producer() {
    for content in ["(%pair 1 2)|(%pair 3 4)", "(array 1 2)|(array 3 4)"] {
        let (init, next) = content.split_once('|').unwrap();
        let (hir, _, info) = pipeline(&format!(
            "(def @h (fn (c)\n\
               (begin (var x {init})\n\
                      (%array-push @[] x)\n\
                      (if c (assign x {next}) nil)\n\
                      nil)))\n\
             (h 1)"
        ));
        let sites = find_reassign_sites(&hir);
        assert!(!sites.is_empty(), "shape must contain a reassign of x");
        assert!(
            sites
                .iter()
                .any(|(site, _)| info.drop_on_overwrite_sites.contains(site)),
            "precondition ({content}): the fn-local container store keeps the gate"
        );
        for (site, _) in &sites {
            assert!(
                !info.donated_overwrite_sites.contains(site),
                "fn-local content ({content}) must keep the counted incref-on-store \
                 — the cell needs a reference of its own to drop at its demise \
                 (site @{})",
                site.0
            );
        }
        // The other half of the pair: the cell's reference has a demise to be
        // released at, so the final (never-overwritten) content is not stranded.
        let x = sites
            .iter()
            .map(|(_, b)| *b)
            .find(|b| info.cell_containers.contains_key(b))
            .unwrap_or_else(|| panic!("fn-local cell ({content}) must record a container"));
        let c = &info.cell_containers[&x];
        assert!(
            c.stores.value_regions().next().is_some(),
            "the container ({content}) must name the regions it may hold"
        );
    }
}

/// The scope split is STRUCTURAL (docs/impl/region/bindings.md § "Reassigned
/// mutable bindings are 1-slot containers"). A fn-local reassigned mutable living
/// in an INLINABLE callee — an immutable `def` bound to a lambda, which
/// `try_inline_call` re-walks at the call site to discover the callee's buried
/// cross-region edges — is fn-local no matter which context re-walks it. The two
/// halves of the model claim different references, so the binding must land in
/// exactly one: the module-scope half suppresses the assign-value region's
/// ordinary decref (donating the producer reference to the cell), the fn-local
/// half keeps it and takes a counted store. Both at once leaves the producer
/// reference with no release at all.
///
/// `mutated_binding_value_regions` is the observable that separates them: the
/// module-scope half records every region the cell may hold there
/// unconditionally, the fn-local half deliberately records none (its final
/// value's release IS a legitimate scope-exit slot route). So a fn-local cell
/// whose regions appear in that set was classified module-scope.
#[test]
fn reassign_scope_split_is_structural_under_inline_rewalk() {
    // `h` is immutable and lambda-bound, so the call at top level re-walks its
    // body; `x` is a genuine fn-local mutable inside it. The conditional keeps the
    // Assign alive through functionalization (a straight-line fn-local reassign is
    // rewritten into a shadowing let and never reaches the gate).
    let (hir, _, info) = pipeline(
        "(def h (fn (c)\n\
           (begin (var x (array 1 2))\n\
                  (%array-push @[] x)\n\
                  (if c (assign x (array 3 4)) nil)\n\
                  nil)))\n\
         (h 1)",
    );
    let sites = find_reassign_sites(&hir);
    let x = sites
        .iter()
        .map(|(_, b)| *b)
        .find(|b| {
            info.binding_source_regions
                .get(b)
                .is_some_and(|rs| !rs.is_empty())
        })
        .expect("shape must contain a heap-carrying reassign of x");
    let x_regs = info.binding_source_regions[&x].clone();
    for r in &x_regs {
        assert!(
            !info.mutated_binding_value_regions.contains(r),
            "fn-local cell region {:?} landed in the module-scope backstop set — the \
             inline re-walk classified a fn-local reassign as module-scope \
             (regs={:?}, backstop={:?})",
            r,
            x_regs,
            info.mutated_binding_value_regions,
        );
    }
    // The fn-local half's own obligation: the assign-value region keeps its
    // ordinary decref, so the cell's producer reference still has a release.
    assert!(
        x_regs
            .iter()
            .any(|r| !info.suppressed_decref_regions.contains(r)),
        "a fn-local 1-slot container must keep at least one region's ordinary \
         decref (regs={:?} were all suppressed)",
        x_regs,
    );
}

/// Facet A of the captured-mutable read mis-coalesce
/// (integration::file_scope::captures::test_mutable_var_mutation_visible_after_call):
/// a `(begin (var x …) …)` single-form file's `x` is a compiled Begin-pre-pass
/// CaptureCell that is RE-STORED from inside a sibling closure, so the write site
/// is in a lambda while the binding is not (`record_top_level_reassign` records
/// it in `captured_reassigns` on the binding's account, not the write site's).
/// A whole-value read through the cell is solved to the CELL's own
/// region, which must be poisoned in `mutated_binding_value_regions` so
/// `coalescible_region` refuses the static route (the return retain stays
/// value-resolved instead of resolving the cell's slot against repointed
/// content — the `AssertRegionMatches` mis-coalesce).
#[test]
fn mutated_slot_backstop_poisons_restorable_begin_cell_regions() {
    let (hir, arena, info) = pipeline(
        "(begin\n\
           (var x (%pair 1 2))\n\
           (def bump (fn () (assign x (%pair 3 4))))\n\
           (bump)\n\
           x)",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of x");
    let b = sites[0].1;
    assert!(
        arena.get(b).is_restorable_capture_cell(),
        "precondition: x must be a re-storable capture cell"
    );
    let cell_regions: Vec<Region> = info
        .begin_cell_regions
        .values()
        .flatten()
        .filter(|(bb, _)| *bb == b)
        .map(|(_, r)| *r)
        .collect();
    assert!(
        !cell_regions.is_empty(),
        "precondition: x must have a compiled Begin-pre-pass cell region"
    );
    for r in cell_regions {
        assert!(
            info.mutated_binding_value_regions.contains(&r),
            "re-storable compiled cell region {:?} must be in the mutated-slot backstop",
            r
        );
    }
}

/// Facet B: the multi-form file, where the trailing `x` read lifts into a
/// file-letrec statement wrapper `[__file_expr_N (deref-cell x)]`. The Letrec
/// arm must apply the same Rule 5 counted-reader treatment as the Let arm
/// (`counted_cell_read_regions`): the wrapper's source region must be a fresh
/// call-result placeholder (value-resolved, counted at the read), NOT the
/// init value's own region — a static route against the init region frees or
/// retains the wrong region once the cell is repointed.
#[test]
fn letrec_wrapper_read_of_restorable_cell_is_counted() {
    let (hir, _, info) = pipeline(
        "(var x (%pair 1 2))\n\
         (def bump (fn () (assign x (%pair 3 4))))\n\
         (bump)\n\
         x",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of x");
    assert!(
        !info.counted_cell_read_sites.is_empty(),
        "the file-letrec wrapper's whole-value read of the re-storable cell \
         must be a counted read (Rule 5 reader retain)"
    );
    // The init %pair's region must NOT be reachable as any OTHER binding's
    // single source region (the wrapper must not inherit it): every
    // counted-read site's placeholder is a call-result region, refused by
    // `coalescible_solver_region`.
    for site in &info.counted_cell_read_sites {
        let r = info
            .alloc_region
            .get(site)
            .expect("counted read site mints a placeholder region");
        assert!(
            info.call_result_regions.contains(r),
            "counted-read placeholder {:?} must be a call-result region",
            r
        );
    }
}

/// Facet C: the module-scope container NO closure captures, so it lives in a
/// plain slot rather than a compiled cell. Its content is re-stored all the same
/// — the top-level model donates the producer's reference and drop-on-overwrite
/// is that reference's ONLY release — so a whole-value read of it is exposed
/// exactly as a read of the celled realization is, and takes the same counted
/// reference (docs/impl/region/bindings.md § "A whole-value read of a 1-slot
/// container takes a counted reference").
///
/// Keying the reader rule on the cell rather than on the re-store would leave
/// this half uncounted, and the gate would then refuse the donation to protect
/// the alias — leaving each displaced value released by nothing but a value route
/// through the container's own mutated slot, which is no route at all.
#[test]
fn toplevel_uncelled_container_read_is_counted() {
    let (hir, arena, info) = pipeline(
        "(var x (%pair 1 2))\n\
         (def keep x)\n\
         (assign x (%pair 3 4))\n\
         (%first keep)",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain a reassign of x");
    assert!(
        !arena.get(sites[0].1).is_restorable_capture_cell(),
        "precondition: no closure captures x, so it has no compiled cell"
    );
    assert!(
        !info.counted_cell_read_sites.is_empty(),
        "a whole-value read of an uncelled module-scope container must be counted"
    );
    assert!(
        sites
            .iter()
            .any(|(site, _)| info.drop_on_overwrite_sites.contains(site)),
        "with the reader counted, the container is its init's sole holder and \
         keeps the container model"
    );
    assert!(
        !info.suppressed_decref_regions.is_empty(),
        "the donation is available again — the producer's reference becomes the \
         container's, released by drop-on-overwrite"
    );
}

/// The heap-carrying reassigned binding of a shape and its assign sites. Every
/// loop-carried-cell shape below also reassigns an immediate loop counter, whose
/// own gate always succeeds (it holds no region to be double-claimed); asserting
/// over every reassign site would read the counter's verdict instead of the
/// cell's.
fn heap_carrying_reassign(hir: &Hir, info: &RegionInfo) -> (Binding, Vec<HirId>) {
    let sites = find_reassign_sites(hir);
    let b = sites
        .iter()
        .map(|(_, b)| *b)
        .find(|b| {
            info.binding_source_regions
                .get(b)
                .is_some_and(|rs| !rs.is_empty())
        })
        .expect("shape must contain a heap-carrying reassign");
    let ids = sites
        .iter()
        .filter(|(_, s)| *s == b)
        .map(|(id, _)| *id)
        .collect();
    (b, ids)
}

/// A loop-carried fn-local cell with a HEAP init keeps the container model
/// (docs/impl/region/bindings.md § "The gate", "A loop parameter's init source
/// is not a second holder"). Functionalization gives the one source name two
/// bindings — the pre-loop version and the loop parameter its init forwards to —
/// and both record the init region as a source, so a `sole_held` that counts
/// bindings reads two holders where the program has one name and refuses the
/// whole model. The count argument is that a plain `Var` read mints nothing, so
/// the pair holds ONE reference; the region-keyed suppression then cancels both
/// names' ordinary decrefs together, leaving drop-on-overwrite as the single
/// channel. A `nil` init has no region to be double-counted, which is why
/// `reassign_gate_keeps_selfref_accumulator` never exposed this.
#[test]
fn reassign_gate_keeps_loop_carried_cell_with_heap_init() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (n)\n\
           (begin (var last (array 0 0))\n\
                  (var i 0)\n\
                  (while (%lt i n)\n\
                    (begin (assign last (array i 7))\n\
                           (assign i (%add i 1))))\n\
                  0)))\n\
         (h 3)",
    );
    let (last, last_sites) = heap_carrying_reassign(&hir, &info);
    assert!(
        info.binding_source_regions[&last].len() >= 2,
        "precondition: a heap-init cell holds its init region plus its \
         assign-value one (got {:?})",
        info.binding_source_regions[&last],
    );
    assert!(
        info.cell_containers.contains_key(&last),
        "a loop-carried cell with a heap init must record a container (so its \
         final content has a demise to be dropped at)"
    );
    assert!(
        last_sites
            .iter()
            .any(|site| info.drop_on_overwrite_sites.contains(site)),
        "a loop-carried cell with a heap init must keep drop-on-overwrite — the \
         channel that releases every displaced prior"
    );
    // The init region is the one the loop's init edge forwards: it carries no
    // producer release of its own once the cell claims it, so it is suppressed
    // while the assign-value region's decref (the producer's, pinned to the
    // store) stays.
    let regs = &info.binding_source_regions[&last];
    assert!(
        regs.iter()
            .any(|r| info.suppressed_decref_regions.contains(r)),
        "the forwarded init region's ordinary decref must be suppressed \
         (regs={regs:?}, suppressed={:?})",
        info.suppressed_decref_regions,
    );
    // Donation and counted init are the two alternatives, so a donated init
    // records no retain: the cell takes the producer's reference, and a retain
    // on top of the suppression would hold the value to teardown.
    assert!(
        info.counted_cell_init_sites.is_empty(),
        "a donated init takes no retain (got {:?})",
        info.counted_cell_init_sites,
    );
}

/// A GENUINE alias of the INIT costs the DONATION, not the model
/// (docs/impl/region/bindings.md § "What the cell donates it must hold alone;
/// what it counts it need not"). `xs` is a different source name bound to the
/// same value, not the loop's own init-forwarding edge, so the pair really is
/// two holders — and suppressing the init region, which is keyed by region,
/// would cancel `xs`'s own decref and free the value under a read that
/// outlives the first overwrite. The cell counts its init instead: a retain at
/// the binder's store, balanced by the same drop-on-overwrite that balances
/// every later store, with nothing suppressed and nothing claimed twice.
///
/// `xs` ALLOCATES the value and the cell's init merely reads it, which is the
/// ordering this route serves: the alias's ordinary decref routes through the
/// allocating binder's slot, and that slot is `xs`'s, which no `assign` repoints.
/// The opposite ordering — the cell's own binder allocating and the alias reading
/// out of it — has no such slot, and the alias takes a counted reference of its
/// own instead (`reassign_gate_counts_a_read_of_an_uncelled_cell`).
///
/// Refusing outright would cost the store-site pin as well, so each stored
/// value's release would ride the cell binding's uses out past the loop — one
/// release for a region that names a different runtime value every iteration
/// (`tests/elle/region-cell-aliased-init.lisp`).
#[test]
fn reassign_gate_counts_an_aliased_init() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (n)\n\
           (begin (var xs (array 0 0))\n\
                  (var last xs)\n\
                  (var i 0)\n\
                  (while (%lt i n)\n\
                    (begin (assign last (array i 7))\n\
                           (assign i (%add i 1))))\n\
                  (%length xs))))\n\
         (h 3)",
    );
    let (last, last_sites) = heap_carrying_reassign(&hir, &info);
    assert!(
        info.cell_containers.contains_key(&last),
        "an aliased init must still take the container model — the store-site \
         pin it carries is what keeps a loop's releases inside the loop"
    );
    assert!(
        last_sites
            .iter()
            .any(|site| info.drop_on_overwrite_sites.contains(site)),
        "an aliased init keeps drop-on-overwrite: that is the release of the \
         cell's OWN counted reference, not of the alias's"
    );
    let regs = &info.binding_source_regions[&last];
    for r in regs {
        assert!(
            !info.suppressed_decref_regions.contains(r),
            "a counted init suppresses nothing — the alias keeps the decref that \
             releases the producer's reference (region {r:?} of {regs:?})",
        );
    }
    assert_eq!(
        info.counted_cell_init_sites.len(),
        1,
        "exactly one retain, at the chain source's binder store (got {:?})",
        info.counted_cell_init_sites,
    );
}

/// A whole-value read of an UNCELLED 1-slot container takes a counted reference,
/// exactly as a read of the celled realization does (docs/impl/region/bindings.md
/// § "A whole-value read of a 1-slot container takes a counted reference"). The
/// container releases what it held at every overwrite — here the compiler's own
/// drop-on-overwrite rather than `capture_store_with_rebind` — so `keep` borrows
/// a reference that dies at the first `(assign last …)`, and the release the
/// borrow needs is one no other name can supply.
///
/// The reference `keep` takes is its own, so `keep` is no longer a holder of the
/// init region and the cell donates its init as an unaliased one would. That is
/// what makes the shape reclaimable at all: the counted-init route would leave
/// the producer's reference to be released through the slot recorded for the init
/// region, which here is the CELL's own — a mutated slot, and no release route.
///
/// The discriminator is the ordering. `reassign_gate_counts_an_aliased_init`
/// above is the same program with the alias bound FIRST, so the alias allocates,
/// its own untainted slot carries the release, and the counted-init route runs
/// instead.
#[test]
fn reassign_gate_counts_a_read_of_an_uncelled_cell() {
    let (hir, arena, info) = pipeline(
        "(def @h (fn (n)\n\
           (let [@last (array 0 0)]\n\
             (let [keep last]\n\
               (var i 0)\n\
               (while (%lt i n)\n\
                 (begin (assign last (array i 7))\n\
                        (assign i (%add i 1))))\n\
               (%length keep)))))\n\
         (h 3)",
    );
    let (last, last_sites) = heap_carrying_reassign(&hir, &info);
    assert!(
        !arena.get(last).is_restorable_capture_cell(),
        "precondition: the container must be UNCELLED — no closure captures it, \
         so its content is re-stored by the compiler's drop-on-overwrite"
    );
    assert!(
        !info.counted_cell_read_sites.is_empty(),
        "a whole-value read of a 1-slot container takes a counted reference \
         whether or not the container is celled"
    );
    // Every counted read is value-resolved: the placeholder rides
    // `call_result_regions`, so the reader releases through its OWN slot rather
    // than inheriting the container's static source region.
    for site in &info.counted_cell_read_sites {
        let r = info
            .alloc_region
            .get(site)
            .expect("counted read site mints a placeholder region");
        assert!(
            info.call_result_regions.contains(r),
            "counted-read placeholder {r:?} must be a call-result region",
        );
    }
    assert!(
        info.cell_containers.contains_key(&last),
        "the container model still runs — the counted read changes who holds the \
         init, not whether the cell is a container"
    );
    assert!(
        last_sites
            .iter()
            .any(|site| info.drop_on_overwrite_sites.contains(site)),
        "drop-on-overwrite is the release of the reference the cell was donated"
    );
    assert!(
        info.counted_cell_init_sites.is_empty(),
        "with the reader counted the cell is the init's sole holder, so the \
         donation is available and no init retain is needed (got {:?})",
        info.counted_cell_init_sites,
    );
    let regs = &info.binding_source_regions[&last];
    assert!(
        regs.iter()
            .any(|r| info.suppressed_decref_regions.contains(r)),
        "the donated init region's ordinary decref is suppressed — its release is \
         the cell's drop-on-overwrite (regs={regs:?})",
    );
}

/// A BRANCH whose every arm is a whole-value read of a 1-slot container is a
/// whole-value read (docs/impl/region/bindings.md § "A branch whose every arm is
/// such a read is one too"). What obliges the reader is the value it holds, not
/// the syntax that selected it: `keep` names, on every path, a borrow out of a
/// container that re-stores, and one `IncrefValueRegion` at the binder covers
/// every arm because it names the runtime value.
///
/// The discriminator against
/// `reassign_gate_counts_a_read_of_an_uncelled_cell` is the branch alone — the
/// same program with the alias's init wrapped in an `if` both of whose arms read
/// the container. Reading the branch as an ordinary alias instead leaves `keep` a
/// holder of the init region, so the container falls back to the counted-init
/// route, whose release routes through the CELL's own slot — a mutated slot, and
/// no release route, so the init strands per call
/// (`tests/elle/region-cell-alias-branch.lisp`).
#[test]
fn reassign_gate_counts_a_branch_read_of_a_container() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (n)\n\
           (let [@last (array 0 0)]\n\
             (let [keep (if (%lt n 0) last last)]\n\
               (var i 0)\n\
               (while (%lt i n)\n\
                 (begin (assign last (array i 7))\n\
                        (assign i (%add i 1))))\n\
               (%length keep)))))\n\
         (h 3)",
    );
    let (last, last_sites) = heap_carrying_reassign(&hir, &info);
    assert!(
        !info.counted_cell_read_sites.is_empty(),
        "a branch every arm of which reads the container is a whole-value read"
    );
    // Value-resolved exactly as a bare read is: the placeholder rides
    // `call_result_regions`, so the reader releases through its OWN slot.
    for site in &info.counted_cell_read_sites {
        let r = info
            .alloc_region
            .get(site)
            .expect("counted read site mints a placeholder region");
        assert!(
            info.call_result_regions.contains(r),
            "counted-read placeholder {r:?} must be a call-result region",
        );
    }
    assert!(
        last_sites
            .iter()
            .any(|site| info.drop_on_overwrite_sites.contains(site)),
        "the container keeps the model — drop-on-overwrite releases the \
         reference it was donated"
    );
    assert!(
        info.counted_cell_init_sites.is_empty(),
        "with the branch counted the container is its init's sole holder, so the \
         donation is available and no init retain is needed (got {:?})",
        info.counted_cell_init_sites,
    );
    let regs = &info.binding_source_regions[&last];
    assert!(
        regs.iter()
            .any(|r| info.suppressed_decref_regions.contains(r)),
        "the donated init region's ordinary decref is suppressed (regs={regs:?})",
    );
}

/// A MIXED branch — one arm reading the container, one allocating — takes the
/// counted read too, and pays for the allocating arm by KEEPING that arm's source
/// regions (docs/impl/region/bindings.md § "A branch is a read of whichever arms
/// read"). The replacement is per-arm: the reader stops holding the container's
/// regions, so the donation runs, while the allocating arm's region stays in the
/// reader's set — it is the only thing extending that value's last use out to the
/// binder's retain.
#[test]
fn reassign_gate_counts_a_mixed_branch_init() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (n)\n\
           (let [@last (array 0 0)]\n\
             (let [keep (if (%lt n 0) last (array 5 5))]\n\
               (var i 0)\n\
               (while (%lt i n)\n\
                 (begin (assign last (array i 7))\n\
                        (assign i (%add i 1))))\n\
               (%length keep)))))\n\
         (h 3)",
    );
    let (last, last_sites) = heap_carrying_reassign(&hir, &info);
    assert_eq!(
        info.counted_cell_read_sites.len(),
        1,
        "the arm that reads the container makes the branch a counted read \
         (got {:?})",
        info.counted_cell_read_sites,
    );
    let site = *info.counted_cell_read_sites.iter().next().unwrap();
    let placeholder = *info
        .alloc_region
        .get(&site)
        .expect("counted read site mints a placeholder region");
    assert!(
        info.call_result_regions.contains(&placeholder),
        "counted-read placeholder {placeholder:?} must be a call-result region",
    );
    // The reader: the one binding the placeholder was minted for.
    let reader = *info
        .binding_source_regions
        .iter()
        .find(|(_, rs)| rs.contains(&placeholder))
        .expect("the placeholder is the reader's source region")
        .0;
    let reader_regs = &info.binding_source_regions[&reader];
    let last_regs = &info.binding_source_regions[&last];
    assert!(
        reader_regs.iter().any(|r| *r != placeholder),
        "the allocating arm's region stays in the reader's source set — cutting \
         it would put that arm's own release ahead of the binder's retain \
         (regs={reader_regs:?})",
    );
    assert!(
        !reader_regs.iter().any(|r| last_regs.contains(r)),
        "the container's regions are withdrawn from the reader, which is what \
         hands the donation back (reader={reader_regs:?}, container={last_regs:?})",
    );
    assert!(
        last_sites
            .iter()
            .any(|site| info.drop_on_overwrite_sites.contains(site)),
        "the container keeps the model — drop-on-overwrite releases the \
         reference it was donated"
    );
    assert!(
        info.counted_cell_init_sites.is_empty(),
        "with the reading arm counted the container is its init's sole holder, \
         so the donation is available and no init retain is needed (got {:?})",
        info.counted_cell_init_sites,
    );
    assert!(
        last_regs
            .iter()
            .any(|r| info.suppressed_decref_regions.contains(r)),
        "the donated init region's ordinary decref is suppressed \
         (regs={last_regs:?})",
    );
}

/// A statement wrapper around the read is descended too: a `Begin`'s value is
/// its tail's, so the reader ends up holding what the tail read and takes the
/// same counted reference (docs/impl/region/bindings.md § "A branch is a read of
/// whichever arms read").
#[test]
fn reassign_gate_counts_a_begin_wrapped_read() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (n)\n\
           (let [@last (array 0 0)]\n\
             (let [keep (begin 1 last)]\n\
               (var i 0)\n\
               (while (%lt i n)\n\
                 (begin (assign last (array i 7))\n\
                        (assign i (%add i 1))))\n\
               (%length keep)))))\n\
         (h 3)",
    );
    let (last, _) = heap_carrying_reassign(&hir, &info);
    assert_eq!(
        info.counted_cell_read_sites.len(),
        1,
        "the begin's tail is the read the binder counts (got {:?})",
        info.counted_cell_read_sites,
    );
    assert!(
        info.counted_cell_init_sites.is_empty(),
        "with the read counted the container is its init's sole holder, so the \
         donation is available (got {:?})",
        info.counted_cell_init_sites,
    );
    let last_regs = &info.binding_source_regions[&last];
    assert!(
        last_regs
            .iter()
            .any(|r| info.suppressed_decref_regions.contains(r)),
        "the donated init region's ordinary decref is suppressed \
         (regs={last_regs:?})",
    );
}

/// Counterfactual against over-admission: a branch NO arm of which reads a
/// container is a read of nothing, so the reader keeps every source region it
/// had and the container falls back to counting its init. Nothing here obliges a
/// retain — neither arm's value can be freed by the container's next overwrite.
#[test]
fn reassign_gate_declines_a_branch_reading_no_container() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (n)\n\
           (let [@last (array 0 0)]\n\
             (let [other (array 1 1)]\n\
               (let [keep (if (%lt n 0) other (array 5 5))]\n\
                 (var i 0)\n\
                 (while (%lt i n)\n\
                   (begin (assign last (array i 7))\n\
                          (assign i (%add i 1))))\n\
                 (%add (%length keep) (%length last)))))))\n\
         (h 3)",
    );
    let (_, last_sites) = heap_carrying_reassign(&hir, &info);
    assert!(
        info.counted_cell_read_sites.is_empty(),
        "neither arm reads a 1-slot container, so there is nothing to count \
         (got {:?})",
        info.counted_cell_read_sites,
    );
    assert!(
        last_sites
            .iter()
            .any(|site| info.drop_on_overwrite_sites.contains(site)),
        "the container still takes the model — what the branch decides is who \
         holds the init, not whether the container is a container"
    );
}

/// Counterfactual against over-admission: an aliased STORED value still refuses
/// the model whole. The model moves each stored value's producer release back to
/// the store site, and `v` is a second name reading the same value — a release
/// pinned to the store would fire under it. Only the init, whose claim the cell
/// can replace with a counted reference of its own, is exempt from the
/// sole-held question.
#[test]
fn reassign_gate_refuses_an_aliased_assign_value() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (n)\n\
           (begin (var last (array 0 0))\n\
                  (var i 0)\n\
                  (while (%lt i n)\n\
                    (let [v (array i 7)]\n\
                      (assign last v)\n\
                      (%length v)\n\
                      (assign i (%add i 1))))\n\
                  0)))\n\
         (h 3)",
    );
    let (last, last_sites) = heap_carrying_reassign(&hir, &info);
    for site in &last_sites {
        assert!(
            !info.drop_on_overwrite_sites.contains(site),
            "an aliased stored value must refuse the container model at @{}",
            site.0
        );
    }
    assert!(
        !info.cell_containers.contains_key(&last),
        "a refused cell records no container — the store-site pin it carries \
         would move a release ahead of the alias's read"
    );
    assert!(
        info.counted_cell_init_sites.is_empty(),
        "a refused cell takes no init retain (got {:?})",
        info.counted_cell_init_sites,
    );
}

/// Two sequential loops over one binding, with an extra `keep` alias or a
/// trailing read spliced in by the caller. Every shape below is the same chain:
/// functionalization gives the source name one version per loop and initializes
/// each from the previous, so the middle version carries a cell of its own
/// (docs/impl/region/bindings.md § "A chain of forwarding edges hands one
/// reference along, so the fold follows it whole").
fn two_loop_chain(between: &str, tail: &str) -> (Hir, RegionInfo) {
    let (hir, _, info) = pipeline(&format!(
        "(def @h (fn (n)\n\
           (begin (var last (array 0 0))\n\
                  (var i 0)\n\
                  (while (%lt i n)\n\
                    (begin (assign last (array i 7))\n\
                           (assign i (%add i 1))))\n\
                  {between}\n\
                  (while (%lt i (%mul n 2))\n\
                    (begin (assign last (array i 9))\n\
                           (assign i (%add i 1))))\n\
                  {tail})))\n\
         (h 3)"
    ));
    (hir, info)
}

/// The chain's links, upstream first — the heap-carrying reassigned bindings, in
/// tree order. The immediate loop counter carries no region and is not one.
fn chain_links(hir: &Hir, info: &RegionInfo) -> Vec<Binding> {
    let mut links: Vec<Binding> = Vec::new();
    for (_, b) in find_reassign_sites(hir) {
        let heap = info
            .binding_source_regions
            .get(&b)
            .is_some_and(|rs| !rs.is_empty());
        if heap && !links.contains(&b) {
            links.push(b);
        }
    }
    links
}

/// Two sequential loops over one cell chain the forwarding
/// (`last#2 ← last#1 ← last#0`), and the fold follows the chain to its last
/// version: a plain `Var` init mints nothing, so the three versions hold ONE
/// reference between them. Both links take the container model, and the
/// reference has exactly one release channel at a time — the upstream link
/// FORWARDS its content drop to the link that receives the reference, keeping
/// only drop-on-overwrite for the priors it displaces.
///
/// The suppression is read over the whole chain: each link keeps its own
/// assign-value regions' decrefs (one producer release per stored value) and
/// only the shared init region is suppressed. Suppressing the upstream link's
/// value regions — which the downstream link's source set contains, because the
/// `Loop` init copies them — would leave every value that link displaced with a
/// store incref and no producer release.
#[test]
fn reassign_gate_keeps_loop_carried_cell_forwarded_from_a_cell() {
    let (hir, info) = two_loop_chain("", "0");
    let links = chain_links(&hir, &info);
    assert_eq!(
        links.len(),
        2,
        "precondition: two sequential loops give the cell two forwarding links \
         (got {links:?})"
    );
    let (up, down) = (links[0], links[1]);
    let up_regs = info.binding_source_regions[&up].clone();
    let down_regs = info.binding_source_regions[&down].clone();
    assert!(
        up_regs.iter().all(|r| down_regs.contains(r)) && down_regs.len() > up_regs.len(),
        "precondition: the downstream link's source regions are the upstream \
         link's plus its own (up={up_regs:?}, down={down_regs:?})"
    );

    let up_cell = info
        .cell_containers
        .get(&up)
        .expect("the upstream link must take the container model");
    let down_cell = info
        .cell_containers
        .get(&down)
        .expect("the downstream link must take the container model");
    assert!(
        up_cell.forwards_content,
        "the upstream link hands its content drop to the link it forwards into \
         — emitting one here would release the forwarded reference twice"
    );
    assert!(
        !down_cell.forwards_content,
        "the last link of the chain keeps the content drop: nothing forwards on \
         from it, so its final content has no other release"
    );

    // Every link's own assign-value regions keep their decrefs — those are the
    // producer releases, pinned to the store sites.
    let kept: Vec<Region> = up_cell
        .stores
        .value_regions()
        .chain(down_cell.stores.value_regions())
        .collect();
    assert!(
        kept.len() >= 2,
        "precondition: each loop stores a value region of its own (kept={kept:?})"
    );
    for r in &kept {
        assert!(
            !info.suppressed_decref_regions.contains(r),
            "region {r:?} is a link's assign-value region: suppressing it strands \
             the producer's reference of every value that link displaced",
        );
    }
    // …and the region no link stores into — the shared init, forwarded down the
    // chain uncounted — is the one that is suppressed.
    let init: Vec<Region> = up_regs
        .iter()
        .copied()
        .filter(|r| !kept.contains(r))
        .collect();
    assert_eq!(
        init.len(),
        1,
        "precondition: the chain shares exactly one init region (up={up_regs:?}, \
         kept={kept:?})"
    );
    assert!(
        info.suppressed_decref_regions.contains(&init[0]),
        "the forwarded init region's ordinary decref must be suppressed — \
         drop-on-overwrite is its one release (suppressed={:?})",
        info.suppressed_decref_regions,
    );

    for (site, b) in find_reassign_sites(&hir) {
        if b != up && b != down {
            continue;
        }
        assert!(
            info.drop_on_overwrite_sites.contains(&site),
            "every link of an admitted chain keeps drop-on-overwrite at @{} — \
             the channel that releases each displaced prior",
            site.0
        );
    }
}

/// Counterfactual against over-admission: the chain is admitted or declined
/// WHOLE. A genuine alias of a middle link — `(var keep last)` between the two
/// loops — is a second name holding the reference, so no link may claim it.
/// Declining only that link would leave the next one's drop-on-overwrite
/// releasing a reference the baseline already released at its ordinary decref.
///
/// The alias sits after the first loop, so it names that loop's STORED value
/// and not merely the chain's init: the counted-init route cannot rescue it,
/// because the model would still pin that value's release to a store site the
/// alias's read outlives.
#[test]
fn reassign_gate_refuses_forwarding_chain_with_an_aliased_link() {
    let (hir, info) = two_loop_chain("(var keep last)", "(%length keep)");
    let links = chain_links(&hir, &info);
    assert!(
        links.len() >= 2,
        "precondition: the shape still chains two links (got {links:?})"
    );
    for (site, b) in find_reassign_sites(&hir) {
        if !links.contains(&b) {
            continue;
        }
        assert!(
            !info.drop_on_overwrite_sites.contains(&site),
            "an aliased link declines the whole chain at @{} — the alias holds a \
             reference a link would claim a second time",
            site.0
        );
    }
    for b in &links {
        for r in &info.binding_source_regions[b] {
            assert!(
                !info.suppressed_decref_regions.contains(r),
                "a declined chain must suppress nothing (region {r:?} of {b:?} \
                 was suppressed)",
            );
        }
    }
}

/// A chain whose LAST link is returned is admitted exactly as an unreturned one
/// is, and keeps the same one-reference-one-channel shape: the upstream link
/// forwards its content drop on, and the last link keeps it. The return claims
/// the reference the `Return`'s mint creates, not the one the chain forwards, so
/// the last link's content drop — emitted after that mint — is still the release
/// of the chain's own reference (docs/impl/region/bindings.md § "Returned
/// fn-local reassigned mutables — the return claims the MINT's reference, not
/// the cell's").
#[test]
fn reassign_gate_counts_a_forwarding_chain_whose_last_link_is_returned() {
    let (hir, info) = two_loop_chain("", "last");
    let links = chain_links(&hir, &info);
    assert_eq!(
        links.len(),
        2,
        "precondition: the shape chains two links (got {links:?})"
    );
    let up = info
        .cell_containers
        .get(&links[0])
        .expect("the upstream link takes the container model");
    let down = info
        .cell_containers
        .get(&links[1])
        .expect("the returned last link takes the container model");
    assert!(
        up.forwards_content,
        "the upstream link hands its content drop to the link it forwards into"
    );
    assert!(
        !down.forwards_content,
        "the returned last link keeps the content drop — the release of the \
         chain's one reference, which the `Return` mint has already replaced for \
         the caller"
    );
    for (site, b) in find_reassign_sites(&hir) {
        if !links.contains(&b) {
            continue;
        }
        assert!(
            info.drop_on_overwrite_sites.contains(&site),
            "every link of an admitted chain keeps drop-on-overwrite at @{}",
            site.0
        );
    }
}

/// Stay-GREEN control: the self-referential accumulator
/// `(assign acc (%pair i acc))` stays gated. Its only cross-region
/// edges are the model's own (the cell-store edge at the assign site;
/// value-succession collapses to a self-edge in a loop's single static
/// region), so the exclusion must not refuse it — refusing would
/// reintroduce the unsuppressed baseline's per-iteration over-keep for
/// the canonical accumulation idiom (region-mutable-reassign-selfref).
#[test]
fn reassign_gate_keeps_selfref_accumulator() {
    let (hir, _, info) = pipeline(
        "(var acc ())\n\
         (var i 0)\n\
         (while (%lt i 3) (begin (assign acc (%pair i acc)) (assign i (%add i 1))))\n\
         (length acc)",
    );
    let sites = find_reassign_sites(&hir);
    assert!(!sites.is_empty(), "shape must contain reassigns");
    assert!(
        sites
            .iter()
            .any(|(site, _)| info.drop_on_overwrite_sites.contains(site)),
        "self-referential accumulator must keep drop-on-overwrite"
    );
    assert!(
        !info.suppressed_decref_regions.is_empty(),
        "self-referential accumulator must keep its suppression"
    );
}

/// A value stored into a 1-slot container is released at its STORE site, and a
/// reader the cell's own reference already outlives may not drag that release
/// forward — neither a cell binding's uses nor an uncounted opcode read of the
/// cell (`%get`/`%first`/`%rest`), whose borrow the cell protects. Both routes
/// reach past the loop that stores a fresh value every iteration, so one release
/// would cover N allocations (docs/impl/region/bindings.md § "A chain of
/// forwarding edges hands one reference along, so the fold follows it whole").
#[test]
fn a_cell_stored_value_is_not_extended_by_a_read_of_the_cell() {
    // The read sits in statement position: an uncounted read in TAIL position
    // returns a borrow out of the container, which transfers the cell's
    // reference and refuses the model outright.
    let (hir, info) = two_loop_chain("", "(begin (%get last 1) 0)");
    let links = chain_links(&hir, &info);
    assert_eq!(links.len(), 2, "precondition: the chain has two links");
    let read_regions: Vec<Region> = info
        .uncounted_read_sites
        .values()
        .flatten()
        .copied()
        .collect();
    assert!(
        !read_regions.is_empty(),
        "precondition: the `%get` records an uncounted read of the cell"
    );
    for b in &links {
        let cell = info
            .cell_containers
            .get(b)
            .unwrap_or_else(|| panic!("precondition: {b:?} takes the container model"));
        let store = cell
            .stores
            .sites()
            .last()
            .expect("the link stores at least once");
        for r in &cell.stores.value_region_set() {
            assert!(
                read_regions.contains(r),
                "precondition: the read names the cell's stored value region {r:?}"
            );
            assert_eq!(
                info.region_data[r].decref_point, store,
                "a stored value's release stays at its store site: the reader \
                 borrows through the cell's own reference, and one release at \
                 the read cannot cover a loop's worth of stores",
            );
        }
    }
}

/// A stored value's producer release is pinned to the store that took **that**
/// value, not to the cell's latest store. Two `assign`s in mutually exclusive
/// arms of a branch inside a loop are the ordinary shape: reading the cell's
/// stores as one set pins the first arm's value inside the SECOND arm, so an
/// iteration taking the first arm again displaces the previous value from its own
/// ANF slot before that pin ever runs — one stranded region per repeat, growing
/// with the iteration count (docs/impl/region/bindings.md § "The store site is
/// the store that took THAT value"; `tests/elle/region-cell-arm-store.lisp`).
///
/// Stated over the program rather than over the container's fields: each stored
/// value's release must land inside the subtree of the `assign` that stored it.
#[test]
fn a_stored_value_is_pinned_to_the_store_that_took_it() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (n)\n\
           (begin (var last (array 0 0))\n\
                  (var i 0)\n\
                  (while (%lt i n)\n\
                    (begin (if (%lt i 2)\n\
                             (assign last (array i 7))\n\
                             (assign last (array i 9)))\n\
                           (assign i (%add i 1))))\n\
                  0)))\n\
         (h 8)",
    );
    // The heap-carrying cell of the loop, and the two arms that store into it.
    let (cell_binding, stores) = info
        .cell_containers
        .iter()
        .map(|(b, c)| (*b, c.stores.len()))
        .find(|&(_, n)| n > 1)
        .expect("precondition: one cell is stored into from both arms");
    assert_eq!(
        stores, 2,
        "precondition: the branch gives the cell exactly two store sites"
    );
    let sites: Vec<HirId> = find_reassign_sites(&hir)
        .into_iter()
        .filter(|&(_, b)| b == cell_binding)
        .map(|(id, _)| id)
        .collect();
    assert_eq!(sites.len(), 2, "precondition: two assigns name the cell");

    // Each arm allocates the value it stores, so the region born under one
    // `assign` must be released under that same `assign`.
    for site in sites {
        let born: Vec<Region> = info
            .alloc_region
            .iter()
            .filter(|(alloc_id, _)| subtree_contains(&hir, site, **alloc_id))
            .map(|(_, &r)| r)
            .collect();
        assert!(
            !born.is_empty(),
            "precondition: the assign at @{} allocates the value it stores",
            site.0
        );
        for r in born {
            let Some(d) = info.region_data.get(&r) else {
                continue;
            };
            assert!(
                subtree_contains(&hir, site, d.decref_point),
                "a value allocated under the assign at @{} is released at @{}, \
                 outside that assign: the pin followed the cell's LAST store \
                 instead of the store that took this value, so every path that \
                 does not reach the last store strands it",
                site.0,
                d.decref_point.0,
            );
        }
    }
}

/// True when `id` is `root` or lies in `root`'s subtree.
fn subtree_contains(hir: &Hir, root: HirId, id: HirId) -> bool {
    fn walk(hir: &Hir, root: HirId, id: HirId, inside: bool) -> bool {
        let inside = inside || hir.id == root;
        if inside && hir.id == id {
            return true;
        }
        let mut found = false;
        hir.for_each_child(|c| found |= walk(c, root, id, inside));
        found
    }
    walk(hir, root, id, false)
}
