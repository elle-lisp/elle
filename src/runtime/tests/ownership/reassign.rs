// audited: 2026-10-20
//! A module-scope mutable reassigned in a loop releases each displaced value at its overwrite, whatever produced it.
//!
//! docs/impl/region/bindings.md

use super::*;

/// The live-region growth of a top-level `@acc` reassigned by `assign_form` on
/// each of 200 trips, read mid-run by the program ([`mid_run_growth`] over the
/// full runtime, whose stdlib the wrappers under test live in). `prelude` runs
/// before the cell is bound.
fn toplevel_region_growth(prelude: &str, assign_form: &str) -> i64 {
    mid_run_growth(
        Runtime::new(),
        &format!("{prelude} (def @acc nil)"),
        assign_form,
        "arena/region-count",
    )
}

/// The gauge's discriminator: the self-referential accumulator chains every
/// prior into the next pair, so every prior IS live and the delta legitimately
/// grows by about 200. A bounded reading beside this proves the measurement
/// sees per-trip growth, so a near-zero delta is a reclamation and not a dead
/// gauge.
fn assert_gauge_sees_growth() {
    let live_chain_growth = mid_run_discriminator(Runtime::new(), "arena/region-count");
    assert!(
        live_chain_growth > 150,
        "precondition: the self-referential accumulator legitimately retains every \
         prior (the chain is live), so region growth over 200 trips must be \
         large (~200) — got {live_chain_growth}; if small, the measurement is not \
         seeing per-trip region growth and the bounded assertions are vacuous",
    );
}

/// A TOP-LEVEL (file-letrec) mutable reassigned in a loop must not accumulate
/// its overwritten PRIOR values until frame teardown
/// (docs/impl/region/bindings.md). Each prior is dead the instant the next `assign` displaces
/// it; holding it to program exit is unbounded RSS growth in a long-running loop.
///
/// The donation: a fresh local allocation's ordinary decref is suppressed and the
/// cell ADOPTS that producer reference, so drop-on-overwrite is its sole release
/// and the lowerer emits NO incref-on-store (`donated_overwrite_sites`). The
/// counter-factual an unbalanced incref-on-store reintroduces: `born + store −
/// overwrite = +1` per displaced prior, every value the cell ever held standing
/// until program exit.
#[test]
fn reassign_toplevel_prior_release_is_bounded() {
    assert_gauge_sees_growth();
    // Fresh `(%pair n n)` each trip: the prior is genuinely dead once the next
    // `assign` displaces it, so a prompt drop-on-overwrite keeps the region count
    // flat across the extra 200 trips.
    let dead_prior_growth = toplevel_region_growth("", "(assign acc (%pair n n))");
    assert!(
        dead_prior_growth < 50,
        "a top-level mutable reassigned to a fresh (dead) value in a loop must \
         release each displaced prior at its overwrite, not hold it to frame \
         teardown — region growth over 200 trips must be near zero, got \
         {dead_prior_growth} (~200 means every prior is over-kept until program \
         exit, the unbalanced incref-on-store)",
    );
}

/// A CALL RESULT stored into a top-level mutable takes the counted store, and
/// the stored region keeps its producer release, pinned to the store site
/// (docs/impl/region/bindings.md). Every shape here is a call
/// result: an array literal and a struct literal lower to a native call, as the
/// `list`, `string` and `pair` wrappers do, and a counted element read aliases
/// one.
///
/// The counter-factual: the analysis suppressed the stored region's ordinary
/// decref beside the counted store, and the emitter's mutated-slot backstop
/// skipped the same release again, so `born + store − overwrite = +1` per store
/// and every shape read about 200. The conditional store reads about 100 under
/// the defect: one per trip that stored.
#[test]
fn reassign_toplevel_call_result_prior_release_is_bounded() {
    assert_gauge_sees_growth();
    let shapes: [(&str, &str, &str); 8] = [
        ("an array literal", "", "(assign acc [n])"),
        ("a list", "", "(assign acc (list n))"),
        ("a string", "", "(assign acc (string \"x\" n))"),
        ("a struct literal", "", "(assign acc {:n n})"),
        ("the pair wrapper", "", "(assign acc (pair n n))"),
        (
            "a user function's result",
            "(def build (fn [n] (list n n)))",
            "(assign acc (build n))",
        ),
        (
            "an element of a call result",
            "",
            "(assign acc (get [[n] [n]] 0))",
        ),
        (
            "a conditional store",
            "",
            "(when (%eq (%rem n 2) 0) (assign acc [n]))",
        ),
    ];
    for (shape, prelude, form) in shapes {
        let growth = toplevel_region_growth(prelude, form);
        assert!(
            growth < 50,
            "a top-level mutable reassigned to {shape} in a loop must release each \
             displaced prior at its overwrite — region growth over 200 trips must be \
             near zero, got {growth} (~200 means the producer's release was \
             suppressed beside the counted store)",
        );
    }
}
