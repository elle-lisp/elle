// audited: 2026-09-06
// docs/impl/region/diagnostics.md
//! What the region gauges read on this tier, and the over-keep they pin.
//!
//! The arena gauges are host-side and tier-transparent, so a program sampling
//! them under the full-module WASM tier measures the tier's own region
//! reclamation. Every region instruction is a structural no-op in this emitter,
//! so an ALLOCATING boundary call strands its fresh region to process teardown
//! — the pinned program-duration over-keep. The pins are shrink-only: realizing
//! region release on this tier lowers them toward the VM's zero; they must
//! never rise.

use super::*;

#[test]
fn wasm_gauge_is_live() {
    // Gauge-live discriminator: a retained list MUST register on the object
    // gauge on every tier — a 0 here means the gauge is dead and every other
    // pin in this block is void, so fail loudly instead of lying.
    let grew = eval(
        "(def o0 (arena/count))\n\
         (def @acc 0)\n\
         (def @k 0)\n\
         (while (%lt k 50)\n\
           (assign acc (%pair k acc))\n\
           (assign k (%add k 1)))\n\
         (%sub (arena/count) o0)",
    );
    let grew: i64 = grew.parse().expect("gauge delta is an int");
    assert!(
        grew >= 50,
        "50 retained pairs grew the object gauge by {grew} — the gauge is dead"
    );
}

#[test]
fn wasm_full_strands_one_region_per_allocating_host_call() {
    // 200 discarded `(pair i i)` — on the VM this reclaims to 0/op; on the
    // full-module WASM tier each allocating boundary call strands its fresh
    // region. Shrink-only pin at 1 region/op.
    let grew = eval(
        "(def r0 (arena/region-count))\n\
         (def @j 0)\n\
         (while (%lt j 200)\n\
           (%pair j j)\n\
           (assign j (%add j 1)))\n\
         (%sub (arena/region-count) r0)",
    );
    let grew: i64 = grew.parse().expect("gauge delta is an int");
    eprintln!("[gauge] wasm-full strand: {grew} regions / 200 allocating ops");
    assert!(
        grew <= 200,
        "200 discarded pairs stranded {grew} regions — the WASM tier's \
         over-keep grew past 1 region per allocating host call"
    );
    assert!(
        grew == 0 || grew == 200,
        "200 discarded pairs stranded {grew} regions — neither the pinned \
         over-keep (200) nor full reclamation (0); re-measure and re-pin"
    );
}

#[test]
fn wasm_full_non_allocating_calls_strand_nothing() {
    // The per-boundary-call region MINT is free: entries materialize lazily on
    // first allocation (regionstore/alloc.rs), so a pure-compute loop must not
    // move the region gauge at all.
    let grew = eval(
        "(def r0 (arena/region-count))\n\
         (def @j 0)\n\
         (while (%lt j 200)\n\
           (assign j (%add j 1)))\n\
         (%sub (arena/region-count) r0)",
    );
    let grew: i64 = grew.parse().expect("gauge delta is an int");
    assert_eq!(
        grew, 0,
        "a non-allocating loop stranded {grew} regions — an unused boundary \
         mint is materializing region entries"
    );
}
