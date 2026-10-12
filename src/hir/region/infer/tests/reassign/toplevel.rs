// audited: 2026-10-11
//! The module-scope half of the gate: what the cell donates, what it counts, and what each records.
//!
//! docs/impl/region/bindings.md

use super::*;

/// A module-scope cell with a `%pair` init, reassigned in a loop by `assign`,
/// and read once after the loop.
fn module_scope_cell(assign: &str) -> (Hir, RegionInfo) {
    let (hir, _, info) = pipeline(&format!(
        "(var s (%pair 0 0))\n\
         (var i 0)\n\
         (while (%lt i 3)\n\
           (begin {assign}\n\
                  (assign i (%add i 1))))\n\
         (length s)"
    ));
    (hir, info)
}

/// A fresh local allocation stored into a module-scope cell is DONATED: the
/// stored region's ordinary decref is suppressed, the site emits no
/// incref-on-store, and the cell records no container
/// (docs/impl/region/bindings.md § "Module scope"). The control the counted
/// store below is measured against.
#[test]
fn toplevel_fresh_allocation_store_is_donated() {
    let (hir, info) = module_scope_cell("(assign s (%pair i 7))");
    let (s, sites) = heap_carrying_reassign(&hir, &info);
    for site in &sites {
        assert!(
            info.drop_on_overwrite_sites.contains(site),
            "a module-scope cell takes drop-on-overwrite"
        );
        assert!(
            info.donated_overwrite_sites.contains(site),
            "a fresh allocation's store is donated, so the site takes no incref"
        );
    }
    for r in &info.binding_source_regions[&s] {
        assert!(
            info.suppressed_decref_regions.contains(r),
            "a donated region's ordinary decref is suppressed (region {r:?})"
        );
    }
    assert!(
        !info.cell_containers.contains_key(&s),
        "a donating cell pins nothing to a store site, so it records no container"
    );
}

/// A CALL RESULT stored into a module-scope cell takes the COUNTED store: the
/// site keeps drop-on-overwrite and is not donated, the stored region keeps its
/// ordinary decref — the producer's release, which the container entry pins to
/// the store site — and the cell records a container with no content drop,
/// its final content being the file-letrec teardown's
/// (docs/impl/region/bindings.md § "Module scope").
///
/// The counter-factual is the leak this pins: with the stored region in
/// `suppressed_decref_regions` beside the counted store, the cell's reference
/// has no balancing release, one region per trip of the loop.
#[test]
fn toplevel_call_result_store_keeps_the_producer_release() {
    let (hir, info) = module_scope_cell("(assign s (array i 7))");
    let (s, sites) = heap_carrying_reassign(&hir, &info);
    for site in &sites {
        assert!(
            info.drop_on_overwrite_sites.contains(site),
            "a counted module-scope cell still takes drop-on-overwrite"
        );
        assert!(
            !info.donated_overwrite_sites.contains(site),
            "a call result's store is counted, never donated"
        );
    }
    let cell = info
        .cell_containers
        .get(&s)
        .expect("a counted module-scope cell records a container for the store-site pin");
    assert!(
        !cell.drops_content(),
        "a module-scope cell's final content is the teardown's, so it emits no \
         content drop"
    );
    let stored: Vec<Region> = cell.stores.value_regions().collect();
    assert!(
        !stored.is_empty(),
        "the container names the regions it stores"
    );
    for r in &stored {
        assert!(
            info.call_result_regions.contains(r),
            "precondition: the stored value is a call result (region {r:?})"
        );
        assert!(
            !info.suppressed_decref_regions.contains(r),
            "a counted store keeps the stored region's ordinary decref, the \
             producer's release (region {r:?})"
        );
        assert!(
            info.cell_stored_regions.contains(r),
            "a counted store's region is a runtime fact, recorded as a stored \
             region (region {r:?})"
        );
    }
    // The init alone is donated: it is the one region the cell holds that no
    // store produced.
    let init: Vec<Region> = info.binding_source_regions[&s]
        .iter()
        .copied()
        .filter(|r| !stored.contains(r))
        .collect();
    assert!(
        !init.is_empty(),
        "precondition: the %pair init has a region"
    );
    for r in &init {
        assert!(
            info.suppressed_decref_regions.contains(r),
            "the init is stored uncounted at the define, so its decref is \
             suppressed and drop-on-overwrite releases it (region {r:?})"
        );
    }
}
