// audited: 2026-11-09
//! Cycles a region root owns once a binder's init is its own last use: reclaimed per run, and panic-clean.
//!
//! docs/impl/region/adopt.md
//! docs/impl/region/owner.md

use super::*;

/// An inline a↔b cycle whose root `x` is stored into a holder `keep`, which dies at
/// the store. The transfer cut refuses a stored consumer, so `keep` owns the cycle
/// and its subtree drop frees all three (inference pin
/// `region::infer::tests::adopt::subtrees::a_stored_inline_cycle_is_owned_by_its_holder`).
///
/// Counter-factual: with `x`'s init held to its binder node, `a` outlived `keep`'s
/// drop, the obligation refused the subtree, and per-region RC left the cycle behind:
/// two regions per run.
#[test]
fn region_ownership_reclaims_a_stored_inline_cycle() {
    const SUBJECT: &str = "(begin (let [keep (@array)] \
                            (let [x (let [a (@array) b (@array)] \
                                      (begin (%array-push a b) (%array-push b a) a))] \
                              (begin (%array-push keep x) nil))) \
                          nil)";
    let leak = leak_discriminator();
    let on = steady_region_growth(SUBJECT);
    assert!(
        leak > 0,
        "gauge live: the refused-cycle discriminator must leak (per-run region growth \
         {leak}); if 0 the gauge is dead and the bounded assertion below is vacuous",
    );
    assert!(
        on <= 0,
        "the holder's subtree drop must reclaim the stored cycle — per-run live-region \
         growth {on} must be <= 0 (the discriminator leaks {leak})",
    );
}

/// The m↔c capture-back-edge cycle with `(c)` run before the root's last store, so
/// the root's drop follows every use of `m` and `c`, and the root owns the cycle
/// (inference pin
/// `region::infer::tests::adopt::obligation::adopt_edges_claims_captured_store_member_dead_before_the_root`).
///
/// The trap: `m` is store-adopted and keeps its own `DecrefValueRegion`. Emitted after
/// the root's drop, that release reads a freed page, and a debug generation assert
/// fires. Fifty panic-clean runs are the soundness half of the pin; the bounded
/// growth beside the leaking discriminator is the reclamation half.
#[test]
fn region_ownership_capture_back_edge_cycle_reclaims_under_its_root() {
    const SUBJECT: &str = "(begin (let [root (@array) m (@array)] \
                            (let [c (fn [] (length m))] \
                              (begin (%array-push m c) (c) (%array-push root m) nil))) \
                          nil)";
    let leak = leak_discriminator();
    let on = steady_region_growth(SUBJECT);
    assert!(
        leak > 0,
        "gauge live: the refused-cycle discriminator must leak (per-run region growth \
         {leak}); if 0 the gauge is dead and the bounded assertion below is vacuous",
    );
    assert!(
        on <= 0,
        "the root's subtree drop must reclaim the m↔c cycle — per-run live-region \
         growth {on} must be <= 0 (the discriminator leaks {leak})",
    );
}
