// audited: 2026-09-29
//! A mutual-recursion cycle whose member is handed back is still reclaimed, however the body hands it over.
//!
//! docs/impl/region/letrec.md

use super::*;

/// The closure-cycle merge's **return-funded admission** (docs/impl/region/letrec.md):
/// the ev/od cycle with a member RETURNED still reclaims per call. One base case apart
/// from `region_ownership_reclaims_mutual_recursion_closure_cycle`, `ev` hands back `ev`
/// itself, so `ev`'s region carries escape's return facet. Refusing the whole SCC to
/// Shared on that facet would leave per-region RC unable to collect the cycle, and both
/// closures and both forward cells would stay live once per call.
///
/// The facet is not a reason to refuse: the merge collapses the returned member's
/// region onto the arena, so the value handed out lives IN the arena and the mint that
/// funds the caller raises the arena's own count. The letrec body's tail is a call to
/// the MEMBER `ev`, so the binding-scope `DecrefRegion` is dead past that
/// frame-replacing `TailCall` and the release rides the member deferral, which runs at
/// the recursion's normal completion — after the mint. The caller then discards the
/// result and the arena reaches zero.
///
/// The other two body shapes that hand the value over themselves are gauged by
/// [`region_ownership_reclaims_returned_cycle_every_frame_exit`].
///
/// The counter-factual is the discriminator, as in the non-returning case: the merge is
/// unconditional, so the pin is per-run region growth beside the leaking bare-@array
/// self-cycle, whose slope proves the gauge is live.
#[test]
fn region_ownership_reclaims_returned_mutual_cycle_per_call() {
    let leak = leak_discriminator();
    assert!(
        leak > 0,
        "gauge live: the refused-cycle discriminator must leak (per-run region growth \
         {leak}); if 0 the gauge is not detecting per-run growth and the bounded \
         assertion below is vacuous",
    );
    // `ev` is RETURNED (a value use), which disables call-site param joins — the
    // diverging guards prove the `%lt`/`%sub` operands. The result is discarded.
    let src = "(def f (fn [k] \
                 (letrec [ev (fn [m] (when (%not (%int? m)) (error :m)) \
                               (if (%lt m 1) ev (od (%sub m 1)))) \
                          od (fn [m] (when (%not (%int? m)) (error :m)) \
                               (if (%lt m 1) ev (ev (%sub m 1))))] \
                   (ev k)))) \
               (begin (f 3) nil)";
    let growth = steady_region_growth(src);
    assert!(
        growth <= 0,
        "a RETURNED member's ev/od cycle must be reclaimed by the return-funded merge \
         admission — per-run live-region growth {growth} must be <= 0 (the \
         discriminator leaks {leak} per run, so the gauge is live)",
    );
}

/// The return-funded admission turns on ONE structural fact — the letrec body hands the
/// value over itself, every tail exit of it leaving the frame — and this drives the two
/// remaining ways it can do that beside the member tail call above
/// (docs/impl/region/letrec.md).
///
/// A NON-member tail call reaches the caller's mint by either of its callee's
/// resolutions: a closure replaces the frame and the release rides
/// `deferred_release_slot` to the recursion's completion, a native keeps it and falls
/// through to the binding-scope `DecrefRegion` the lowerer emits at the `Letrec` node —
/// after the mint the call itself emits at the call site. A bare member VALUE tail has
/// no tail call at all, and the frame's own `Return` (which functionalization places
/// inside the letrec body, the letrec being the frame's tail) is what mints first.
///
/// The cycle bound OUT of tail position is not a frame exit at all and reclaims for a
/// different reason;
/// [`region_ownership_reclaims_returned_cycle_bound_out_of_tail_position`] is its
/// gauge. The bare-`@array` discriminator asserted first is what proves the bounded
/// assertions here are measuring reclamation rather than a dead gauge.
#[test]
fn region_ownership_reclaims_returned_cycle_every_frame_exit() {
    let leak = leak_discriminator();
    assert!(
        leak > 0,
        "gauge live: the refused-cycle discriminator must leak (per-run region growth \
         {leak}); if 0 the gauge is not detecting per-run growth and the bounded \
         assertions below are vacuous",
    );
    // The ev/od cycle, spelled once; each case differs only in the letrec BODY and in
    // what encloses it. `ev` is returned (a value use), which disables call-site param
    // joins — the diverging guards prove the `%lt`/`%sub` operands.
    let cycle = "letrec [ev (fn [m] (when (%not (%int? m)) (error :m)) \
                              (if (%lt m 1) ev (od (%sub m 1)))) \
                         od (fn [m] (when (%not (%int? m)) (error :m)) \
                              (if (%lt m 1) ev (ev (%sub m 1))))]";

    let foreign = steady_region_growth(&format!(
        "(def g (fn [x] x)) (def f (fn [k] ({cycle} (g (ev k))))) (begin (f 3) nil)"
    ));
    assert!(
        foreign <= 0,
        "a returned cycle whose body tail-calls a NON-member must be reclaimed — both \
         of that callee's resolutions release after the mint — but per-run live-region \
         growth is {foreign} (the discriminator leaks {leak} per run, so the gauge is \
         live)",
    );

    let value = steady_region_growth(&format!("(def f (fn [k] ({cycle} ev))) (begin (f 3) nil)"));
    assert!(
        value <= 0,
        "a returned cycle whose body's tail is a bare member VALUE must be reclaimed — \
         the frame's `Return` sits inside the letrec body and mints before the \
         binding-scope drop — but per-run live-region growth is {value} (the \
         discriminator leaks {leak} per run, so the gauge is live)",
    );
}

/// The cycle whose letrec is NOT its frame's tail: bound to `c` and handed on by a
/// later statement, so the body falls out to a bare member value and `c` names the
/// member's region directly (docs/impl/region/letrec.md). No mint stands between the
/// letrec and the binding scope, so a release pinned there would free the arena under
/// `c`; the merge instead adopts the point the last-use rule already computed for the
/// handed-out member — the enclosing `Return`, whose mint precedes that node's own
/// releases — and waives the sole-held proxy for the member it followed.
///
/// Two shapes, because the two halves of that reading are independent: the value is
/// RETURNED out of `f` (the release rides the `Return` pin), and the value is CALLED
/// inside `f` and never handed further (the release rides its ordinary last use). Both
/// reclaim the whole cycle — two closures and two forward cells — per call.
///
/// Counterfactual: both read the discriminator's slope if the cycle is refused to Shared,
/// where per-region RC cannot collect it.
#[test]
fn region_ownership_reclaims_returned_cycle_bound_out_of_tail_position() {
    let leak = leak_discriminator();
    assert!(
        leak > 0,
        "gauge live: the refused-cycle discriminator must leak (per-run region growth \
         {leak}); if 0 the gauge is not detecting per-run growth and the bounded \
         assertions below are vacuous",
    );
    let cycle = "letrec [ev (fn [m] (when (%not (%int? m)) (error :m)) \
                              (if (%lt m 1) ev (od (%sub m 1)))) \
                         od (fn [m] (when (%not (%int? m)) (error :m)) \
                              (if (%lt m 1) ev (ev (%sub m 1))))]";

    let returned = steady_region_growth(&format!(
        "(def g (fn [x] x)) \
         (def f (fn [k] (let [c ({cycle} ev)] (g k) c))) \
         (begin (f 3) nil)"
    ));
    assert!(
        returned <= 0,
        "a cycle bound out of tail position and RETURNED must be reclaimed — the \
         handed-out member's release is pinned at the enclosing `Return`, after its \
         mint — but per-run live-region growth is {returned} (the discriminator leaks \
         {leak} per run, so the gauge is live)",
    );

    let called = steady_region_growth(&format!(
        "(def g (fn [x] x)) \
         (def f (fn [k] (let [c ({cycle} ev)] (g k) (begin (c 3) nil)))) \
         (begin (f 3) nil)"
    ));
    assert!(
        called <= 0,
        "a cycle bound out of tail position and CALLED in place must be reclaimed — the \
         handed-out member's release sits at its ordinary last use, which post-dominates \
         the binding scope — but per-run live-region growth is {called} (the \
         discriminator leaks {leak} per run, so the gauge is live)",
    );
}
