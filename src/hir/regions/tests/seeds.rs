use super::*;

// ── ownership inference: the Shared-seed set ──────────────────────────
//
// `regions::ownership::compute_shared_seeds` — the front edge of the forest's
// Owned/Shared classifier. A seed is a region a value escapes the activation/fiber
// FRONTIER through (return, or fiber boundary), and cannot be Owned. Containment
// facets — store and capture — are deliberately NOT seeds: they build the subtree,
// and the step-2 external-uniqueness walk decides their Shared-ness from whether the
// CONTAINER crosses a frontier (the scheduler's captured-yet-Owned cells are the
// witness; see the module doc). These pins are written from that definition: the
// positives mark a returned region; the negatives — a captured-but-not-returned
// region and a purely-local one — are the load-bearing counterfactuals that a
// containment-seeding projection would wrongly flag. The set is computed, not yet
// consumed.

#[test]
fn shared_seed_marks_returned_local_via_holder() {
    // `p` holds the pair and is returned (the program's tail), so `p` crosses the
    // return frontier; its region projects into the seeds through the return facet
    // (`binding_escapes_via_return`) and `binding_source_regions`.
    let (hir, info, seeds) = shared_seeds("(let [p (%pair 1 2)] p)");
    let pair_r = sole_pair_region(&hir, &info);
    assert!(
        seeds.contains(&pair_r),
        "a returned local's region r{} must be a Shared seed; seeds={:?}",
        pair_r.0,
        seeds,
    );
}

#[test]
fn shared_seed_marks_returned_aggregate_region_level() {
    // The fresh pair is the program's tail value, so its region is in the return
    // frontier (escape's `escapes_return_frontier`, projected through
    // `alloc_region`) — independent of any binding that may also hold it.
    let (hir, info, seeds) = shared_seeds("(%pair 1 2)");
    let pair_r = sole_pair_region(&hir, &info);
    assert!(
        seeds.contains(&pair_r),
        "a returned aggregate r{} must be a Shared seed; seeds={:?}",
        pair_r.0,
        seeds,
    );
}

#[test]
fn shared_seed_excludes_captured_but_not_returned() {
    // `p` is captured by a closure that escapes (it is the program's return value),
    // but the closure does NOT return `p` — it uses it (`(length p)`) and returns an
    // immediate. Capture is a CONTAINMENT edge into the closure's region, not a
    // frontier crossing, so `p`'s region is NOT a seed: the external-uniqueness walk,
    // not this pass, decides its fate from whether the closure's subtree is Owned.
    // Counterfactual: seeding the full `binding_escapes_activation` (capture facet)
    // would wrongly flag it — the scheduler's captured-yet-Owned cells in miniature.
    let (hir, info, seeds) = shared_seeds("(let [p (%pair 1 2)] (fn [] (length p)))");
    let pair_r = sole_pair_region(&hir, &info);
    assert!(
        !seeds.contains(&pair_r),
        "a captured-but-not-returned region r{} must NOT be a Shared seed; seeds={:?}",
        pair_r.0,
        seeds,
    );
}

#[test]
fn shared_seed_excludes_purely_local_region() {
    // `p` is built, read locally by `%first`, and discarded (the program returns
    // nil). It never crosses a frontier, so its region is NOT a Shared seed (it is an
    // Owned candidate). Counterfactual to the positives: were the return projection
    // over-broad, this would fail.
    let (hir, info, seeds) = shared_seeds("(begin (let [p (%pair 1 2)] (%first p)) nil)");
    let pair_r = sole_pair_region(&hir, &info);
    assert!(
        !seeds.contains(&pair_r),
        "a purely-local non-escaping region r{} must NOT be a Shared seed; seeds={:?}",
        pair_r.0,
        seeds,
    );
}

#[test]
fn shared_seed_marks_emitted_aggregate() {
    // The **fiber** frontier, region-level. `(yield (%pair 1 2))` hands the fresh
    // pair to the resumer — a different activation, in general a different fiber — so
    // the pair crosses the fiber boundary and cannot be Owned. Yet no binding holds
    // it (atomless) and the fiber body returns the immediate `0`, so the pair is in
    // NEITHER return projection: the body's tail is `0`, and no holder binding flows
    // to a tail (`binding_escapes_via_return`). Only the region-level EMIT projection
    // (escape's `escapes_fiber_frontier`, the atomless half of the fiber facet)
    // sees it.
    let (hir, info, seeds) = shared_seeds("(fiber/new (fn () (yield (%pair 1 2)) 0) |:yield|)");
    let pair_r = sole_pair_region(&hir, &info);
    assert!(
        seeds.contains(&pair_r),
        "an emitted aggregate r{} must be a Shared seed (it crosses the fiber \
         frontier); seeds={:?}",
        pair_r.0,
        seeds,
    );
}

#[test]
fn shared_seed_marks_emitted_local_via_walk() {
    // The same fiber frontier reached through a binding: `q` holds the pair and is
    // yielded. The emit projection records whatever the operand's value-flow walk
    // returns — here `binding_regions[q]`, the pair's region — so a held-then-emitted
    // value is seeded by the same region-level mechanism as the atomless case (no
    // separate binding-level fiber facet needed). Like the atomless pin, the body's
    // tail is the immediate `0`, so the return projections do not reach `q`.
    let (hir, info, seeds) =
        shared_seeds("(fiber/new (fn () (let [q (%pair 1 2)] (yield q) 0)) |:yield|)");
    let pair_r = sole_pair_region(&hir, &info);
    assert!(
        seeds.contains(&pair_r),
        "a yielded local's region r{} must be a Shared seed (fiber frontier); \
         seeds={:?}",
        pair_r.0,
        seeds,
    );
}

#[test]
fn shared_seed_marks_sent_message() {
    // The **send** half of the fiber frontier, region-level. `(chan/send _ (%pair 1
    // 2))` hands the fresh pair to another fiber through the channel — by pointer, no
    // deep copy under the single-threaded scheduler (`prim_chan_send` enqueues the raw
    // `SendableValue`) — so the message crosses the fiber frontier and cannot be
    // Owned. Escape resolves `chan/send` to its declared `Sends{[1]}` effect and
    // marks the message (its fiber facet); the seed projection unions it. `Sends`
    // (not `Stores`) is what distinguishes this fiber crossing from a containment
    // store like `ffi/callback`'s `Stores{[1]}`, which must NOT seed.
    //
    // The sender argument is irrelevant to compile-time region inference (type-blind;
    // never executed), so `nil` keeps the shape to exactly one `%pair`. Uses the real
    // classification — under the default empty effects `chan/send` is an opaque user
    // fn and the send would not seed. Counterfactual: with the prior return+emit seed
    // set (no send projection) this region is absent and the assert fails.
    let (hir, info, seeds) = shared_seeds_with_effects("(chan/send nil (%pair 1 2))");
    let pair_r = sole_pair_region(&hir, &info);
    assert!(
        seeds.contains(&pair_r),
        "a sent message r{} must be a Shared seed (it crosses the fiber frontier); \
         seeds={:?}",
        pair_r.0,
        seeds,
    );
}
