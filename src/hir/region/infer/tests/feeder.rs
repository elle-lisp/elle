// audited: 2026-09-21
//! The store's feeder: a name whose only job is to carry a value into a
//! reassigned binding, and what disqualifies one.
//!
//! docs/impl/region/bindings.md

use super::*;

/// The shape both tests drive: a fn-local `last` reassigned from a `let`-bound
/// `v`, with the binder's position and the trailing read left to the caller.
/// Answers the reassigned heap-carrying binding and its store sites.
fn one_binder_shape(body: &str) -> (RegionInfo, Binding, Vec<HirId>) {
    let (hir, _, info) = pipeline(&format!(
        "(def @h (fn (n) (begin (var last (array 0 0)) {body} (%length last))))\n\
         (h 3)"
    ));
    let sites = find_reassign_sites(&hir);
    let last = sites
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
        .filter(|(_, b)| *b == last)
        .map(|(id, _)| *id)
        .collect();
    (info, last, ids)
}

/// A name whose only job is to carry the value INTO the store is the store's
/// FEEDER, and the gate reads it as no holder of that value
/// (docs/impl/region/bindings.md § "A name the store consumes is not a second
/// holder of the value"). Nothing reads `v` after the store, so the store-site
/// pin lands where `v` dies and the sole-held question has nothing to protect.
///
/// The counter-factual is `reassign_gate_refuses_an_aliased_assign_value`, the
/// same shape with a `(%length v)` after the store: the pair isolates the read
/// rather than the name. Refusing here leaves the unsuppressed baseline, whose
/// one release covers every value the loop stored, so a walk's own collection
/// strands per call (`tests/elle/region-cell-feeder.lisp`).
#[test]
fn reassign_gate_counts_a_feeder_as_no_holder() {
    let (info, last, last_sites) = one_binder_shape(
        "(var i 0)\n\
         (while (%lt i n)\n\
           (let [v (array i 7)]\n\
             (assign last v)\n\
             (assign i (%add i 1))))",
    );
    assert!(
        last_sites
            .iter()
            .any(|site| info.drop_on_overwrite_sites.contains(site)),
        "a feeder must not refuse the container model"
    );
    let cell = info
        .cell_containers
        .get(&last)
        .expect("a feeder's container records its stores");
    assert!(
        !cell.stores.value_region_set().is_empty(),
        "the container's stored value carries a region to pin"
    );
}

/// Counterfactual against over-admission: a name bound OUTSIDE the loop that
/// stores it is not a feeder, however little reads it. The store-site pin fires
/// once per iteration against one producer reference, so a pin would release a
/// reference the producer never took (docs/impl/region/bindings.md § "A name the
/// store consumes is not a second holder of the value", the once-per-binding
/// fact).
///
/// `reassign_gate_counts_a_feeder_as_no_holder` above is the same shape with the
/// binder inside the loop, so the pair isolates where the name is bound. The
/// trailing `(%length v)` is deliberately absent: with it the shape would be
/// refused for the read instead, and the fact under test would go untested.
#[test]
fn reassign_gate_refuses_a_feeder_bound_outside_the_loop() {
    let (info, last, last_sites) = one_binder_shape(
        "(let [v (array n 7)]\n\
           (var i 0)\n\
           (while (%lt i n)\n\
             (begin (assign last v)\n\
                    (assign i (%add i 1)))))",
    );
    for site in &last_sites {
        assert!(
            !info.drop_on_overwrite_sites.contains(site),
            "a name bound outside the loop must refuse the model at @{}",
            site.0
        );
    }
    assert!(
        !info.cell_containers.contains_key(&last),
        "a refused cell records no container — its store-site pin would fire \
         once per iteration against one producer reference"
    );
}
