// audited: 2026-10-06
//! Lowering builds in pages of the heap `with_heap` names, and leaves no working region behind, failing or not.
//!
//! docs/impl/lir.md

use super::*;

/// A `begin` of `n` integer literals, then `last`.
fn many_constants_then(n: usize, last: HirKind) -> Hir {
    let mut body: Vec<Hir> = (0..n)
        .map(|i| Hir::silent(HirKind::Int(i as i64), make_span()))
        .collect();
    body.push(Hir::silent(last, make_span()));
    Hir::silent(HirKind::Begin(body), make_span())
}

/// Lower `hir` on a heap of the test's own, and answer the page claims the
/// lowering made there and whether it succeeded.
fn lower_on_a_fresh_heap(hir: &Hir) -> (u64, bool) {
    let arena = crate::hir::BindingArena::new();
    let mut heap = FiberHeap::new();
    let regions = heap.active_region_count();
    let claims = heap.page_claims();
    let ok = {
        let mut lowerer = Lowerer::new(&arena).with_heap(&mut heap);
        lowerer.lower(hir).is_ok()
    };
    assert_eq!(
        heap.active_region_count(),
        regions,
        "lowering leaves no working region behind (succeeded: {ok})"
    );
    (heap.page_claims() - claims, ok)
}

/// The counter-factual is a lowerer that keeps its working form on the Rust
/// heap: it claims no page, so the region gauges cannot see what it built.
#[test]
fn lowering_claims_pages_from_the_heap_it_names() {
    let (claims, ok) = lower_on_a_fresh_heap(&many_constants_then(4_000, HirKind::Nil));
    assert!(ok, "precondition: a begin of constants lowers");
    assert!(
        claims > 0,
        "4,000 instructions claim pages from the lowerer's heap"
    );
}

/// A lowering that fails partway frees the working region as one that
/// succeeds does. The counter-factual is a lowerer that frees its region only
/// on the success path: the failure leaves an active region on the heap.
#[test]
fn a_failed_lowering_frees_its_working_region() {
    let (claims, ok) = lower_on_a_fresh_heap(&many_constants_then(4_000, HirKind::Error));
    assert!(!ok, "precondition: the poison node fails the lowering");
    assert!(
        claims > 0,
        "the instructions built before the failure claimed pages"
    );
}
