//! Native fresh-result reclamation across the trait-dispatch boundary.
//!
//! `dispatch_native_call` upholds one invariant that the whole pass-through /
//! skip accounting rests on: **a native's fresh result lives in the call's own
//! `alloc_region`**. When it does, the result is recognised as fresh and the
//! escape-incref is skipped, so the consumer's single `DecrefValueRegion`
//! frees it; when the result lives elsewhere it is a genuine pass-through
//! (a borrowed element — `first`/`nth`) and is retained so the consumer's
//! decref balances the retain instead of freeing a region owned elsewhere
//! (`src/vm/core/region.rs`, `pass_through_retain`).
//!
//! A native that dispatches through the trait registry (`first`/`rest`/… →
//! `traitregistry::call_method_fn`) must therefore run its resolved trait
//! method against the SAME `ctx` — the outer call's `alloc_region` — so a fresh
//! result lands where `dispatch_native_call` expects it. Minting a separate
//! `boundary` region for the method instead put a genuinely-fresh result
//! (`(rest [array])`'s copied tail slice) in a region distinct from
//! `alloc_region`; `dispatch_native_call` then mis-read it as a pass-through and
//! over-retained it, so the consumer's decref left the boundary region at RC 1 —
//! never freed. The array-tail copy is a `Fresh`-shaped call-result region no
//! static slot can name, discarded by the caller, leaking one region per call
//! (the `rest-array-copy` oracle probe).
//!
//! The pinning shape below discards `(rest [1 2 3 4 5])` in the steady-growth
//! harness. `first`/`last`/`nth` never exercised the gap (they return a borrowed
//! element, correctly pass-through-retained either way); `rest` on an array is
//! the isolated witness, since its tail copy is the fresh result the mint
//! stranded.

use super::*;

/// `(rest [array])` copies the tail into a fresh immutable array; its call-result
/// region must be reclaimed on discard. This is the trait-dispatch fresh-result
/// reclamation: the resolved `Sequence:rest` native allocates the slice into the
/// outer `rest` call's own region, so `dispatch_native_call` recognises it as
/// fresh and the consumer's `DecrefValueRegion` frees it.
///
/// Counterfactual: minting a separate `boundary` result region for the trait
/// method strands the slice's region at RC 1, leaking one region per run — this
/// same shape reads unbounded growth. Bounded growth beside a leaking
/// discriminator proves the reclamation, not the gauge being dead.
#[test]
fn region_native_trait_dispatch_fresh_result_reclaims() {
    // `(rest array)` — the trait-dispatched fresh-array copy, discarded as a
    // statement, run repeatedly. The array literal reclaims on its own; the
    // tail-copy slice is the fresh call-result region under test.
    const SUBJECT: &str = "(begin (rest [1 2 3 4 5]) nil)";
    let leak = leak_discriminator();
    let on = steady_region_growth(SUBJECT);
    assert!(
        leak > 0,
        "gauge live: the discriminator must leak (per-run region growth {leak}); if 0 \
         the gauge is dead and the bounded assertion below is vacuous",
    );
    assert!(
        on <= 0,
        "a trait-dispatched native's fresh result must be reclaimed on discard — the \
         `(rest [array])` tail-copy region's per-run growth {on} must be <= 0 (the \
         discriminator leaks {leak}); a stranded boundary region leaks one per run",
    );
}

/// Control: `(first array)` returns a borrowed element, not a fresh allocation,
/// so it reclaims regardless of the boundary decision — the pass-through half of
/// the same invariant. Kept beside the `rest` counterfactual so a regression that
/// "fixes" `rest` by over-freeing a genuine pass-through element is caught here
/// (this run must stay panic-clean: over-releasing the borrowed element would
/// trip the debug generation/decref assert).
#[test]
fn region_native_trait_dispatch_passthrough_element_reclaims() {
    const SUBJECT: &str = "(begin (first [1 2 3 4 5]) nil)";
    let on = steady_region_growth(SUBJECT);
    assert!(
        on <= 0,
        "a trait-dispatched pass-through element must not leak — per-run growth {on} \
         must be <= 0",
    );
}
