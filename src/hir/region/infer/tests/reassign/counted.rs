// audited: 2026-09-22
//! The counted-init route: what an aliased init costs the cell, and what the mutated-slot backstop keeps off a cell's slot.
//!
//! docs/impl/region/bindings.md

use super::*;

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
/// reference (docs/impl/region/reads.md § "A whole-value read of a 1-slot
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
