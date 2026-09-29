// audited: 2026-09-29
//! A self-recursive local closure pins no per-call forward cell: its retained cost matches a cell-free closure's.
//!
//! docs/impl/region/cells.md

use super::*;

/// The per-call cost of a self-recursive local closure: it is **cell-free**. A
/// binding referenced only by its own initializer lambda is captured solely by a
/// self-edge, which does not mark it captured (src/hir/arena.rs `mark_captured`).
/// So it has `needs_capture() == false` and no forward cell. Its self-reference
/// resolves to the executing closure (`LoadSelf` / a self-call), never a cell load.
/// A RETAINED self-recursive `loop` therefore pins exactly TWO region objects per
/// call, the closure and its one-entry env. A foreign-capturing closure of equal
/// capture arity (one captured upvalue, not itself, likewise cell-free) pins the
/// SAME two. Their object-count gap is ~0: no per-call forward cell tells them apart.
///
/// This gauges that cell-free baseline, so a regression that reintroduces a
/// per-call cell for pure self-recursion fails loudly. The test RETAINS every
/// closure in a program-lifetime `%pair` CHAIN, where each new pair links the last,
/// so every closure stays reachable. It then reads object-count growth
/// (`arena/count`) across 200 retained builds, which the program samples mid-run as
/// `reassign_toplevel_prior_release_is_bounded` samples its gauge. The closure
/// escapes by return, so the caller holds it.
///
/// The retain route decides the reading, and must stay a chain rather than a push into a
/// container. A `%pair` records a COUNTED cross-region edge to the closure, so the
/// closure's region stays in the active accounting `arena/count` sums. A container
/// store instead lets the ownership forest ADOPT the stored region as a member of the
/// container's subtree, and an Owned member carries no count — it leaves the sums
/// entirely (retained values, invisible objects). A control retained that way reads
/// as *shrinking*, and the gap below stops measuring cells at all.
///
/// Each retained build therefore pins THREE objects per call — the closure, its
/// one-entry env, and the linking pair — and TWO regions (the closure's and the
/// pair's). What the gap pins is that both shapes pay the same: a per-call forward
/// cell would add a fourth object to the self-recursive shape alone. A fresh-pair
/// retain is also the live-growth discriminator: it must grow ~1 object/call, proving
/// `arena/count` tracks per-call allocation (else every reading is void).
#[test]
fn self_recursive_loop_is_cell_free() {
    let retained_growth = |prelude: &str, body: &str, gauge: &str| {
        mid_run_growth(Runtime::without_stdlib(), prelude, body, gauge)
    };

    // Subject: a self-recursive in-lambda `loop`. Its initializer references only
    // itself, a self-edge that does not mark it captured, so `loop` is cell-free —
    // its self-reference resolves to the executing closure. `(frec false)` recurses
    // to the base case and RETURNS the `loop` closure (escaping), which `@keep` pins.
    let rec_prelude = "(def @keep nil) \
        (def frec (fn [k] (letrec [loop (fn [m] (if m loop (loop true)))] (loop k))))";
    // Cell-free analog of equal capture arity: `h` captures one upvalue (the
    // immediate `k`), not itself — likewise a closure + one-entry env, no cell. With
    // self-recursion also cell-free, the only structural difference is gone.
    let for_prelude = "(def @keep nil) \
        (def ffor (fn [k] (let [h (fn [m] (if m k k))] h)))";
    let rec_body = "(assign keep (%pair (frec false) keep))";
    let for_body = "(assign keep (%pair (ffor false) keep))";

    let rec_obj = retained_growth(rec_prelude, rec_body, "arena/count");
    let for_obj = retained_growth(for_prelude, for_body, "arena/count");
    // Gauge-live discriminator: the chain accumulator retains every prior pair by
    // REFERENCE (each new pair links the last), so the object count must grow ~1
    // per call. A pushed fresh pair cannot serve here — nor can it retain the
    // closures above, for the same reason: the `%array-push` funnel store-adopts
    // the pushed region, and an Owned member leaves the active accounting
    // `arena/count` sums — retained values, invisible objects.
    let pair_obj = mid_run_discriminator(Runtime::without_stdlib(), "arena/count");
    let rec_reg = retained_growth(rec_prelude, rec_body, "arena/region-count");
    let for_reg = retained_growth(for_prelude, for_body, "arena/region-count");

    // If the discriminator reads small, `arena/count` is not tracking per-call
    // allocation and every assertion below is vacuous.
    assert!(
        pair_obj > 150,
        "gauge-live: chaining 200 fresh pairs must grow the object count ~200, \
         got {pair_obj}; if small, arena/count is dead and the pins below are void",
    );

    // The cell-free baseline: a self-recursive `loop` mints NO per-call forward cell,
    // so the retained-object gap over the equal-arity cell-free closure collapses to
    // ~0 (over 200 calls, |gap| well under one-per-call). A gap of ~200 would mean a
    // per-call cell came back.
    let cell_gap = rec_obj - for_obj;
    assert!(
        cell_gap.abs() < 60,
        "cell-free self-recursion: a self-recursive `loop` must mint no forward cell, \
         so the retained-object gap over the equal-arity cell-free closure is ~0: \
         self-recursive {rec_obj} - foreign-capture {for_obj} = {cell_gap}, expected \
         ~0 (a gap near 200 means a per-call cell was reintroduced)",
    );

    // The absolute baseline: each retained build pins ~3 objects per call — the
    // closure, its one-entry env, and the pair linking it into the chain (~600/200) —
    // the same as the foreign-capture control. A forward cell would make it ~4/call.
    assert!(
        (450..=750).contains(&rec_obj),
        "cell-free baseline: 200 retained self-recursive `loop` closures pin ~600 \
         objects (3/call: closure + env + the retaining pair, no forward cell), \
         got {rec_obj}",
    );

    // Region growth matches between the shapes too (the closure and its env share one
    // region, and the retaining pair adds one), so a per-call cell would show as an
    // extra REGION as well as an extra object.
    assert!(
        (rec_reg - for_reg).abs() < 50,
        "region growth must match between the self-recursive and foreign-capture \
         shapes (closure + env share one region, plus the retaining pair): \
         self-recursive {rec_reg} vs foreign-capture {for_reg}",
    );
}
