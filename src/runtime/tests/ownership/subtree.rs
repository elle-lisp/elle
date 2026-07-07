use super::*;

/// End-to-end exercise of the ownership forest (docs/impl/region-model.md
/// § "Adoption and subtree drop"). The never-mergeable shape — a Fresh mutable
/// container `(@array)` and the value `(array 1 2)` pushed into it, both
/// call-result regions no static slot can name, so MERGE cannot collapse them —
/// compiled under `--region-ownership` emits `AdoptRegion(container, value)` at the
/// push (pinned in `lir::lower::tests::adopt_region_emitted_for_owned_container_under_flag`).
/// Running it must:
///  - execute cleanly: a broken adopt or subtree drop would free the value early or
///    twice, tripping a debug `decref`/generation assert (double-free / stale deref);
///  - reclaim the container+value each run, so the live region count stays bounded
///    across repeated runs — the container's single decref subtree-drops the value.
#[test]
fn region_ownership_adopt_subtree_drop_reclaims_in_a_real_run() {
    use crate::pipeline::compile_file_repl;
    let mut rt = Runtime::without_stdlib();
    let src = "(begin (%array-push (@array) (array 1 2)) nil)";

    // Compile once (isolates the runtime region behaviour from compile scratch),
    // then run the same bytecode repeatedly.
    let result = {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file_repl(src, symbols, cctx, "<embed>")
            .expect("compiles")
            .0
    };

    // Warm-up run, then measure the steady-state live region count.
    {
        let (vm, symbols, cctx) = rt.parts();
        let v = vm
            .execute_scheduled(&result.bytecode, symbols, cctx)
            .expect("runs");
        assert!(v.is_nil(), "the discarded-container program returns nil");
    }
    let baseline = rt.heap().active_region_count();

    for _ in 0..50 {
        let (vm, symbols, cctx) = rt.parts();
        let v = vm
            .execute_scheduled(&result.bytecode, symbols, cctx)
            .expect("runs");
        assert!(v.is_nil());
    }
    let after = rt.heap().active_region_count();
    assert!(
        after <= baseline,
        "the Owned container+value subtree must be reclaimed by subtree drop each \
         run — live region count must not grow (baseline={baseline}, after 50 \
         runs={after})",
    );
}

/// End-to-end exercise of the **interior-cycle** ownership cut. A Fresh container
/// `root` directly holds two members
/// `a` and `b`, which reference EACH OTHER (`a ⊇ b`, `b ⊇ a`). Per-region RC cannot
/// collect the a↔b reference cycle (region-rules.md Rule 8), so flag-OFF this leaks —
/// the live-region count grows every run. Under `--region-ownership` the cut adopts a
/// and b directly by the root, whose single decref subtree-drops the whole cycle, so
/// the count stays bounded. The push order makes `root` the last region used, so its
/// `decref_point` post-dominates the members (the lifetime obligation).
///
/// The flag-off measurement is the built-in counterfactual: the SAME bytecode shape
/// must leak flag-off (proving the cut, not the shape, is what reclaims it) and be
/// bounded flag-on. Running flag-on must also be panic-clean — a broken adopt/subtree
/// drop would free a member early or twice, tripping a debug generation/decref assert.
#[test]
fn region_ownership_reclaims_interior_cycle_subtree() {
    // root directly holds a and b, which reference EACH OTHER (`a ⊇ b`, `b ⊇ a`). The
    // push order makes `root` the last region used, so its decref_point post-dominates
    // the members (the lifetime obligation). The forest adopts a and b by the root,
    // whose single decref subtree-drops the whole cycle — which per-region RC could
    // never collect (region-rules.md Rule 8).
    const SUBJECT: &str = "(begin (let [root (@array) a (@array) b (@array)] \
                           (begin (%array-push a b) (%array-push b a) \
                                  (%array-push root a) (%array-push root b) nil)) \
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
        "the ownership forest must reclaim the interior a↔b cycle by subtree drop — \
         per-run live-region growth {on} must be <= 0 (the discriminator leaks {leak})",
    );

    // The checked-on (native-Call) production face: the stores are opaque `Funnel`
    // calls whose containment is funnel-recovered, and the adopt is keyed at the
    // funnel call site (region-model.md § "The funnel adopt — the checked-on store
    // face"); the cut must reclaim identically.
    let _ci = crate::config::test_override::ScopedCheckedIntrinsics::new(true);
    let leak_checked = leak_discriminator();
    let on_checked = steady_region_growth(SUBJECT);
    assert!(
        leak_checked > 0,
        "gauge live (checked-on): the discriminator must leak (growth {leak_checked})",
    );
    assert!(
        on_checked <= 0,
        "the funnel-adopt must reclaim the interior cycle on the checked-on path too — \
         per-run growth {on_checked} must be <= 0 (the discriminator leaks {leak_checked})",
    );
}

/// End-to-end soundness of the **capture** ownership cut. A pair `p` captured by a
/// LOCAL closure `c` (called in place,
/// discarded) is an Owned member of `c`'s subtree once tight last-use admits it: the
/// lowerer adopts `p` at the closure construction and suppresses `p`'s own compiler decref,
/// so `p` is reclaimed solely by `c`'s subtree drop.
///
/// Unlike the interior-cycle/nested cuts, the simple capture shape does NOT leak flag-off
/// (per-region RC reclaims the immutable pair), so there is no leak counterfactual here —
/// the test is a SOUNDNESS guard. Running flag-on must be panic-clean: the suppression is
/// load-bearing — `p`'s `decref_point` is over-extended one structural step past the
/// closure, so were its decref NOT suppressed it would fire AFTER the
/// subtree drop freed `p`, a direct decref of an absent region tripping the debug
/// `regionstore` phantom/double-free assert. Bounded growth confirms the subtree is
/// reclaimed each run (an un-adopted-yet-suppressed `p` would instead leak — unbounded
/// growth — the regression this also catches).
#[test]
fn region_ownership_capture_adopt_reclaims_in_a_real_run() {
    use crate::pipeline::compile_file_repl;
    let mut rt = Runtime::without_stdlib();
    // The closure body reads `p` with `%first` (the solver pins use `(length p)`, the
    // canonical analysis-only form; `length` would raise "Not a
    // proper list" on the improper `%pair` at runtime). `%first p` captures `p` exactly
    // the same way — a free variable in the closure body — so the capture-adopt shape is
    // identical, but it executes.
    let src = "(begin (let [p (%pair 1 2)] (let [c (fn [] (%first p))] (c))) nil)";
    let result = {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file_repl(src, symbols, cctx, "<embed>")
            .expect("compiles")
            .0
    };
    {
        let (vm, symbols, cctx) = rt.parts();
        let v = vm
            .execute_scheduled(&result.bytecode, symbols, cctx)
            .expect("runs");
        assert!(
            v.is_nil(),
            "the discarded captured-value program returns nil"
        );
    }
    let baseline = rt.heap().active_region_count();
    for _ in 0..50 {
        let (vm, symbols, cctx) = rt.parts();
        let v = vm
            .execute_scheduled(&result.bytecode, symbols, cctx)
            .expect("runs");
        assert!(v.is_nil());
    }
    let after = rt.heap().active_region_count();
    assert!(
        after <= baseline,
        "the captured-value Owned subtree must be reclaimed by the closure's subtree drop \
         each run — live region count must not grow (baseline={baseline}, after 50 \
         runs={after}); growth here means `p` was suppressed but not adopted (a leak)",
    );
}

/// End-to-end exercise of the **co-owned-cycle** cut. A *bare* mutual reference cycle
/// — two `@array`s pushing each other (`a ⊇ b`,
/// `b ⊇ a`) with NO container parent — has no owner among its members, so neither the
/// flat nor the interior-cycle adopt cut (both need a top container) can reclaim it.
/// Per-region RC cannot collect the a↔b cycle (region-rules.md Rule 8), so flag-OFF this
/// leaks — the live-region count grows every run. Under `--region-ownership` the cut frees
/// the whole cycle as one `FreeRegionGroup` at its collective last use, so the count stays
/// bounded. This is the distinguishing case from `region_ownership_reclaims_interior_cycle_subtree`,
/// which has a `root` container holding the cycle; here there is none.
///
/// The flag-off measurement is the built-in counterfactual: the SAME bytecode shape must
/// leak flag-off (proving the cut, not the shape, reclaims it) and be bounded flag-on.
/// Running flag-on must also be panic-clean — a broken group free would free a member
/// early or twice, tripping a debug generation/decref assert.
#[test]
fn region_ownership_reclaims_bare_cycle_group() {
    // a ⊇ b (push a b); b ⊇ a (push b a). No container holds a or b — the bare cycle,
    // reclaimed by one `FreeRegionGroup` at its collective last use (per-region RC
    // cannot collect the a↔b cycle, region-rules.md Rule 8). This is the distinguishing
    // case from the interior-cycle subtree, which has a `root` container; here there is
    // none.
    const SUBJECT: &str = "(begin (let [a (@array) b (@array)] \
                           (begin (%array-push a b) (%array-push b a) nil)) \
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
        "the ownership forest must reclaim the bare a↔b cycle by the co-owned group \
         free — per-run live-region growth {on} must be <= 0 (the discriminator leaks \
         {leak})",
    );

    // The checked-on (native-Call) production face: the group walk reads the same
    // funnel-recovered containment (`ownership_inputs`), and its `FreeRegionGroup`
    // emit is value-resolved (member slots, no store opcode), so the bare cycle must
    // reclaim identically on the production path.
    let _ci = crate::config::test_override::ScopedCheckedIntrinsics::new(true);
    let leak_checked = leak_discriminator();
    let on_checked = steady_region_growth(SUBJECT);
    assert!(
        leak_checked > 0,
        "gauge live (checked-on): the discriminator must leak (growth {leak_checked})",
    );
    assert!(
        on_checked <= 0,
        "the co-owned group free must reclaim the bare cycle on the checked-on path \
         too — per-run growth {on_checked} must be <= 0 (the discriminator leaks \
         {leak_checked})",
    );
}

/// End-to-end exercise of the **deep-nesting** cut. A Fresh container `root` holds `a`,
/// and `a` holds `b`, which holds `a` back
/// — a reference cycle `a ⊇ b ⊇ a` nested one level below the root, which holds only `a`
/// directly (no `root ⊇ b` edge). The flat cut refused this subtree (`b` has no
/// `member → root` edge), so under it the nested a↔b cycle leaks exactly as flag-off
/// does; the deep-nesting cut adopts `b` by its actual parent `a` and `a` by the root, so
/// the root's recursive subtree drop frees the whole chain — the case the flat cut could
/// not reach.
///
/// Counterfactual built in: the SAME bytecode must leak flag-off (the nested a↔b cycle is
/// uncollectable by per-region RC, region-rules.md Rule 8) and be bounded flag-on, and
/// flag-on must run panic-clean (a mis-ordered adopt or a missing recursive drop would
/// free `b` early or twice, tripping a debug generation/decref assert).
#[test]
fn region_ownership_reclaims_nested_cycle_subtree() {
    // root ⊇ a (push root a); a ⊇ b (push a b); b ⊇ a (push b a) — the a↔b cycle is
    // nested under `a`, reachable from the root ONLY through `a`. The `root` push is
    // last so its decref_point post-dominates the members (the lifetime obligation).
    // The deep-nesting cut adopts `b` through its actual parent `a` and `a` by the root,
    // whose recursive subtree drop frees the whole chain.
    const SUBJECT: &str = "(begin (let [root (@array) a (@array) b (@array)] \
                           (begin (%array-push a b) (%array-push b a) \
                                  (%array-push root a) nil)) \
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
        "the deep-nesting cut must reclaim the nested cycle by adopting `b` through `a` \
         and subtree-dropping from the root — per-run live-region growth {on} must be \
         <= 0 (the discriminator leaks {leak})",
    );

    // The checked-on (native-Call) production face: the multi-level containment is
    // funnel-recovered — `b`'s adopt through its actual parent `a` must be keyed at
    // `(%array-push a b)`'s funnel call site exactly as at the intrinsic store
    // (region-model.md § "The funnel adopt — the checked-on store face").
    let _ci = crate::config::test_override::ScopedCheckedIntrinsics::new(true);
    let leak_checked = leak_discriminator();
    let on_checked = steady_region_growth(SUBJECT);
    assert!(
        leak_checked > 0,
        "gauge live (checked-on): the discriminator must leak (growth {leak_checked})",
    );
    assert!(
        on_checked <= 0,
        "the funnel-adopt must reclaim the nested cycle on the checked-on path too — \
         per-run growth {on_checked} must be <= 0 (the discriminator leaks {leak_checked})",
    );
}

/// End-to-end soundness of a COMBINED store + capture subtree (the modes chained in
/// one Owned component, pinned at the inference level by
/// `regions::tests::adopt::adopt_edges_chains_store_and_capture_in_one_subtree`). A Fresh
/// container `root` holds a local capturing closure `c`, and `c` captures the pair `p`:
/// `c` is adopted by `root` through a STORE edge and `p` is adopted by `c` through a
/// CAPTURE edge. So at runtime `c` is a capture-adopt PARENT (owning `p`) that then
/// becomes a store-adopt CHILD (owned by `root`) — the untested interaction is whether
/// `c`'s `owned_children` (holding `p`) survive `c`'s own adoption, so the root's
/// RECURSIVE subtree drop reaches `p` two levels down.
///
/// Like the lone-capture cut this acyclic chain reclaims flag-off (per-region RC frees
/// the root→c→p chain), so it is a SOUNDNESS guard, not a leak counterfactual. Flag-on
/// must run panic-clean — `p`'s own decref is suppressed (capture-adopt member), so were
/// `p` NOT reached by the recursive drop it would be a stranded leak (caught by bounded
/// growth), and a broken re-adoption that dropped `c`'s `owned_children` would either
/// strand `p` (leak) or free it twice (a debug generation/decref panic). The `(c)` call
/// precedes the push so the root (last used at the push) post-dominates `c`'s last use.
#[test]
fn region_ownership_store_then_capture_chain_reclaims_in_a_real_run() {
    use crate::pipeline::compile_file_repl;
    let mut rt = Runtime::without_stdlib();
    let src = "(begin (let [p (%pair 1 2) root (@array)] \
                        (let [c (fn [] (%first p))] \
                          (begin (c) (%array-push root c) nil))) \
                      nil)";
    let result = {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file_repl(src, symbols, cctx, "<embed>")
            .expect("compiles")
            .0
    };
    {
        let (vm, symbols, cctx) = rt.parts();
        let v = vm
            .execute_scheduled(&result.bytecode, symbols, cctx)
            .expect("runs");
        assert!(
            v.is_nil(),
            "the discarded container+closure program returns nil"
        );
    }
    let baseline = rt.heap().active_region_count();
    for _ in 0..50 {
        let (vm, symbols, cctx) = rt.parts();
        let v = vm
            .execute_scheduled(&result.bytecode, symbols, cctx)
            .expect("runs");
        assert!(v.is_nil());
    }
    let after = rt.heap().active_region_count();
    assert!(
        after <= baseline,
        "the combined store+capture Owned subtree must be reclaimed by the root's \
         recursive subtree drop each run — live region count must not grow \
         (baseline={baseline}, after 50 runs={after}); growth means the captured pair \
         `p`, suppressed as a capture-adopt member, was not reached by the recursive \
         drop through the store-adopted closure `c` (a stranded leak)",
    );
}

/// End-to-end soundness of the store-adopt **emit order** at a shared `decref_point`.
/// A fresh `%pair` is pushed into a fresh, LET-BOUND `(@array)` whose push result is
/// DISCARDED (the `let` is not the loop body's tail), driven in a loop. The container
/// is a `Fresh` call-result freed value-based, and at the let-body it is freed by TWO
/// releases — its binding release AND the discarded pass-through result of
/// `%array-push` (which returns its container) — while the pushed pair is a
/// store-adopted member whose OWN slot-resolved `DecrefRegion` shares that same
/// `decref_point`. The member's decref is a structural no-op only while it is still
/// `Owned`, so it must be emitted before the container's rc-zeroing release; the
/// members-first bucket sort (`with_region_info`) guarantees it. Pre-fix the sort put
/// the member's plain `DecrefRegion` LAST, so the container's discarded-pass-through
/// release subtree-dropped the pair before its own decref — the pair's slot-resolved
/// `DecrefRegion` then landed on a freed region (the phantom/double-free panic at
/// `regionstore/refcount.rs`). This is the counterfactual: `steady_region_growth`
/// runs the loop and would PANIC on the first double-free before the fix.
///
/// Requires the intrinsic (`--checked-intrinsics=off`) path — the ambient default in
/// these tests — where `%pair` lowers as an intrinsic freed by a slot-resolved
/// `DecrefRegion`; checked-on the member is a `Fresh` value-based release that
/// tolerates the freed case. Bounded growth beside the leaking discriminator confirms
/// the subtree still reclaims each iteration (the fix reorders releases, it does not
/// refuse the adopt).
#[test]
fn region_ownership_pair_pushed_into_let_bound_array_in_loop_reclaims() {
    // The pushed pair is a store-adopted member of the let-bound container's Owned
    // subtree; the loop rebuilds and reclaims it every iteration. The container's push
    // result is discarded (the `let` precedes `(assign j …)`), so the discarded
    // pass-through release coincides with the member's decref — the emit order the fix
    // corrects.
    const SUBJECT: &str = "(begin (def @j 0) \
                           (while (%lt j 3) \
                             (let [items (@array)] (%array-push items (%pair 1 2))) \
                             (assign j (%add j 1))) \
                           nil)";
    let leak = leak_discriminator();
    // Pre-fix this call panics on the first iteration's double-free (the counterfactual);
    // post-fix it returns a bounded growth.
    let on = steady_region_growth(SUBJECT);
    assert!(
        leak > 0,
        "gauge live: the discriminator must leak (per-run region growth {leak}); if 0 \
         the gauge is dead and the bounded assertion below is vacuous",
    );
    assert!(
        on <= 0,
        "the let-bound container's Owned subtree must reclaim the pushed pair each \
         iteration — per-run live-region growth {on} must be <= 0 (the discriminator \
         leaks {leak})",
    );
}
