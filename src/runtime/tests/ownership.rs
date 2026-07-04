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
    let _g = ScopedRegionOwnership::new(RegionOwnership::On);
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
    use crate::pipeline::compile_file_repl;

    // Per-run steady-state live-region growth over 50 runs of the same compiled
    // bytecode, with the ownership flag scoped to this whole compile+run.
    fn steady_growth(ownership: RegionOwnership) -> i64 {
        let _g = ScopedRegionOwnership::new(ownership);
        let mut rt = Runtime::without_stdlib();
        let src = "(begin (let [root (@array) a (@array) b (@array)] \
                           (begin (%array-push a b) (%array-push b a) \
                                  (%array-push root a) (%array-push root b) nil)) \
                         nil)";
        let result = {
            let (_vm, symbols, cctx) = rt.parts();
            compile_file_repl(src, symbols, cctx, "<embed>")
                .expect("compiles")
                .0
        };
        // Warm-up run, then measure the steady state across repeated runs.
        {
            let (vm, symbols, cctx) = rt.parts();
            let v = vm
                .execute_scheduled(&result.bytecode, symbols, cctx)
                .expect("runs");
            assert!(v.is_nil(), "the discarded-cycle program returns nil");
        }
        let baseline = rt.heap().active_region_count() as i64;
        for _ in 0..50 {
            let (vm, symbols, cctx) = rt.parts();
            let v = vm
                .execute_scheduled(&result.bytecode, symbols, cctx)
                .expect("runs");
            assert!(v.is_nil());
        }
        rt.heap().active_region_count() as i64 - baseline
    }

    let off = steady_growth(RegionOwnership::Off);
    let on = steady_growth(RegionOwnership::On);
    assert!(
        off > 0,
        "precondition: the interior a↔b cycle must leak flag-off (per-run region growth \
         {off}); if 0, the shape no longer forms an uncollectable cycle and the test no \
         longer bites",
    );
    assert!(
        on <= 0,
        "under --region-ownership the interior cycle must be reclaimed by subtree drop — \
         per-run live-region growth {on} must be <= 0 (flag-off leaks {off} per run)",
    );

    // The checked-on (native-Call) production face: the stores are opaque `Funnel`
    // calls whose containment is funnel-recovered, and the adopt is keyed at the
    // funnel call site (region-model.md § "The funnel adopt — the checked-on store
    // face"); the cut must reclaim identically.
    let _ci = crate::config::test_override::ScopedCheckedIntrinsics::new(true);
    let off_checked = steady_growth(RegionOwnership::Off);
    let on_checked = steady_growth(RegionOwnership::On);
    assert!(
        off_checked > 0,
        "precondition (checked-on): the interior cycle must leak flag-off (growth \
         {off_checked})",
    );
    assert!(
        on_checked <= 0,
        "the funnel-adopt must reclaim the interior cycle on the checked-on path too — \
         per-run growth {on_checked} must be <= 0 (flag-off leaks {off_checked})",
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
    let _g = ScopedRegionOwnership::new(RegionOwnership::On);
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
    use crate::pipeline::compile_file_repl;

    fn steady_growth(ownership: RegionOwnership) -> i64 {
        let _g = ScopedRegionOwnership::new(ownership);
        let mut rt = Runtime::without_stdlib();
        // a ⊇ b (push a b); b ⊇ a (push b a). No container holds a or b — the bare cycle.
        let src = "(begin (let [a (@array) b (@array)] \
                           (begin (%array-push a b) (%array-push b a) nil)) \
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
            assert!(v.is_nil(), "the discarded bare-cycle program returns nil");
        }
        let baseline = rt.heap().active_region_count() as i64;
        for _ in 0..50 {
            let (vm, symbols, cctx) = rt.parts();
            let v = vm
                .execute_scheduled(&result.bytecode, symbols, cctx)
                .expect("runs");
            assert!(v.is_nil());
        }
        rt.heap().active_region_count() as i64 - baseline
    }

    let off = steady_growth(RegionOwnership::Off);
    let on = steady_growth(RegionOwnership::On);
    assert!(
        off > 0,
        "precondition: the bare a↔b cycle must leak flag-off (per-run region growth \
         {off}); if 0, the shape no longer forms an uncollectable cycle and the test no \
         longer bites",
    );
    assert!(
        on <= 0,
        "under --region-ownership the bare cycle must be reclaimed by the co-owned group \
         free — per-run live-region growth {on} must be <= 0 (flag-off leaks {off} per run)",
    );

    // The checked-on (native-Call) production face: the group walk reads the same
    // funnel-recovered containment (`ownership_inputs`), and its `FreeRegionGroup`
    // emit is value-resolved (member slots, no store opcode), so the bare cycle must
    // reclaim identically once `--region-ownership` runs checked-on.
    let _ci = crate::config::test_override::ScopedCheckedIntrinsics::new(true);
    let off_checked = steady_growth(RegionOwnership::Off);
    let on_checked = steady_growth(RegionOwnership::On);
    assert!(
        off_checked > 0,
        "precondition (checked-on): the bare cycle must leak flag-off (growth \
         {off_checked})",
    );
    assert!(
        on_checked <= 0,
        "the co-owned group free must reclaim the bare cycle on the checked-on path \
         too — per-run growth {on_checked} must be <= 0 (flag-off leaks {off_checked})",
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
    use crate::pipeline::compile_file_repl;

    fn steady_growth(ownership: RegionOwnership) -> i64 {
        let _g = ScopedRegionOwnership::new(ownership);
        let mut rt = Runtime::without_stdlib();
        // root ⊇ a (push root a); a ⊇ b (push a b); b ⊇ a (push b a) — the a↔b cycle is
        // nested under `a`, reachable from the root ONLY through `a`. The `root` push is
        // last so its decref_point post-dominates the members (the lifetime obligation).
        let src = "(begin (let [root (@array) a (@array) b (@array)] \
                           (begin (%array-push a b) (%array-push b a) \
                                  (%array-push root a) nil)) \
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
            assert!(v.is_nil(), "the discarded nested-cycle program returns nil");
        }
        let baseline = rt.heap().active_region_count() as i64;
        for _ in 0..50 {
            let (vm, symbols, cctx) = rt.parts();
            let v = vm
                .execute_scheduled(&result.bytecode, symbols, cctx)
                .expect("runs");
            assert!(v.is_nil());
        }
        rt.heap().active_region_count() as i64 - baseline
    }

    let off = steady_growth(RegionOwnership::Off);
    let on = steady_growth(RegionOwnership::On);
    assert!(
        off > 0,
        "precondition: the nested a↔b cycle must leak flag-off (per-run region growth \
         {off}); if 0, the shape no longer forms an uncollectable nested cycle and the \
         test no longer bites",
    );
    assert!(
        on <= 0,
        "under --region-ownership the deep-nesting cut must reclaim the nested cycle by \
         adopting `b` through `a` and subtree-dropping from the root — per-run live-region \
         growth {on} must be <= 0 (flag-off leaks {off} per run)",
    );

    // The checked-on (native-Call) production face: the multi-level containment is
    // funnel-recovered — `b`'s adopt through its actual parent `a` must be keyed at
    // `(%array-push a b)`'s funnel call site exactly as at the intrinsic store
    // (region-model.md § "The funnel adopt — the checked-on store face").
    let _ci = crate::config::test_override::ScopedCheckedIntrinsics::new(true);
    let off_checked = steady_growth(RegionOwnership::Off);
    let on_checked = steady_growth(RegionOwnership::On);
    assert!(
        off_checked > 0,
        "precondition (checked-on): the nested cycle must leak flag-off (growth \
         {off_checked})",
    );
    assert!(
        on_checked <= 0,
        "the funnel-adopt must reclaim the nested cycle on the checked-on path too — \
         per-run growth {on_checked} must be <= 0 (flag-off leaks {off_checked})",
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
    let _g = ScopedRegionOwnership::new(RegionOwnership::On);
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

/// End-to-end reclamation of the **capture-back-edge cycle** — the activation-owner cut
/// (docs/impl/region-model.md § "Owner nodes" — "The capture-back-edge SCC"; inference
/// pin `regions::tests::adopt::activation_adopts_capture_back_edge_scc`). A Fresh container
/// `root` holds `m` (store `root ⊇ m`), `m` holds a closure `c` (store `m ⊇ c`), and `c`
/// captures `m` back (capture `c ⊇ m`) — the m↔c cycle through a closure env. No REGION root
/// can own it: `m` is captured, so its `decref_point` over-extends past the closure while its
/// own `DecrefValueRegion` stays live — the owner-aware lifetime obligation refuses (the
/// permanent refusal `adopt_edges_refuses_captured_store_member_on_lifetime` pins; before it,
/// flag-on freed `m` at the root's subtree drop and the trailing decref-value SIGSEGV'd under
/// guardfree — running 50× panic-clean keeps guarding that over-free). The ACTIVATION owns it
/// instead: both members are `AdoptIntoActivation`'d at the SCC's enclosing scope, their own
/// decrefs suppressed, and the activation's completion release subtree-drops the cycle —
/// interior m↔c references reclaiming with the set.
///
/// The flag-off measurement is the built-in counterfactual: per-region RC cannot collect the
/// m↔c cycle (region-rules.md Rule 8), so the SAME bytecode shape must leak flag-off and be
/// bounded flag-on — proving the cut, not the shape, reclaims it. Both `--checked-intrinsics`
/// settings are pinned: checked-off the interior store is a `cross_region_refs` edge;
/// checked-on it is funnel-recovered `containment_edges` (the value-resolved adopt needs no
/// store site), so the cut serves the production path identically.
#[test]
fn region_ownership_capture_back_edge_cycle_reclaims() {
    use crate::pipeline::compile_file_repl;

    fn steady_growth(ownership: RegionOwnership) -> i64 {
        let _g = ScopedRegionOwnership::new(ownership);
        let mut rt = Runtime::without_stdlib();
        let src = "(begin (let [root (@array) m (@array)] \
                            (let [c (fn [] (length m))] \
                              (begin (%array-push m c) (c) (%array-push root m) nil))) \
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
            assert!(v.is_nil(), "the capture-back-edge program returns nil");
        }
        let baseline = rt.heap().active_region_count() as i64;
        // 50 runs; flag-on, a broken adopt or a doubled member release trips the
        // debug generation/decref asserts, so completing panic-clean is itself
        // the soundness half of the pin.
        for _ in 0..50 {
            let (vm, symbols, cctx) = rt.parts();
            let v = vm
                .execute_scheduled(&result.bytecode, symbols, cctx)
                .expect("runs");
            assert!(v.is_nil());
        }
        rt.heap().active_region_count() as i64 - baseline
    }

    let off = steady_growth(RegionOwnership::Off);
    let on = steady_growth(RegionOwnership::On);
    assert!(
        off > 0,
        "precondition: the m↔c capture-back-edge cycle must leak flag-off (per-run \
         region growth {off}); if 0, the shape no longer forms an uncollectable cycle \
         and the test no longer bites",
    );
    assert!(
        on <= 0,
        "under --region-ownership the capture-back-edge cycle must be reclaimed by the \
         activation owner node's completion release — per-run live-region growth {on} \
         must be <= 0 (flag-off leaks {off} per run)",
    );

    // The checked-on (native-Call) production face: the interior store is
    // funnel-recovered containment, and the cut must reclaim identically.
    let _ci = crate::config::test_override::ScopedCheckedIntrinsics::new(true);
    let off_checked = steady_growth(RegionOwnership::Off);
    let on_checked = steady_growth(RegionOwnership::On);
    assert!(
        off_checked > 0,
        "precondition (checked-on): the cycle must leak flag-off (growth {off_checked})",
    );
    assert!(
        on_checked <= 0,
        "the activation cut must reclaim the funnel-recovered cycle on the checked-on \
         path too — per-run growth {on_checked} must be <= 0 (flag-off leaks \
         {off_checked})",
    );
}

/// End-to-end reclamation of the **transferred returned cycle** — the
/// consuming-activation owner cut (docs/impl/region-model.md § "Owner nodes" —
/// "The transferred returned subtree"; inference pin
/// `regions::tests::adopt::transfer_adopts_returned_cycle_to_consumer`). A
/// producer `mk` builds an a↔b cycle and returns its root; the top-level
/// consumer discards it. No region root can own it (the root crosses the
/// return frontier) and per-region RC cannot collect the cycle, so flag-off it
/// leaks per call. Under `--region-ownership` the producer's interior adopt
/// hangs `b` under the returned root and the consumer's release is replaced by
/// `AdoptIntoActivation`, so the activation's completion release set-drops the
/// whole cycle.
///
/// The flag-off measurement is the built-in counterfactual: the SAME bytecode
/// shape must leak flag-off and be bounded flag-on. Both `--checked-intrinsics`
/// settings are pinned: checked-off the interior store is a `cross_region_refs`
/// edge; checked-on it is funnel-recovered containment whose adopt is keyed at
/// the funnel call site (the value-resolved adopt needs no store opcode).
#[test]
fn region_ownership_reclaims_returned_cycle_across_calls() {
    use crate::pipeline::compile_file_repl;

    fn steady_growth(ownership: RegionOwnership) -> i64 {
        let _g = ScopedRegionOwnership::new(ownership);
        let mut rt = Runtime::without_stdlib();
        let src = "(def mk (fn [] (let [a (@array) b (@array)] \
                     (begin (%array-push a b) (%array-push b a) a)))) \
                   (mk) \
                   (mk) \
                   nil";
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
                "the discarded returned-cycle program returns nil"
            );
        }
        let baseline = rt.heap().active_region_count() as i64;
        // 50 runs; flag-on, a broken adopt or a doubled release trips the debug
        // generation/decref asserts, so completing panic-clean is itself the
        // soundness half of the pin.
        for _ in 0..50 {
            let (vm, symbols, cctx) = rt.parts();
            let v = vm
                .execute_scheduled(&result.bytecode, symbols, cctx)
                .expect("runs");
            assert!(v.is_nil());
        }
        rt.heap().active_region_count() as i64 - baseline
    }

    let off = steady_growth(RegionOwnership::Off);
    let on = steady_growth(RegionOwnership::On);
    assert!(
        off > 0,
        "precondition: the returned a↔b cycle must leak flag-off (per-run region \
         growth {off}); if 0, the shape no longer forms an uncollectable returned \
         cycle and the test no longer bites",
    );
    assert!(
        on <= 0,
        "under --region-ownership the returned cycle must be reclaimed by the \
         consuming activation's owner-node release — per-run live-region growth \
         {on} must be <= 0 (flag-off leaks {off} per run)",
    );

    let _ci = crate::config::test_override::ScopedCheckedIntrinsics::new(true);
    let off_checked = steady_growth(RegionOwnership::Off);
    let on_checked = steady_growth(RegionOwnership::On);
    assert!(
        off_checked > 0,
        "precondition (checked-on): the returned cycle must leak flag-off (growth \
         {off_checked})",
    );
    assert!(
        on_checked <= 0,
        "the transfer cut must reclaim the funnel-recovered returned cycle on the \
         checked-on path too — per-run growth {on_checked} must be <= 0 (flag-off \
         leaks {off_checked})",
    );
}

/// The **fiber face** of the transfer cut: a silent fiber body's terminal value
/// is the returned cycle, handed across the fiber frontier by the completing
/// resume and discarded. Flag-off the cycle leaks per run (the fiber machinery
/// balances its own retains — the cycle is what remains); flag-on the resume
/// result's release is replaced by `AdoptIntoActivation` and the consuming
/// activation's completion reclaims it. Both `--checked-intrinsics` settings.
#[test]
fn region_ownership_reclaims_fiber_terminal_cycle() {
    use crate::pipeline::compile_file_repl;

    fn steady_growth(ownership: RegionOwnership) -> i64 {
        let _g = ScopedRegionOwnership::new(ownership);
        let mut rt = Runtime::without_stdlib();
        let src = "(begin (let [f (fiber/new (fn [] (let [a (@array) b (@array)] \
                     (begin (%array-push a b) (%array-push b a) a))) 1)] \
                     (begin (fiber/resume f) nil)) \
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
                "the discarded fiber-terminal program returns nil"
            );
        }
        let baseline = rt.heap().active_region_count() as i64;
        for _ in 0..50 {
            let (vm, symbols, cctx) = rt.parts();
            let v = vm
                .execute_scheduled(&result.bytecode, symbols, cctx)
                .expect("runs");
            assert!(v.is_nil());
        }
        rt.heap().active_region_count() as i64 - baseline
    }

    let off = steady_growth(RegionOwnership::Off);
    let on = steady_growth(RegionOwnership::On);
    assert!(
        off > 0,
        "precondition: the fiber-terminal cycle must leak flag-off (per-run region \
         growth {off}); if 0, the shape no longer bites",
    );
    assert!(
        on <= 0,
        "under --region-ownership the fiber-terminal cycle must be reclaimed at the \
         consuming activation's completion — per-run growth {on} must be <= 0 \
         (flag-off leaks {off} per run)",
    );

    let _ci = crate::config::test_override::ScopedCheckedIntrinsics::new(true);
    let off_checked = steady_growth(RegionOwnership::Off);
    let on_checked = steady_growth(RegionOwnership::On);
    assert!(
        off_checked > 0,
        "precondition (checked-on): the fiber-terminal cycle must leak flag-off \
         (growth {off_checked})",
    );
    assert!(
        on_checked <= 0,
        "the fiber face must reclaim on the checked-on path too — per-run growth \
         {on_checked} must be <= 0 (flag-off leaks {off_checked})",
    );
}

/// The transfer adopt **rides parks and the fiber teardown** — the S7 wiring,
/// exercised end-to-end by production-emitted adopts. The consumer is a FIBER
/// BODY that calls the producer, yields (parking its activation node with the
/// adopted cycle), and either completes (the resumed body's clean break frees
/// node + members) or is hard-killed mid-park (`fiber/cancel` → the terminal
/// teardown frees the parked node's members). The carrier-retain residue of
/// suspending resumes leaks identically at BOTH flag settings (a pre-existing
/// class, not this cut's), so the counterfactual is the flag DELTA: flag-on
/// must reclaim the cycles' regions on top of whatever both settings leak.
#[test]
fn region_ownership_transfer_adopt_rides_parks_and_fiber_teardown() {
    use crate::pipeline::compile_file_repl;

    fn steady_growth(ownership: RegionOwnership, src: &str) -> i64 {
        let _g = ScopedRegionOwnership::new(ownership);
        let mut rt = Runtime::without_stdlib();
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
            assert!(v.is_nil(), "the parked-consumer program returns nil");
        }
        let baseline = rt.heap().active_region_count() as i64;
        for _ in 0..50 {
            let (vm, symbols, cctx) = rt.parts();
            let v = vm
                .execute_scheduled(&result.bytecode, symbols, cctx)
                .expect("runs");
            assert!(v.is_nil());
        }
        rt.heap().active_region_count() as i64 - baseline
    }

    // Drained to completion: two cycles adopted into the body's activation
    // node, a yield parking the node between them; the resumed body's normal
    // completion frees node + members.
    let complete = "(def mk (fn [] (let [a (@array) b (@array)] \
                      (begin (%array-push a b) (%array-push b a) a)))) \
                    (let [f (fiber/new (fn [] (begin (mk) (emit :yield 0) (mk) nil)) 2)] \
                      (begin (fiber/resume f) (fiber/resume f) nil)) \
                    nil";
    let off = steady_growth(RegionOwnership::Off, complete);
    let on = steady_growth(RegionOwnership::On, complete);
    assert!(
        off > on && off - on >= 3,
        "the two in-fiber cycles (4 regions/run) must be reclaimed across the park \
         at the body's completion: flag-off growth {off} vs flag-on {on} (the \
         residual both settings share is the suspending-resume carrier retain, not \
         this cut's)",
    );

    // Hard-killed mid-park: the first cycle is adopted, the body parks at the
    // yield, and `fiber/cancel` tears the fiber down — the kill must free the
    // parked activation node's members (the second cycle is never built).
    let cancel = "(def mk (fn [] (let [a (@array) b (@array)] \
                    (begin (%array-push a b) (%array-push b a) a)))) \
                  (let [f (fiber/new (fn [] (begin (mk) (emit :yield 0) (mk) nil)) 2)] \
                    (begin (fiber/resume f) (fiber/cancel f :dead) nil)) \
                  nil";
    let off = steady_growth(RegionOwnership::Off, cancel);
    let on = steady_growth(RegionOwnership::On, cancel);
    assert!(
        off > on && off - on >= 1,
        "the parked cycle (2 regions/run) must be freed by the hard kill's \
         terminal teardown: flag-off growth {off} vs flag-on {on}",
    );
}

/// VM≡JIT parity for the transfer cut: the producer's interior `AdoptRegion`
/// and the consumer's `AdoptIntoActivation` + owner-node completion release all
/// run through compiled code. The consumer wrapper carries no `MakeClosure`
/// (the producer is a top-level def), so it JIT-compiles; `jit_compiled` guards
/// a vacuous reading exactly as the S-series JIT pins do.
#[cfg(feature = "jit")]
#[test]
fn region_ownership_reclaims_returned_cycle_under_jit() {
    use crate::config::JitPolicy;
    use crate::pipeline::compile_file_repl;

    fn growth(ownership: RegionOwnership) -> (i64, bool) {
        let _g = ScopedRegionOwnership::new(ownership);
        let mut rt = Runtime::without_stdlib();
        rt.vm().runtime_config.jit = JitPolicy::Eager;
        let src = "(def mk (fn [] (let [a (@array) b (@array)] \
                     (begin (%array-push a b) (%array-push b a) a)))) \
                   ((fn [] (begin (mk) nil))) \
                   nil";
        let prog = {
            let (_vm, symbols, cctx) = rt.parts();
            compile_file_repl(src, symbols, cctx, "<embed>")
                .expect("compiles")
                .0
        };
        {
            let (vm, symbols, cctx) = rt.parts();
            let v = vm
                .execute_scheduled(&prog.bytecode, symbols, cctx)
                .expect("runs (submits the JIT task)");
            assert!(v.is_nil());
        }
        rt.vm().drain_jit_pending();
        let jit_compiled = !rt.vm().jit_cache.is_empty();
        {
            let (vm, symbols, cctx) = rt.parts();
            let v = vm
                .execute_scheduled(&prog.bytecode, symbols, cctx)
                .expect("runs");
            assert!(v.is_nil());
        }
        let baseline = rt.heap().active_region_count() as i64;
        for _ in 0..50 {
            let (vm, symbols, cctx) = rt.parts();
            let v = vm
                .execute_scheduled(&prog.bytecode, symbols, cctx)
                .expect("runs");
            assert!(v.is_nil());
        }
        (
            rt.heap().active_region_count() as i64 - baseline,
            jit_compiled,
        )
    }

    let (on, jit_compiled) = growth(RegionOwnership::On);
    assert!(
        jit_compiled,
        "the consumer wrapper must JIT-compile under the flag — an empty jit_cache \
         means a worker died (e.g. on a missing translate arm)",
    );
    let (off, _) = growth(RegionOwnership::Off);
    assert!(
        off > 0,
        "precondition: the returned cycle must leak flag-off under the JIT \
         (per-run growth {off})",
    );
    assert!(
        on <= 0,
        "under --region-ownership the JIT-compiled consumer must reclaim the \
         returned cycle — per-run growth {on} must be <= 0 (flag-off leaks {off})",
    );
}

/// The consumer-facing adopt channel is IDEMPOTENT on an already-Owned child:
/// delivering one region to `AdoptIntoActivation` twice within an activation (a
/// masked-`:error` fiber restarted after handing out the same payload) leaves
/// it owned by the first adopt instead of tripping `adopt_region`'s one-owner
/// assert, and the completion release frees it exactly once.
#[test]
fn adopt_into_activation_absorbs_redelivery() {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use std::rc::Rc;

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    for _ in 0..50 {
        let (child, child_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let gen_before = unsafe { &*heap_ptr }.generation_raw(child_rid.get());

        // Body: adopt the same member twice, then return — the second adopt
        // must be a structural no-op (the debug one-owner assert would
        // otherwise detonate mid-loop).
        let mut bc = Bytecode::new();
        let idx = bc.add_constant(child);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        bc.emit(Instruction::AdoptIntoActivation);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        bc.emit(Instruction::AdoptIntoActivation);
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Return);
        let code = crate::value::Code::new(
            Rc::new(bc.instructions),
            Rc::new(bc.constants),
            Rc::new(crate::error::LocationMap::new()),
            Rc::new(vec![]),
        );

        let result = vm.execute_bytecode_saving_stack(&code, &Rc::new(vec![]));
        assert!(
            result.bits.is_ok(),
            "the double-adopt body completes normally"
        );
        let gen_after = unsafe { &*heap_ptr }.generation_raw(child_rid.get());
        assert!(
            gen_after > gen_before,
            "the twice-delivered member must be freed exactly once, by the node's \
             completion release (gen {gen_before} -> {gen_after})",
        );
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "node + member must be reclaimed at each activation's completion — live \
         region count must not grow (baseline={baseline}, after 50 activations={after})",
    );
}

/// Boundary pin for the capture-adopt admission (region-model.md § "The capture
/// adopt"): the nested-closure-captures-its-encloser family — the only shape family
/// whose owner-capture is an UPVALUE and which external uniqueness admits — must stay
/// on the per-region-RC baseline under --region-ownership. The nested closure's
/// region is minted per CALL of the enclosing closure, so adopting the longer-lived
/// member would free it under the encloser's still-live env reference and re-adopt an
/// already-Owned region on the next construction; the lifetime obligation refuses the
/// shape by construction (the forwarding capture pins the member's tight last-use
/// at/past the enclosing lambda's own node, which post-dates the nested root's
/// in-body drop in post-order).
///
/// Two facets, over a body that constructs the nested closure TWICE per run
/// (`(e true)` recurses once through `(e false)`):
/// - flag-on runs PANIC-CLEAN: an over-admission would emit the adopt at o's
///   construction, and the second construction would re-adopt the already-Owned
///   member — the one-owner debug assert (or a generation panic) detonates;
/// - flag-on growth EQUALS flag-off growth: the family is refused to Shared, so the
///   flag changes nothing (the built-in flag-neutrality counterfactual).
///
/// Both --checked-intrinsics settings: the shape's %pair/%first lower as intrinsics
/// checked-off and as native calls checked-on; the refusal must hold on both paths.
#[test]
fn region_ownership_upvalue_capture_family_stays_on_baseline() {
    let src = "(begin (let [m (%pair 1 2)] \
                 (letrec [e (fn [k] (let [o (fn [] (if k (e false) (%first m)))] (o)))] \
                   (begin (e true) nil))) \
               nil)";
    let off = closure_cycle_growth(RegionOwnership::Off, src);
    let on = closure_cycle_growth(RegionOwnership::On, src);
    assert_eq!(
        on, off,
        "the upvalue-captured member must stay on the RC baseline: per-run region \
         growth must be identical flag-on ({on}) and flag-off ({off}) — a divergence \
         means the ownership cut claimed (or leaked) the refused family",
    );
    let _ci = crate::config::test_override::ScopedCheckedIntrinsics::new(true);
    let off_checked = closure_cycle_growth(RegionOwnership::Off, src);
    let on_checked = closure_cycle_growth(RegionOwnership::On, src);
    assert_eq!(
        on_checked, off_checked,
        "the refusal must hold on the checked-intrinsics (native-Call) path too: \
         per-run growth flag-on ({on_checked}) vs flag-off ({off_checked})",
    );
}

/// A TOP-LEVEL (file-letrec) mutable reassigned in a loop must
/// not accumulate its overwritten PRIOR values until frame teardown (the over-keep
/// blocker; docs/impl/region-bindings.md "Reassigned mutable bindings are 1-slot
/// containers"). Each prior is
/// dead the instant the next `assign` displaces it; holding it to program exit is
/// unbounded RSS growth in a long-running loop (the production blocker).
///
/// The 1-slot container model (docs/impl/region-bindings.md) suppresses the
/// module-scope value's ordinary decref and lets the cell ADOPT that producer
/// reference — so the drop-on-overwrite is its sole release and the lowerer must
/// emit NO incref-on-store (`donated_overwrite_sites`). The bug an unbalanced
/// incref-on-store reintroduces: `born + store − overwrite = +1` per displaced
/// prior, every value the cell ever held standing until program exit.
///
/// Oracle: per-iteration live-region growth via
/// `arena/region-count`, sampled mid-run BY THE PROGRAM (after 50 and after 250
/// reassigns) and returned as the raw region-count delta — not emitted RC, and not
/// a post-run host probe (which also counts scheduler/teardown state). A prompt
/// prior-release keeps the delta near zero (each prior's region recycles at its
/// overwrite); the over-keep grows it by ~1 per added iteration (~200).
///
/// The self-referential accumulator `(assign acc (%pair n acc))` is the built-in
/// discriminator: there every prior IS live (chained into the next pair), so its
/// delta legitimately grows by ~200. It proves the measurement actually detects
/// per-iteration region growth — so a near-zero delta for the non-self-ref shape is
/// a real reclamation, not a dead gauge.
#[test]
fn reassign_toplevel_prior_release_is_bounded() {
    use crate::pipeline::compile_file_repl;

    // Run an Elle program that reassigns a top-level `@acc` 50 then 250 times,
    // sampling `arena/region-count` mid-run at each point, and return the raw
    // count delta (c250 − c50) the program computes. Compiled on the **checked-on**
    // (native-Call) path — the production default (`elle FILE`), where the reassign
    // value is an opaque call-result the 1-slot container model claims; the
    // `Runtime::new()` test default is checked-off, a distinct region path.
    fn region_growth(assign_form: &str) -> i64 {
        let _ci = crate::config::test_override::ScopedCheckedIntrinsics::new(true);
        let mut rt = Runtime::new();
        let src = format!(
            "(def @acc nil) (var n 0) \
             (while (%lt n 50) {assign_form} (assign n (%add n 1))) \
             (def c50 (arena/region-count)) \
             (while (%lt n 250) {assign_form} (assign n (%add n 1))) \
             (def c250 (arena/region-count)) \
             (%sub c250 c50)"
        );
        let result = {
            let (_vm, symbols, cctx) = rt.parts();
            compile_file_repl(&src, symbols, cctx, "<embed>")
                .expect("compiles")
                .0
        };
        let (vm, symbols, cctx) = rt.parts();
        vm.execute_scheduled(&result.bytecode, symbols, cctx)
            .expect("runs")
            .as_int()
            .expect("program returns the region-count delta as an int")
    }

    // Fresh `(%pair n n)` each iteration: the prior is genuinely dead once the next
    // `assign` displaces it, so a prompt drop-on-overwrite keeps the region count
    // flat across the extra 200 iterations.
    let dead_prior_growth = region_growth("(assign acc (%pair n n))");
    // The discriminator: `(%pair n acc)` chains every prior into the next pair, so
    // they are all live — the region count legitimately grows ~1 per iteration.
    let live_chain_growth = region_growth("(assign acc (%pair n acc))");

    assert!(
        live_chain_growth > 150,
        "precondition: the self-referential accumulator legitimately retains every \
         prior (the chain is live), so region growth over 200 iterations must be \
         large (~200) — got {live_chain_growth}; if small, the measurement is not \
         seeing per-iteration region growth and the assertion below is vacuous",
    );
    assert!(
        dead_prior_growth < 50,
        "a top-level mutable reassigned to a fresh (dead) value in a loop must \
         release each displaced prior at its overwrite, not hold it to frame \
         teardown — region growth over 200 iterations must be \
         near zero, got {dead_prior_growth} (~200 means every prior is over-kept \
         until program exit, the unbalanced incref-on-store)",
    );
}

/// Per-run steady-state live-region growth over 50 runs of one compiled program,
/// under the given ownership flag. The closure-cycle MERGE is UNCONDITIONAL (it
/// rides `compute_merges`, not the `--region-ownership` spike), so a letrec closure
/// cycle must read bounded at EITHER flag setting — that flag-independence is what
/// makes it all-tier. `without_stdlib` keeps the measurement to region count; the
/// trustworthy UAF oracle is full-stdlib `--trace=guardfree` (the elle corpus).
fn closure_cycle_growth(ownership: RegionOwnership, src: &str) -> i64 {
    use crate::pipeline::compile_file_repl;
    let _g = ScopedRegionOwnership::new(ownership);
    let mut rt = Runtime::without_stdlib();
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
            "the discarded closure-cycle program returns nil"
        );
    }
    let baseline = rt.heap().active_region_count() as i64;
    for _ in 0..50 {
        let (vm, symbols, cctx) = rt.parts();
        let v = vm
            .execute_scheduled(&result.bytecode, symbols, cctx)
            .expect("runs");
        assert!(v.is_nil());
    }
    rt.heap().active_region_count() as i64 - baseline
}

/// The built-in live-growth discriminator: a bare `@array` MUTABLE mutual cycle.
/// Its members are call-result regions the closure-cycle MERGE cannot name (no
/// static slot), per-region RC cannot collect the cycle (region-rules.md Rule 8),
/// and — flag-off — no co-owned-group cut reclaims it either. So it leaks per run.
/// A near-zero closure-cycle growth is real reclamation ONLY beside a positive
/// discriminator growth (else the gauge is dead and every "bounded" reading void).
const LEAK_DISCRIMINATOR: &str =
    "(begin (let [a (@array) b (@array)] (%array-push a b) (%array-push b a) nil) nil)";

/// End-to-end reclamation of the **mutually-recursive** immutable closure cycle by
/// the closure-cycle MERGE. A local `letrec` (`ping`/`pong`) builds an immutable
/// reference cycle: each closure's env references the other, captured at the
/// `letrec`, never mutated. Per-region RC cannot collect it (region-rules.md Rule
/// 8); unlike a *mutable* `@array` cycle (the deliberate class-8 boundary) an
/// immutable one is reclaimable, and the merge collapses the closures+cells onto one
/// arena freed at the enclosing scope.
///
/// The merge is unconditional, so the counterfactual is bounded-vs-discriminator,
/// not flag-off-vs-on (the flag no longer discriminates): the cycle must read
/// bounded at BOTH flag settings, while the `LEAK_DISCRIMINATOR` (a bare @array
/// cycle) leaks flag-off — proving the gauge detects per-run region growth.
#[test]
fn region_ownership_reclaims_mutual_recursion_closure_cycle() {
    let leak = closure_cycle_growth(RegionOwnership::Off, LEAK_DISCRIMINATOR);
    assert!(
        leak > 0,
        "precondition: the bare @array mutual cycle must leak flag-off (per-run region \
         growth {leak}); if 0 the gauge is not detecting per-run growth and the bounded \
         assertions below are vacuous",
    );
    // ping <-> pong: two closures whose envs reference each other (immutable cycle).
    let src = "(begin (letrec [ping (fn [n] (if (%lt n 1) :done (pong (%sub n 1)))) \
                               pong (fn [n] (ping n))] \
                        (ping 3)) \
                     nil)";
    let off = closure_cycle_growth(RegionOwnership::Off, src);
    let on = closure_cycle_growth(RegionOwnership::On, src);
    assert!(
        off <= 0,
        "the immutable ping/pong closure cycle must be reclaimed by the UNCONDITIONAL \
         closure-cycle merge (flag-off) — per-run live-region growth {off} must be <= 0 \
         (the discriminator leaks {leak} per run, so the gauge is live)",
    );
    assert!(
        on <= 0,
        "the merge is flag-independent, so the cycle must also be bounded flag-on — \
         per-run live-region growth {on} must be <= 0",
    );
}

/// PROMPTNESS of the closure-cycle merge's drop site (docs/impl/region-model.md
/// § The letrec closure-cycle merge; the §9 promptness ledger). A *discarded*
/// top-level letrec closure cycle must be freed at its BINDING SCOPE — the `letrec`
/// that prebinds its capture cells — its true last use, NOT held to the enclosing
/// post-dominator (the file `Begin`, i.e. program teardown). The capture cell is
/// keyed by the letrec NODE, whose enclosing-scope stack excludes itself, so the
/// allocation-site post-dominator dropped at the letrec's PARENT (the file Begin for
/// a top-level cycle) — a program-duration over-keep that, summed over many such
/// cycles, is unbounded RSS.
///
/// Oracle: build N distinct top-level letrec cycles between two `arena/region-count`
/// samples. Each merged cycle is one region. DISCARDED (used, then dropped), each
/// must free at its own letrec, so the count delta stays near zero. The
/// DISCRIMINATOR retains each cycle's closure in a program-lifetime array — a
/// cross-region store that RC-pins the merged region — so the delta legitimately
/// grows ~N, proving the gauge detects per-cycle region retention (else a dead gauge
/// paints the discarded case green for free). The merge fires identically in both;
/// only the external RC holder differs. The store is the same shape that makes the
/// earlier (enclosing-Begin) drop sound: a foreign reference into the merged region
/// is RC-counted and outlives the single decref.
#[test]
fn closure_cycle_discarded_release_is_prompt() {
    use crate::pipeline::compile_file_repl;

    // N distinct top-level letrec cycles between two region-count samples. RETAIN
    // splices a push of each cycle's closure into a program-lifetime `@keep`
    // (RC-pinned → grows the count); the discard variant uses then drops it.
    fn region_growth(retain: bool) -> i64 {
        const N: usize = 200;
        let mut rt = Runtime::without_stdlib();
        let mut src = String::from("(def @keep @[])\n(def c0 (arena/region-count))\n");
        for k in 0..N {
            let body = if retain {
                format!("(%array-push keep r{k})")
            } else {
                format!("(r{k} 2)")
            };
            src.push_str(&format!(
                "(letrec [r{k} (fn [m] (if (%lt m 1) :done (r{k} (%sub m 1))))] {body})\n"
            ));
        }
        src.push_str("(def c1 (arena/region-count))\n(%sub c1 c0)");
        let result = {
            let (_vm, symbols, cctx) = rt.parts();
            compile_file_repl(&src, symbols, cctx, "<embed>")
                .expect("compiles")
                .0
        };
        let (vm, symbols, cctx) = rt.parts();
        vm.execute_scheduled(&result.bytecode, symbols, cctx)
            .expect("runs")
            .as_int()
            .expect("program returns the region-count delta as an int")
    }

    let discarded = region_growth(false);
    let retained = region_growth(true);
    assert!(
        retained > 150,
        "precondition: retaining each cycle's closure in a program-lifetime array \
         legitimately grows the live region count ~N (got {retained}); if small, the \
         gauge is not detecting per-cycle retention and the assertion below is vacuous",
    );
    assert!(
        discarded < 50,
        "a discarded top-level letrec closure cycle must be freed at its binding-scope \
         letrec, not held to program teardown — region growth over 200 cycles must be \
         near zero, got {discarded} (~200 means each merged cycle survives to the file \
         Begin scope-exit, the coarse allocation-site drop)",
    );
}

/// Companion to the mutual case: a **self-recursive** `letrec` closure (`loop`
/// references itself) — the most pervasive recursive shape (every recursive local fn).
/// Unlike the mutual cycle this is **cell-free**: the self-edge does not mark `loop`
/// captured (`hir/arena.rs::mark_captured`), so it has no forward cell and no
/// cell↔closure cycle — its self-reference resolves to the executing closure
/// (`LoadSelf` / a self-call). The per-call closure region is reclaimed by ordinary RC
/// (the tail-call adopt for a self-tail-loop), NOT the merge (which serves the
/// cell-bearing mutual cycle). Same bounded-vs-discriminator counterfactual as the
/// mutual case: reclaimed self-recursion reads bounded region growth beside a leaking
/// bare-@array-cycle discriminator, and — being cell-free RC/adopt, flag-independent
/// like the merge — bounded at both flag settings.
#[test]
fn region_ownership_reclaims_self_recursion_closure_cycle() {
    let leak = closure_cycle_growth(RegionOwnership::Off, LEAK_DISCRIMINATOR);
    assert!(
        leak > 0,
        "precondition: the bare @array mutual cycle must leak flag-off (per-run region \
         growth {leak}); if 0 the gauge is not detecting per-run growth and the bounded \
         assertions below are vacuous",
    );
    // loop references itself: a cell-free self-recursion (no forward cell, LoadSelf).
    let src = "(begin (letrec [loop (fn [n] (if (%lt n 1) :done (loop (%sub n 1))))] \
                        (loop 3)) \
                     nil)";
    let off = closure_cycle_growth(RegionOwnership::Off, src);
    let on = closure_cycle_growth(RegionOwnership::On, src);
    assert!(
        off <= 0,
        "the cell-free self-recursive closure must be reclaimed by ordinary RC / the \
         tail-call adopt (flag-off) — per-run live-region growth {off} must be <= 0 (the \
         discriminator leaks {leak} per run, so the gauge is live)",
    );
    assert!(
        on <= 0,
        "reclamation is flag-independent, so the self-recursion must also be bounded \
         flag-on — per-run live-region growth {on} must be <= 0",
    );
}

/// Per-CALL reclamation of a self-recursive local closure (`letrec`) NESTED in a function
/// body, invoked many times within ONE run — the universal shape (every recursive local
/// helper; every variadic operator `+`/`<`, whose body is a `(letrec [go …] …)` over its
/// varargs). The `{self,mutual}_recursion` tests above build a TOP-LEVEL letrec and re-run
/// the whole program, so they never invoke a nested letrec; this drives a nested one many
/// times within one run.
///
/// A self-recursive `loop` is **cell-free**: its self-edge does not mark it captured
/// (`hir/arena.rs::mark_captured`), so there is no forward cell and no cell↔closure cycle —
/// its self-reference resolves to the executing closure (`LoadSelf` / a self-call). The
/// closure is an ordinary per-call region whose demise the recursive `TailCall` strands as
/// dead code; the tail-call adopt (`lir/lower/control/call.rs::tail_callee_adopts`,
/// `stranded_self_bindings`) supplies the once-only release at the recursion's normal
/// completion, so the region is reclaimed per call — RC-identical to a top-level recursive
/// `defn`. (The merge is unrelated here: it serves the cell-bearing MUTUAL cycle, not this
/// cell-free self-recursion.)
///
/// Oracle: per-iteration live-region growth via `arena/region-count`, sampled mid-run BY
/// THE PROGRAM (after 50 then 250 invocations) and returned as the raw delta, exactly as
/// `reassign_toplevel_prior_release_is_bounded` does. The self-referential accumulator
/// `(assign acc (%pair n acc))` is the built-in discriminator: every prior IS live, so
/// its delta legitimately grows ~200 — proving the gauge detects per-iteration growth, so
/// a near-zero delta for the letrec call is real reclamation, not a dead gauge.
#[test]
fn closure_cycle_nested_letrec_reclaims_per_call() {
    use crate::pipeline::compile_file_repl;

    // Run `body` 50 then 250 times in a single program, sampling `arena/region-count`
    // at each point, and return the raw count delta (c250 − c50) the program computes.
    fn region_growth(prelude: &str, body: &str) -> i64 {
        let mut rt = Runtime::new();
        let src = format!(
            "{prelude} (var n 0) \
             (while (%lt n 50) {body} (assign n (%add n 1))) \
             (def c50 (arena/region-count)) \
             (while (%lt n 250) {body} (assign n (%add n 1))) \
             (def c250 (arena/region-count)) \
             (%sub c250 c50)"
        );
        let result = {
            let (_vm, symbols, cctx) = rt.parts();
            compile_file_repl(&src, symbols, cctx, "<embed>")
                .expect("compiles")
                .0
        };
        let (vm, symbols, cctx) = rt.parts();
        vm.execute_scheduled(&result.bytecode, symbols, cctx)
            .expect("runs")
            .as_int()
            .expect("program returns the region-count delta as an int")
    }

    // Subject: `f` wraps a self-recursive letrec closure; each `(f 3)` builds and discards
    // one cell↔closure cycle, which must be reclaimed per call.
    let call_growth = region_growth(
        "(def f (fn [k] \
            (letrec [loop (fn [m] (if (%lt m 1) :done (loop (%sub m 1))))] \
              (loop k))))",
        "(f 3)",
    );
    // Discriminator: the self-referential accumulator legitimately retains every prior.
    let live_chain_growth = region_growth("(def @acc nil)", "(assign acc (%pair n acc))");

    assert!(
        live_chain_growth > 150,
        "precondition: the self-referential accumulator legitimately retains every prior, \
         so region growth over 200 iterations must be large (~200) — got \
         {live_chain_growth}; if small, the gauge is not seeing per-iteration region \
         growth and the assertion below is vacuous",
    );
    assert!(
        call_growth < 50,
        "a cell-free self-recursive local closure nested in an invoked function must be \
         reclaimed per call by the tail-call adopt — region growth over 200 calls must be \
         near zero, got {call_growth} (each call's stranded closure region leaks to program \
         teardown if the adopt does not supply its release)",
    );
}

/// Per-CALL reclamation of an in-lambda MUTUAL letrec cycle — the closure-cycle
/// merge's in-lambda case (docs/impl/region-model.md § The letrec closure-cycle
/// merge; oracle.lisp `recur-local-mutual`). Each `(f 3)` builds one ev↔od
/// cell↔closure cycle inside `f`'s body; the merge collapses the four members
/// (two closures + two forward cells) onto one arena, and — the letrec body
/// `(ev k)` being a tail call to a member — the tail-call adopt releases that
/// arena once at the recursion's normal completion. `(f 0)` is the base-case-only
/// path: the recursion never rotates to a sibling, so the ENTRY call's adopt is
/// the sole release channel — a marking that only covered interior rotations
/// would leak exactly this path.
///
/// The merge is unconditional (it rides `compute_merges`, not the
/// `--region-ownership` spike), so growth must be bounded at BOTH flag settings.
/// Oracle: per-iteration live-region growth via `arena/region-count`, sampled
/// mid-run BY THE PROGRAM (after 50 then 250 calls), beside the self-referential
/// accumulator discriminator whose growth proves the gauge is live.
#[test]
fn region_ownership_reclaims_nested_mutual_recursion_per_call() {
    use crate::pipeline::compile_file_repl;

    // Run `body` 50 then 250 times in one program under the given ownership flag,
    // sampling `arena/region-count` at each point; returns c250 − c50.
    fn region_growth(ownership: RegionOwnership, prelude: &str, body: &str) -> i64 {
        let _g = ScopedRegionOwnership::new(ownership);
        let mut rt = Runtime::new();
        let src = format!(
            "{prelude} (var n 0) \
             (while (%lt n 50) {body} (assign n (%add n 1))) \
             (def c50 (arena/region-count)) \
             (while (%lt n 250) {body} (assign n (%add n 1))) \
             (def c250 (arena/region-count)) \
             (%sub c250 c50)"
        );
        let result = {
            let (_vm, symbols, cctx) = rt.parts();
            compile_file_repl(&src, symbols, cctx, "<embed>")
                .expect("compiles")
                .0
        };
        let (vm, symbols, cctx) = rt.parts();
        vm.execute_scheduled(&result.bytecode, symbols, cctx)
            .expect("runs")
            .as_int()
            .expect("program returns the region-count delta as an int")
    }

    let prelude = "(def f (fn [k] \
        (letrec [ev (fn [m] (if (%lt m 1) :even (od (%sub m 1)))) \
                 od (fn [m] (if (%lt m 1) :odd (ev (%sub m 1))))] \
          (ev k))))";

    // Discriminator: the self-referential accumulator legitimately retains every
    // prior, proving the gauge detects per-iteration region growth.
    let live_chain_growth = region_growth(
        RegionOwnership::Off,
        "(def @acc nil)",
        "(assign acc (%pair n acc))",
    );
    assert!(
        live_chain_growth > 150,
        "precondition: the live accumulator retains every prior, so region growth \
         over 200 iterations must be large (~200) — got {live_chain_growth}; if \
         small, the gauge is dead and the assertions below are vacuous",
    );

    let rotating = region_growth(RegionOwnership::Off, prelude, "(f 3)");
    assert!(
        rotating < 50,
        "an in-lambda mutual letrec cycle must be reclaimed per call by the \
         closure-cycle merge + the tail-call adopt — region growth over 200 calls \
         must be near zero, got {rotating} (each call's merged arena leaks if the \
         cycle is refused or the stranded binding-scope drop is never supplied)",
    );
    let base_case = region_growth(RegionOwnership::Off, prelude, "(f 0)");
    assert!(
        base_case < 50,
        "the base-case-only path (`(f 0)` — no sibling rotation) must also reclaim: \
         the ENTRY tail call's adopt is the sole release channel there — region \
         growth over 200 calls must be near zero, got {base_case}",
    );
    // The merge is flag-independent: bounded under --region-ownership too.
    let rotating_on = region_growth(RegionOwnership::On, prelude, "(f 3)");
    assert!(
        rotating_on < 50,
        "reclamation is flag-independent — region growth over 200 calls under \
         --region-ownership must be near zero, got {rotating_on}",
    );
}

/// Per-call reclamation of a cell-free self-recursive `letrec` closure
/// (docs/impl/selfrec.md), isolated WITHOUT the stdlib so nothing else churns the
/// region count. The subject is the same shape as
/// `closure_cycle_nested_letrec_reclaims_per_call` but boolean-only, so it runs on
/// `Runtime::without_stdlib()` (no integer trait dispatch): `loop` is a self-recursive
/// in-lambda binding — cell-free (its self-edge does not mark it captured, so it has no
/// forward cell; the self-reference resolves to the executing closure) — whose `(loop k)`
/// letrec body is a tail call.
///
/// The closure is an ordinary per-call region whose scope-end `DecrefRegion` the
/// frame-replacing `(loop k)` `TailCall` strands as dead code, so without the adopt every
/// `(f false)` would leak one region. The program samples `arena/region-count` across 10
/// discarded `loop` closures; with the tail-scoped adopt (`tail_callee_adopts` /
/// `stranded_self_bindings`) the region is freed once at the recursion's normal completion,
/// so the delta stays bounded — RC-identical to a top-level recursive `defn`.
#[test]
fn self_recursive_loop_reclaims_per_call_no_stdlib() {
    use crate::pipeline::compile_file_repl;
    // ONE compile so `f` and `loop` resolve in the same arena (a fresh REPL compile
    // renumbers global slots — a separate `(f false)` compile would mis-resolve `f`).
    let src = "(def f (fn [k] (letrec [loop (fn [m] (if m :done (loop true)))] (loop k)))) \
        (f false) \
        (def a (arena/region-count)) \
        (f false) (f false) (f false) (f false) (f false) \
        (f false) (f false) (f false) (f false) (f false) \
        (def b (arena/region-count)) \
        (%sub b a)";
    let mut rt = Runtime::without_stdlib();
    let res = {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file_repl(src, symbols, cctx, "<embed>")
            .expect("compiles")
            .0
    };
    let (vm, symbols, cctx) = rt.parts();
    let delta = vm
        .execute_scheduled(&res.bytecode, symbols, cctx)
        .expect("runs")
        .as_int()
        .expect("program returns the region-count delta as an int");
    assert!(
        delta <= 2,
        "a cell-free self-recursive loop closure must be reclaimed per call: live region \
         growth over 10 discarded closures must be ~0, got {delta} — its per-call region \
         leaks if the tail-call adopt does not supply the tail-call-stranded scope-end \
         DecrefRegion",
    );
}

/// A tail-position `(or param …)` (or `(and …)`) that SHORT-CIRCUIT-returns an owned heap
/// param must hand the caller an owning reference to it, exactly like `(if param param …)`.
///
/// Under the prediction-free return model (`src/hir/return_incref.rs`), every returned value
/// is wrapped in `Return`, which mints an `IncrefValueRegion` (the caller's owning reference)
/// and lets the region analysis extend the value's region `decref_point` past that mint. The
/// return-wrapping pass treats `or`/`and` as tail-transparent and pushes `Return` only into
/// the LAST operand — so a SHORT-CIRCUIT value (a non-last operand returned because it was
/// truthy/falsy) gets neither the mint nor the decref-point extension. Its owned-param decref
/// then fires before the value is returned, freeing it out from under the caller: a
/// `tag/object mismatch — list` use-after-free (witnessed standalone by `lib/http.lisp`'s
/// `merge-query`, whose `(or url-query encoded)` passthrough-returns its first arg).
///
/// `mq` here is NOT self-recursive, so this is independent of the self-recursion machinery:
/// it pins the `or`/`and` short-circuit-return mint. Counterfactual: panics before the fix
/// that wraps the whole tail `or`/`and` in `Return`; returns "hi" cleanly after.
#[test]
fn tail_or_short_circuit_returns_owned_param_no_uaf() {
    use crate::pipeline::compile_file_repl;
    let src = "(def m ((fn [] \
        (defn mq [url] (or url \"z\")) \
        {:run (fn [] (mq \"hi\"))}))) \
        (m:run)";
    let mut rt = Runtime::new();
    let res = {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file_repl(src, symbols, cctx, "<embed>")
            .expect("compiles")
            .0
    };
    let (vm, symbols, cctx) = rt.parts();
    let v = vm
        .execute_scheduled(&res.bytecode, symbols, cctx)
        .expect("a tail `(or param …)` returning an owned heap param must not double-free it");
    assert!(
        v.is_string(),
        "the passthrough `(or url \"z\")` returns the string param \"hi\", got {v:?}"
    );
}

/// A cell-free self-recursive `def` nested in a lambda, exercised through ACTUAL per-call
/// recursion with heap-allocating arithmetic (`<`/`-`, whose stdlib bodies churn regions),
/// under the FULL stdlib — the universal shape of a module-level `(defn …)` that recurses
/// (every `lib/*.lisp` helper). This is the strong companion to the boolean
/// `…_no_double_free` test below: the boolean shape never allocates inside the recursion, so
/// a prematurely-freed closure region's page is not recycled before the recursion ends — the
/// use-after-free reads stale-but-intact memory and stays silent. With heap-churning
/// arithmetic that freed page is recycled mid-recursion, so the self-call re-dispatch (which
/// re-enters the executing closure living in that region) reads a foreign object and trips
/// the `tag/object mismatch — list` panic at `arena.rs`.
///
/// A self-recursive `def`'s closure region demises at the binding's last use — the func-load
/// of the `(loop …)` recursive call — which the lowerer would emit as a LIVE `DecrefRegion`
/// right before that tail call, freeing the closure out from under its own re-entry. So
/// `lower_define` SUPPRESSES that decref (`suppressed_self_regions`) and STRANDS the binding
/// (`stranded_self_bindings`); the tail-call adopt is then the sole, once-only release,
/// reproducing the `letrec` path's accounting. The gauge (region growth over 200 calls)
/// additionally pins the region is reclaimed per call — a leak would grow it unbounded.
///
/// Counterfactual: panics with the `tag/object mismatch` UAF before the `def`-stranding +
/// premature-decref-suppression fix; runs clean and bounded after.
#[test]
fn self_recursive_define_with_arith_reclaims_per_call() {
    use crate::pipeline::compile_file_repl;

    // Run `body` 50 then 250 times in one program, sampling `arena/region-count` at each
    // point, and return the raw count delta (c250 − c50) the program computes. Mirrors
    // `closure_cycle_nested_letrec_reclaims_per_call`'s gauge — a crash inside the run
    // panics here (the RED counterfactual); a leak grows the returned delta.
    fn region_growth(prelude: &str, body: &str) -> i64 {
        let mut rt = Runtime::new();
        let src = format!(
            "{prelude} (var n 0) \
             (while (%lt n 50) {body} (assign n (%add n 1))) \
             (def c50 (arena/region-count)) \
             (while (%lt n 250) {body} (assign n (%add n 1))) \
             (def c250 (arena/region-count)) \
             (%sub c250 c50)"
        );
        let result = {
            let (_vm, symbols, cctx) = rt.parts();
            compile_file_repl(&src, symbols, cctx, "<embed>")
                .expect("compiles")
                .0
        };
        let (vm, symbols, cctx) = rt.parts();
        vm.execute_scheduled(&result.bytecode, symbols, cctx)
            .expect("runs")
            .as_int()
            .expect("program returns the region-count delta as an int")
    }

    // Subject: a self-recursive `def` (not `letrec`) nested in a lambda, recursing with
    // heap-allocating stdlib `<`/`-` so a freed `R_cell` page is recycled mid-recursion.
    let call_growth = region_growth(
        "(def f (fn [k] \
            (def loop (fn [m] (if (< m 1) :done (loop (- m 1))))) \
            (loop k)))",
        "(f 3)",
    );
    // Discriminator: the self-referential accumulator legitimately retains every prior, so
    // the gauge MUST see large growth here — else the bounded assertion below is vacuous.
    let live_chain_growth = region_growth("(def @acc nil)", "(assign acc (%pair n acc))");

    assert!(
        live_chain_growth > 150,
        "precondition: the live accumulator retains every prior, so region growth over 200 \
         iterations must be large (~200) — got {live_chain_growth}; a small value means the \
         gauge is dead and the assertion below is vacuous",
    );
    assert!(
        call_growth < 50,
        "a cell-free self-recursive `def` closure must be reclaimed per call by the \
         tail-call adopt — region growth over 200 calls must be near zero, got {call_growth} \
         (its per-call closure region leaks, or worse, is freed before the `(loop k)` tail \
         call re-enters it)",
    );
}

/// A self-recursive `def` nested in a lambda is cell-free (docs/impl/selfrec.md), handled
/// exactly like a self-recursive `letrec`: no forward cell, the self-reference resolves to
/// the executing closure. `lower_define` STRANDS the binding (`stranded_self_bindings`) and
/// SUPPRESSES its closure region's would-be-live `DecrefRegion` (`suppressed_self_regions`)
/// so the tail-call adopt is the sole release — the closure region must be freed EXACTLY
/// once. A leaked suppression (both the live decref AND the adopt firing) is a double-free.
/// This pins that the program runs to completion (the double-free was a `DecrefRegion(...) —
/// phantom region or double-free` panic in `regionstore/refcount.rs`).
#[test]
fn self_recursive_define_in_lambda_no_double_free() {
    use crate::pipeline::compile_file_repl;
    let src = "(def outer (fn [k] \
        (def loop (fn [m] (if m :done (loop true)))) \
        (loop k))) \
        (outer false)";
    let mut rt = Runtime::without_stdlib();
    let res = {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file_repl(src, symbols, cctx, "<embed>")
            .expect("compiles")
            .0
    };
    let (vm, symbols, cctx) = rt.parts();
    let v = vm
        .execute_scheduled(&res.bytecode, symbols, cctx)
        .expect("a cell-free self-recursive `def` must not double-free its closure region");
    assert!(
        v.is_keyword(),
        "the recursive `def` returns the :done keyword, got {v:?}"
    );
}

/// VM≡JIT parity for the landed ownership ops (the `AdoptRegion`/`FreeRegionGroup`
/// emit modes): the SAME never-mergeable shapes the VM tests above pin, but the
/// reclaiming function runs through the JIT — so the `elle_jit_adopt_region` /
/// `elle_jit_free_region_group` helpers and their translate arms
/// (`src/jit/translate/instr/predicates.rs`) carry the ops, mirroring the
/// interpreter handlers.
///
/// `body` is wrapped in an immediately-invoked lambda `((fn [] body))` whose body
/// carries the never-mergeable owned shape under the flag (the same shapes the VM
/// tests above pin). A single compile is re-run: the first run submits the lambda
/// for background JIT compilation (eager → hot on its first call), and
/// `drain_jit_pending` blocks until that compile finishes, so the steady-state
/// measurement dispatches the lambda through cached native code, not the
/// interpreter. An inline lambda (not a `def`-bound `f`) is used deliberately —
/// a fresh REPL compile renumbers global slots, so a separate `(f)` compile would
/// mis-resolve a `def`-bound `f` (see `self_recursive_loop_reclaims_per_call_no_stdlib`) — and
/// re-running the same program keeps the closure-template bytecode pointer stable,
/// so hotness accumulates onto the cached compile.
///
/// Returns `(per_run_region_delta, jit_compiled)`. `jit_compiled` guards against a
/// vacuous reading: before the translate arms land, the lambda's `AdoptRegion`/
/// `FreeRegionGroup` hits `unreachable!` in the background worker, which dies
/// before it can cache anything, so `jit_cache` stays empty and `jit_compiled` is
/// false even though the interpreter fallback still reclaims.
#[cfg(feature = "jit")]
fn jit_region_growth(ownership: RegionOwnership, body: &str) -> (i64, bool) {
    use crate::config::JitPolicy;
    use crate::pipeline::compile_file_repl;
    let _g = ScopedRegionOwnership::new(ownership);
    let mut rt = Runtime::without_stdlib();
    rt.vm().runtime_config.jit = JitPolicy::Eager;

    let src = format!("((fn [] {body}))");
    let prog = {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file_repl(&src, symbols, cctx, "<embed>")
            .expect("compiles")
            .0
    };
    // First run builds the lambda and calls it, submitting it for background JIT
    // compilation; drain blocks until that finishes (or the worker dies).
    {
        let (vm, symbols, cctx) = rt.parts();
        let v = vm
            .execute_scheduled(&prog.bytecode, symbols, cctx)
            .expect("runs (submits the JIT task)");
        assert!(v.is_nil(), "the discarded-shape lambda returns nil");
    }
    rt.vm().drain_jit_pending();
    let jit_compiled = !rt.vm().jit_cache.is_empty();

    // Warmup (the lambda body now dispatches to cached native code), then measure.
    {
        let (vm, symbols, cctx) = rt.parts();
        let v = vm
            .execute_scheduled(&prog.bytecode, symbols, cctx)
            .expect("runs");
        assert!(v.is_nil());
    }
    let baseline = rt.heap().active_region_count() as i64;
    for _ in 0..50 {
        let (vm, symbols, cctx) = rt.parts();
        let v = vm
            .execute_scheduled(&prog.bytecode, symbols, cctx)
            .expect("runs");
        assert!(v.is_nil());
    }
    let delta = rt.heap().active_region_count() as i64 - baseline;
    (delta, jit_compiled)
}

/// VM≡JIT parity for the **flat store-adopt**. A discarded mutable container
/// `(@array)` with an immutable value `(array 1 2)` pushed into it — both
/// call-result regions no slot can name, so MERGE cannot collapse them and the cut
/// emits `AdoptRegion(container, value)`. Run through the JIT, it must reclaim the
/// container+value subtree each call (bounded growth) and be panic-clean: a broken
/// `elle_jit_adopt_region` or subtree drop would free the value early/twice,
/// tripping a debug generation/decref assert. SOUNDNESS guard (the immutable value
/// is RC-reclaimable, so there is no flag-off leak counterfactual here).
#[cfg(feature = "jit")]
#[test]
fn region_ownership_adopt_subtree_drop_under_jit() {
    let body = "(begin (%array-push (@array) (array 1 2)) nil)";
    let (on, jit_compiled) = jit_region_growth(RegionOwnership::On, body);
    assert!(
        jit_compiled,
        "f must JIT-compile under the flag for this to test the JIT adopt path \
         (empty jit_cache means the background worker died — e.g. on a missing \
         AdoptRegion translate arm)",
    );
    assert!(
        on <= 0,
        "the Owned container+value subtree must be reclaimed by the JIT subtree \
         drop each call — per-run live-region growth {on} must be <= 0",
    );
}

/// VM≡JIT parity for the **interior-cycle adopt**. A container `root` directly
/// holds `a` and `b`, which reference each other (`a ⊇ b`, `b ⊇ a`). Per-region RC
/// cannot collect the a↔b cycle (region-rules.md Rule 8), so flag-OFF it leaks
/// under the JIT exactly as on the VM; under the flag the cut adopts a and b by
/// root, whose JIT subtree drop reclaims the cycle. The bounded-vs-leaking
/// counterfactual proves the cut (not the shape) reclaims it, AND — because before
/// the translate arms land `f` cannot JIT-compile — `jit_compiled` proves the JIT
/// path actually ran.
#[cfg(feature = "jit")]
#[test]
fn region_ownership_reclaims_interior_cycle_subtree_under_jit() {
    let body = "(let [root (@array) a (@array) b (@array)] \
                (begin (%array-push a b) (%array-push b a) \
                       (%array-push root a) (%array-push root b) nil))";
    let (on, jit_compiled) = jit_region_growth(RegionOwnership::On, body);
    assert!(
        jit_compiled,
        "f must JIT-compile under the flag — an empty jit_cache means the \
         AdoptRegion translate arm is missing (the worker hit `unreachable!`)",
    );
    let (off, _) = jit_region_growth(RegionOwnership::Off, body);
    assert!(
        off > 0,
        "precondition: the interior a↔b cycle must leak flag-off under the JIT \
         (per-run region growth {off}); if 0 the shape no longer forms an \
         uncollectable cycle",
    );
    assert!(
        on <= 0,
        "under --region-ownership the interior cycle must be reclaimed by the JIT \
         subtree drop — per-run live-region growth {on} must be <= 0 (flag-off \
         leaks {off} per run)",
    );
}

/// VM≡JIT parity for the **co-owned bare-cycle group**. Two `@array`s pushing each
/// other (`a ⊇ b`, `b ⊇ a`) with NO container parent — no owner among the members,
/// reclaimed by one `FreeRegionGroup`. Per-region RC cannot collect it, so flag-OFF
/// it leaks under the JIT; under the flag the JIT `elle_jit_free_region_group`
/// helper frees the cycle wholesale. Counterfactual + `jit_compiled` guard as
/// above.
#[cfg(feature = "jit")]
#[test]
fn region_ownership_reclaims_bare_cycle_group_under_jit() {
    let body = "(let [a (@array) b (@array)] \
                (begin (%array-push a b) (%array-push b a) nil))";
    let (on, jit_compiled) = jit_region_growth(RegionOwnership::On, body);
    assert!(
        jit_compiled,
        "f must JIT-compile under the flag — an empty jit_cache means the \
         FreeRegionGroup translate arm is missing (the worker hit `unreachable!`)",
    );
    let (off, _) = jit_region_growth(RegionOwnership::Off, body);
    assert!(
        off > 0,
        "precondition: the bare a↔b cycle must leak flag-off under the JIT \
         (per-run region growth {off}); if 0 the shape no longer forms an \
         uncollectable cycle",
    );
    assert!(
        on <= 0,
        "under --region-ownership the bare cycle must be reclaimed by the JIT \
         co-owned group free — per-run live-region growth {on} must be <= 0 \
         (flag-off leaks {off} per run)",
    );
}

/// The per-call cost of a self-recursive local closure: it is **cell-free**. A
/// binding referenced only by its own initializer lambda is captured solely by a
/// self-edge, which does not mark it captured (`hir/arena.rs` `mark_captured`), so it
/// has `needs_capture() == false` — no forward cell. Its self-reference resolves to
/// the executing closure (`LoadSelf` / a self-call), never a cell load. So a RETAINED
/// self-recursive `loop` pins exactly TWO region objects per call — the closure and
/// its one-entry env — the SAME as a foreign-capturing closure of equal capture
/// arity (one captured upvalue, not itself, likewise cell-free). Their object-count
/// gap is therefore ~0: no per-call forward cell distinguishes them.
///
/// This gauges that cell-free baseline so any regression that reintroduces a per-call
/// cell for pure self-recursion is a visible, loud failure. Made observable by
/// RETAINING every closure in a program-lifetime `@keep` — each pinned closure keeps
/// its region alive — then reading object-count growth (`arena/count`) across 200
/// retained builds, sampled mid-run by the program exactly as
/// `reassign_toplevel_prior_release_is_bounded` samples its gauge. The returned
/// closure escapes via return, so it is the caller that holds it.
///
/// Object count, not region count, is the gauge (the closure and its env share one
/// region in both shapes, so region growth is identical — asserted below). A
/// fresh-pair retain is the live-growth discriminator: it must grow ~1 object/call,
/// proving `arena/count` tracks per-call allocation (else every reading is void).
#[test]
fn self_recursive_loop_is_cell_free() {
    use crate::pipeline::compile_file_repl;

    // Object growth (`gauge`) over 200 closures built by `build` and RETAINED in a
    // program-lifetime `@keep`, sampled mid-run after 50 then 250 builds; returns
    // c250 - c50 (the per-200-call delta the program computes).
    fn retained_growth(prelude: &str, build: &str, gauge: &str) -> i64 {
        let mut rt = Runtime::without_stdlib();
        let src = format!(
            "{prelude} (var n 0) \
             (while (%lt n 50) (%array-push keep {build}) (assign n (%add n 1))) \
             (def c50 ({gauge})) \
             (while (%lt n 250) (%array-push keep {build}) (assign n (%add n 1))) \
             (def c250 ({gauge})) \
             (%sub c250 c50)"
        );
        let result = {
            let (_vm, symbols, cctx) = rt.parts();
            compile_file_repl(&src, symbols, cctx, "<embed>")
                .expect("compiles")
                .0
        };
        let (vm, symbols, cctx) = rt.parts();
        vm.execute_scheduled(&result.bytecode, symbols, cctx)
            .expect("runs")
            .as_int()
            .expect("program returns the gauge delta as an int")
    }

    // Subject: a self-recursive in-lambda `loop`. Its initializer references only
    // itself, a self-edge that does not mark it captured, so `loop` is cell-free —
    // its self-reference resolves to the executing closure. `(frec false)` recurses
    // to the base case and RETURNS the `loop` closure (escaping), which `@keep` pins.
    let rec_prelude = "(def @keep @[]) \
        (def frec (fn [k] (letrec [loop (fn [m] (if m loop (loop true)))] (loop k))))";
    // Cell-free analog of equal capture arity: `h` captures one upvalue (the
    // immediate `k`), not itself — likewise a closure + one-entry env, no cell. With
    // self-recursion also cell-free, the only structural difference is gone.
    let for_prelude = "(def @keep @[]) \
        (def ffor (fn [k] (let [h (fn [m] (if m k k))] h)))";

    let rec_obj = retained_growth(rec_prelude, "(frec false)", "arena/count");
    let for_obj = retained_growth(for_prelude, "(ffor false)", "arena/count");
    let pair_obj = retained_growth("(def @keep @[])", "(%pair 1 2)", "arena/count");
    let rec_reg = retained_growth(rec_prelude, "(frec false)", "arena/region-count");
    let for_reg = retained_growth(for_prelude, "(ffor false)", "arena/region-count");

    // Gauge-live discriminator: retaining 200 fresh pairs must grow the object
    // count ~200 (one per call). If small, `arena/count` is not tracking per-call
    // allocation and every assertion below is vacuous.
    assert!(
        pair_obj > 150,
        "gauge-live: retaining 200 fresh pairs must grow the object count ~200, \
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

    // The absolute baseline: a retained self-recursive closure pins ~2 objects per
    // call (closure + one-entry env ~= 400/200), the same as the foreign-capture
    // control — no forward cell.
    assert!(
        (300..=500).contains(&rec_obj),
        "cell-free baseline: 200 retained self-recursive `loop` closures pin ~400 \
         objects (2/call: closure + env, no forward cell), got {rec_obj}",
    );

    // Region count grows identically in both shapes (closure + env share one region),
    // so object count is the necessary gauge for the per-call cell.
    assert!(
        (rec_reg - for_reg).abs() < 50,
        "region growth must match between the self-recursive and foreign-capture \
         shapes (closure + env share one region): self-recursive {rec_reg} vs \
         foreign-capture {for_reg}",
    );
}

/// End-to-end exercise of the ACTIVATION OWNER NODE on the interpreter
/// (docs/impl/region-model.md § "Owner nodes — an activation as a forest root").
/// No production lowering emits `AdoptIntoActivation` yet, so the activation body
/// is hand-emitted bytecode: load a fresh-region member from the constant pool,
/// adopt it into the current activation's (lazily-minted) owner node, return nil.
/// The activation's NORMAL completion — the trampoline's clean break — must free
/// the node, whose subtree drop reclaims the member: its generation bumps (pages
/// returned) and the live region count stays bounded across 50 activations. The
/// counterfactual is the adopt itself: the member is Owned (its count consumed),
/// so if the completion release does not fire, NOTHING reclaims it — node + member
/// entries survive every run and the count grows by 2 per activation.
#[test]
fn activation_owner_node_frees_adopted_member_on_normal_completion() {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use std::rc::Rc;

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    for _ in 0..50 {
        // The member: a pair in its own fresh region on the VM's heap.
        let (child, child_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let gen_before = unsafe { &*heap_ptr }.generation_raw(child_rid.get());

        // Body: push the member, adopt it into the activation node, return nil.
        let mut bc = Bytecode::new();
        let idx = bc.add_constant(child);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        bc.emit(Instruction::AdoptIntoActivation);
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Return);
        let code = crate::value::Code::new(
            Rc::new(bc.instructions),
            Rc::new(bc.constants),
            Rc::new(crate::error::LocationMap::new()),
            Rc::new(vec![]),
        );

        let result = vm.execute_bytecode_saving_stack(&code, &Rc::new(vec![]));
        assert!(
            result.bits.is_ok(),
            "the adopt-and-return body completes normally"
        );
        let gen_after = unsafe { &*heap_ptr }.generation_raw(child_rid.get());
        assert!(
            gen_after > gen_before,
            "the adopted member's pages must be returned (generation bumped) by \
             the owner node's subtree drop at the activation's normal completion \
             (gen {gen_before} -> {gen_after})",
        );
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "node + member must be reclaimed at each activation's completion — live \
         region count must not grow (baseline={baseline}, after 50 activations={after})",
    );
}

/// The activation owner node SURVIVES a yield→resume park
/// (docs/impl/region-model.md § "Owner nodes" — "A park moves the node into the
/// suspended frame"). The hand-emitted body (no production lowering emits
/// `AdoptIntoActivation`) adopts a fresh-region member into the activation's
/// node, yields, and — once resumed — completes normally. The park must carry
/// the node (the member stays Owned, RC frozen, while the fiber is parked — it
/// must NOT be freed mid-park), and the RESUMED body's normal completion must
/// free node + member: generation bump, bounded region count over 50
/// activations. The counterfactual is the park itself: a suspend that drops the
/// node slot strands the Owned member — nothing ever reclaims it, the
/// generation never bumps, and the count grows by 2 per activation.
#[test]
fn activation_owner_node_survives_yield_resume_completion() {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use std::rc::Rc;

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    for _ in 0..50 {
        let (child, child_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let gen_before = unsafe { &*heap_ptr }.generation_raw(child_rid.get());

        // Body: adopt the member, yield nil, then (on resume) return the
        // resume value pushed as the yield expression's result.
        let mut bc = Bytecode::new();
        let idx = bc.add_constant(child);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        bc.emit(Instruction::AdoptIntoActivation);
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Emit);
        bc.emit_u16(crate::value::fiber::SIG_YIELD.raw() as u16);
        bc.emit(Instruction::Return);
        let code = crate::value::Code::new(
            Rc::new(bc.instructions),
            Rc::new(bc.constants),
            Rc::new(crate::error::LocationMap::new()),
            Rc::new(vec![]),
        );

        let result = vm.execute_bytecode_saving_stack(&code, &Rc::new(vec![]));
        assert!(
            result.bits.contains(crate::value::fiber::SIG_YIELD),
            "the body parks at the yield"
        );
        assert_eq!(
            unsafe { &*heap_ptr }.generation_raw(child_rid.get()),
            gen_before,
            "the adopted member must stay live while the activation is parked \
             (an Owned member is freed only by the node's completion release)",
        );

        let frames = vm.fiber.suspended.take().expect("the yield parked a frame");
        let bits = vm.resume_suspended(frames, crate::value::Value::NIL);
        assert!(bits.is_ok(), "the resumed body completes normally");
        let gen_after = unsafe { &*heap_ptr }.generation_raw(child_rid.get());
        assert!(
            gen_after > gen_before,
            "the adopted member's pages must be returned (generation bumped) by \
             the owner node's subtree drop at the RESUMED activation's normal \
             completion — the node must survive the park \
             (gen {gen_before} -> {gen_after})",
        );
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "node + member must be reclaimed at each parked-and-resumed activation's \
         completion — live region count must not grow (baseline={baseline}, after \
         50 activations={after})",
    );
}

/// The node survives REPEATED parks: yield → resume → yield again → resume →
/// complete. The first park carries the node out of the unwinding activation;
/// the resume restores it into the live slot; the second park (during the
/// RESUMED execution) re-captures it; the final completion frees node + member
/// exactly once. Both halves are load-bearing: dropping the restore or the
/// re-capture strands the Owned member (its generation never bumps), and a
/// clone anywhere instead of a move would free it twice (the debug regionstore
/// asserts detonate mid-loop).
#[test]
fn activation_owner_node_survives_repeated_parks() {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use std::rc::Rc;

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    for _ in 0..50 {
        let (child, child_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let gen_before = unsafe { &*heap_ptr }.generation_raw(child_rid.get());

        // Body: adopt, yield, (resume) discard the resume value, yield again,
        // (resume) return the second resume value.
        let mut bc = Bytecode::new();
        let idx = bc.add_constant(child);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        bc.emit(Instruction::AdoptIntoActivation);
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Emit);
        bc.emit_u16(crate::value::fiber::SIG_YIELD.raw() as u16);
        bc.emit(Instruction::Pop);
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Emit);
        bc.emit_u16(crate::value::fiber::SIG_YIELD.raw() as u16);
        bc.emit(Instruction::Return);
        let code = crate::value::Code::new(
            Rc::new(bc.instructions),
            Rc::new(bc.constants),
            Rc::new(crate::error::LocationMap::new()),
            Rc::new(vec![]),
        );

        let result = vm.execute_bytecode_saving_stack(&code, &Rc::new(vec![]));
        assert!(
            result.bits.contains(crate::value::fiber::SIG_YIELD),
            "the body parks at the first yield"
        );

        let frames = vm.fiber.suspended.take().expect("first park");
        let bits = vm.resume_suspended(frames, crate::value::Value::NIL);
        assert!(
            bits.contains(crate::value::fiber::SIG_YIELD),
            "the resumed body parks again at the second yield"
        );
        assert_eq!(
            unsafe { &*heap_ptr }.generation_raw(child_rid.get()),
            gen_before,
            "the adopted member must stay live across BOTH parks",
        );

        let frames = vm.fiber.suspended.take().expect("second park");
        let bits = vm.resume_suspended(frames, crate::value::Value::NIL);
        assert!(bits.is_ok(), "the twice-resumed body completes normally");
        let gen_after = unsafe { &*heap_ptr }.generation_raw(child_rid.get());
        assert!(
            gen_after > gen_before,
            "the adopted member must be freed at the twice-parked activation's \
             completion — the node must ride park, restore, and re-park \
             (gen {gen_before} -> {gen_after})",
        );
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "node + member must be reclaimed after the repeated parks — live region \
         count must not grow (baseline={baseline}, after 50 activations={after})",
    );
}

/// The node rides `ExecResult::activation_owner_node` when the park is built by
/// the CALLER of the already-unwound activation — the fuel-pause channel
/// (docs/impl/region-model.md § "Owner nodes"). A fuel pause (unlike a yield)
/// creates no suspended frame inside the dispatch loop: the activation unwinds
/// through `execute_bytecode_saving_stack`, which must move the node into the
/// `ExecResult` beside the region map, and the caller builds the park from that
/// result — exactly what `do_fiber_first_resume` does for a fiber body's pause,
/// mirrored here directly. The body adopts a member, then hits a backward jump
/// (the fuel check site) with zero fuel; refueled and resumed, it completes and
/// the node's release frees the member. A `saving_stack` that dropped the node
/// instead of capturing it strands the Owned member: the generation never bumps.
#[test]
fn activation_owner_node_rides_exec_result_across_fuel_pause() {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use crate::value::{BytecodeFrame, SuspendedFrame};
    use std::rc::Rc;

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    for _ in 0..50 {
        let (child, child_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let gen_before = unsafe { &*heap_ptr }.generation_raw(child_rid.get());

        // Body: adopt the member, then jump forward over the landing pad to a
        // BACKWARD jump — the fuel check site — that jumps back to the pad
        // (Nil, Return). With zero fuel the backward jump pauses; refueled, it
        // completes.
        //
        //   0: LoadConst idx          3: AdoptIntoActivation
        //   4: Jump +2  (→ 11)        9: Nil    10: Return
        //  11: Jump -7  (→ 9, backward: fuel-checked)
        let mut bc = Bytecode::new();
        let idx = bc.add_constant(child);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        bc.emit(Instruction::AdoptIntoActivation);
        bc.emit(Instruction::Jump);
        bc.emit_i32(2);
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Return);
        bc.emit(Instruction::Jump);
        bc.emit_i32(-7);
        let code = crate::value::Code::new(
            Rc::new(bc.instructions),
            Rc::new(bc.constants),
            Rc::new(crate::error::LocationMap::new()),
            Rc::new(vec![]),
        );

        vm.fiber.fuel = Some(0);
        let result = vm.execute_bytecode_saving_stack(&code, &Rc::new(vec![]));
        assert!(
            result.bits.contains(crate::value::SIG_FUEL),
            "the body pauses at the backward jump with zero fuel"
        );
        assert!(
            vm.fiber.suspended.is_none(),
            "a fuel pause parks no frame of its own — the caller builds it"
        );
        assert_eq!(
            unsafe { &*heap_ptr }.generation_raw(child_rid.get()),
            gen_before,
            "the adopted member must stay live across the fuel pause",
        );

        // Build the park from the returned context, exactly as
        // `do_fiber_first_resume` does for a paused fiber body.
        let frame = BytecodeFrame::suspend(
            result.code,
            result.env,
            result.ip,
            result.stack,
            !result.bits.contains(crate::value::SIG_FUEL),
            result.activation_region_map,
            result.activation_owner_node,
            result.current_closure,
            vm.heap(),
        );
        vm.fiber.fuel = None;
        let bits = vm.resume_suspended(
            vec![SuspendedFrame::Bytecode(frame)],
            crate::value::Value::NIL,
        );
        assert!(bits.is_ok(), "the refueled body completes normally");
        let gen_after = unsafe { &*heap_ptr }.generation_raw(child_rid.get());
        assert!(
            gen_after > gen_before,
            "the adopted member must be freed at the resumed activation's \
             completion — the node must ride the ExecResult out of the paused \
             activation (gen {gen_before} -> {gen_after})",
        );
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "node + member must be reclaimed after the fuel-pause round trip — live \
         region count must not grow (baseline={baseline}, after 50 activations={after})",
    );
}

/// A closure over hand-emitted bytecode, for driving a fiber body no production
/// lowering can build yet (`AdoptIntoActivation` / the fiber owner node have no
/// emitters). The zero-arity template wraps the bytecode + constants exactly as
/// a compiled thunk would.
fn fiber_body_closure(
    bc: crate::compiler::bytecode::Bytecode,
) -> std::rc::Rc<crate::value::Closure> {
    use std::rc::Rc;
    Rc::new(crate::value::Closure {
        template: crate::value::TemplateRef::new(Rc::new(crate::value::ClosureTemplate::new(
            Rc::new(bc.instructions),
            crate::value::Arity::Exact(0),
            Rc::new(bc.constants),
        ))),
        env: crate::value::region_slice::RegionSlice::empty(),
        squelch_mask: crate::value::SignalBits::EMPTY,
    })
}

/// A child fiber over `closure`, plus its heap value (built through an `Alloc`
/// ctx into a region of its own, which the caller releases per cycle).
fn child_fiber(
    heap: &mut crate::value::fiberheap::FiberHeap,
    closure: std::rc::Rc<crate::value::Closure>,
) -> (crate::value::FiberHandle, crate::value::Value) {
    let handle = crate::value::FiberHandle::new(crate::value::Fiber::new(
        closure,
        crate::value::SignalBits::EMPTY,
    ));
    let ctx = crate::primitives::ctx::Alloc::new(heap);
    let fiber_value = ctx.fiber_from_handle(handle.clone());
    (handle, fiber_value)
}

/// Release the per-cycle fiber VALUE's region (the `Alloc` ctx minted it), so
/// the bounded-count loops measure only what the teardown under test leaves.
fn release_fiber_value(
    heap: &mut crate::value::fiberheap::FiberHeap,
    fiber_value: crate::value::Value,
) {
    if let Some(r) = crate::value::arena::region_of(heap, fiber_value) {
        heap.decref_region_if_present(r);
    }
}

/// The FIBER owner node is freed at the fiber's normal completion
/// (docs/impl/region-model.md § "Owner nodes" — "Fiber teardown frees everything
/// the fiber owns"). No production lowering targets the fiber node yet, so the
/// test stands in for the cross-fiber ownership cuts: it mints the node, adopts
/// a fresh-region member into it, and runs the fiber to completion
/// (`do_fiber_resume`). The `:dead` transition must free node + member — the
/// member's generation bumps — and the live region count stays bounded across
/// 50 fibers. The counterfactual is the adopt itself: the member is Owned (its
/// count consumed), so if the completion teardown does not fire, NOTHING
/// reclaims it and the count grows every cycle.
#[test]
fn fiber_owner_node_freed_at_fiber_completion() {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use crate::value::fiber::FiberStatus;

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    for _ in 0..50 {
        // The body: a noop thunk that completes immediately.
        let mut bc = Bytecode::new();
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Return);
        let (handle, fiber_value) = child_fiber(unsafe { &mut *heap_ptr }, fiber_body_closure(bc));

        // The fiber's owned state: a pages-less node with one adopted member.
        let (_member, member_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let node = unsafe { &mut *heap_ptr }.new_runtime_region();
        unsafe { &mut *heap_ptr }.adopt_region(node, member_rid);
        handle.with_mut(|f| f.fiber_owner_node = Some(node));
        let gen_before = unsafe { &*heap_ptr }.generation_raw(member_rid.get());

        let (bits, _v) = vm.do_fiber_resume(&handle, fiber_value);
        assert!(bits.is_ok(), "the noop fiber body completes");
        assert_eq!(handle.with(|f| f.status), FiberStatus::Dead);
        let gen_after = unsafe { &*heap_ptr }.generation_raw(member_rid.get());
        assert!(
            gen_after > gen_before,
            "the fiber-node member's pages must be returned (generation bumped) \
             by the fiber's completion teardown (gen {gen_before} -> {gen_after})",
        );

        release_fiber_value(unsafe { &mut *heap_ptr }, fiber_value);
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "fiber node + member must be reclaimed at each fiber's completion — live \
         region count must not grow (baseline={baseline}, after 50 fibers={after})",
    );
}

/// The fiber owner node SURVIVES parks — it is fiber state, riding suspension
/// structurally — and is freed at the resumed fiber's completion, alongside a
/// MULTI-FRAME parked chain whose per-frame activation nodes each reclaim at
/// their own frame's completion (docs/impl/region-model.md § "Owner nodes").
/// The body adopts a member into its ACTIVATION node and yields (frame 1); a
/// second hand-built frame carrying its own node + member is appended (the
/// outer-caller shape of a yield-through chain); the fiber node holds a third
/// member. Across the park all three stay live (Owned, RC frozen — no other
/// release route); the resume replays both frames to completion, freeing each
/// frame's node at that frame's completion and the FIBER node at `:dead`.
/// The counterfactual is the fiber-node half: without the completion teardown
/// its member's generation never bumps and the count grows per cycle.
#[test]
fn fiber_owner_node_survives_parks_and_frees_at_completion() {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use crate::value::fiber::FiberStatus;
    use crate::value::{BytecodeFrame, SuspendedFrame};
    use std::rc::Rc;

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    for _ in 0..50 {
        let heap = unsafe { &mut *heap_ptr };

        // Frame-1 body: adopt member_a into the activation node, yield, return
        // the resume value.
        let (member_a, rid_a) = alloc_in_fresh_region(heap, cons());
        let mut bc = Bytecode::new();
        let idx = bc.add_constant(member_a);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        bc.emit(Instruction::AdoptIntoActivation);
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Emit);
        bc.emit_u16(crate::value::fiber::SIG_YIELD.raw() as u16);
        bc.emit(Instruction::Return);
        let (handle, fiber_value) = child_fiber(heap, fiber_body_closure(bc));

        // The fiber's own owned state.
        let (_mf, rid_f) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let node_f = unsafe { &mut *heap_ptr }.new_runtime_region();
        unsafe { &mut *heap_ptr }.adopt_region(node_f, rid_f);
        handle.with_mut(|f| f.fiber_owner_node = Some(node_f));

        let gen_a = unsafe { &*heap_ptr }.generation_raw(rid_a.get());
        let gen_f = unsafe { &*heap_ptr }.generation_raw(rid_f.get());

        let (bits, _v) = vm.do_fiber_resume(&handle, fiber_value);
        assert!(
            bits.contains(crate::value::fiber::SIG_YIELD),
            "the body parks at the yield"
        );
        assert_eq!(handle.with(|f| f.status), FiberStatus::Paused);
        assert_eq!(
            unsafe { &*heap_ptr }.generation_raw(rid_a.get()),
            gen_a,
            "the parked frame's adopted member stays live across the park",
        );
        assert_eq!(
            unsafe { &*heap_ptr }.generation_raw(rid_f.get()),
            gen_f,
            "the fiber-node member stays live across the park",
        );

        // Frame 2: a hand-built outer activation parked with its own node +
        // member — the multi-frame chain of a yield through a call.
        let (_mb, rid_b) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let node_b = unsafe { &mut *heap_ptr }.new_runtime_region();
        unsafe { &mut *heap_ptr }.adopt_region(node_b, rid_b);
        let gen_b = unsafe { &*heap_ptr }.generation_raw(rid_b.get());
        let mut bc2 = Bytecode::new();
        bc2.emit(Instruction::Return);
        let code2 = crate::value::Code::new(
            Rc::new(bc2.instructions),
            Rc::new(bc2.constants),
            Rc::new(crate::error::LocationMap::new()),
            Rc::new(vec![]),
        );
        let frame2 = BytecodeFrame::suspend(
            code2,
            Rc::new(vec![]),
            0,
            vec![],
            true,
            rustc_hash::FxHashMap::default(),
            Some(node_b),
            crate::value::Value::NIL,
            unsafe { &*heap_ptr },
        );
        handle.with_mut(|f| {
            f.suspended
                .as_mut()
                .expect("the yield parked a chain")
                .push(SuspendedFrame::Bytecode(frame2));
        });

        let (bits, _v) = vm.do_fiber_resume(&handle, fiber_value);
        assert!(bits.is_ok(), "the resumed two-frame chain completes");
        assert_eq!(handle.with(|f| f.status), FiberStatus::Dead);
        let bumped_a = unsafe { &*heap_ptr }.generation_raw(rid_a.get()) > gen_a;
        let bumped_b = unsafe { &*heap_ptr }.generation_raw(rid_b.get()) > gen_b;
        let bumped_f = unsafe { &*heap_ptr }.generation_raw(rid_f.get()) > gen_f;
        assert!(
            bumped_a && bumped_b,
            "each parked frame's activation node frees at that frame's completion \
             (frame 1 freed: {bumped_a}, frame 2 freed: {bumped_b})",
        );
        assert!(
            bumped_f,
            "the fiber node must ride the parks and free at the fiber's completion",
        );

        release_fiber_value(unsafe { &mut *heap_ptr }, fiber_value);
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "every node + member must be reclaimed by the parked-and-resumed fiber's \
         completion — live region count must not grow (baseline={baseline}, after \
         50 cycles={after})",
    );
}

/// A hard kill frees everything the fiber owns: `fiber/cancel` of a PARKED
/// fiber releases both the parked frame's activation owner node and the fiber
/// owner node (gathered under it by `reparent_owned_children` — one set-drop),
/// and `fiber/abort` of a not-yet-started fiber releases the fiber node
/// (docs/impl/region-model.md § "Owner nodes" — "Fiber teardown frees
/// everything the fiber owns"). Both route through `kill_fiber`; before it, the
/// cancel arm dropped the chain bare (`suspended = None`), stranding every
/// parked node. The counterfactual is exactly that strand: without the
/// teardown, no generation bumps and the count grows per cycle.
#[test]
fn fiber_kill_frees_parked_and_fiber_owned() {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use crate::value::fiber::FiberStatus;

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let vm_ptr: *mut crate::vm::VM = &mut vm;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    for _ in 0..50 {
        // ── fiber/cancel of a parked fiber ──
        let heap = unsafe { &mut *heap_ptr };
        let (member_a, rid_a) = alloc_in_fresh_region(heap, cons());
        let mut bc = Bytecode::new();
        let idx = bc.add_constant(member_a);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        bc.emit(Instruction::AdoptIntoActivation);
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Emit);
        bc.emit_u16(crate::value::fiber::SIG_YIELD.raw() as u16);
        bc.emit(Instruction::Return);
        let (handle, fiber_value) = child_fiber(heap, fiber_body_closure(bc));

        let (_mf, rid_f) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let node_f = unsafe { &mut *heap_ptr }.new_runtime_region();
        unsafe { &mut *heap_ptr }.adopt_region(node_f, rid_f);
        handle.with_mut(|f| f.fiber_owner_node = Some(node_f));
        let gen_a = unsafe { &*heap_ptr }.generation_raw(rid_a.get());
        let gen_f = unsafe { &*heap_ptr }.generation_raw(rid_f.get());

        let (bits, _v) = vm.do_fiber_resume(&handle, fiber_value);
        assert!(
            bits.contains(crate::value::fiber::SIG_YIELD),
            "the body parks at the yield"
        );

        // Cancel through the primitive — the production hard-kill path.
        let ctx_region = unsafe { &mut *heap_ptr }.new_runtime_region();
        let (bits, _v) = {
            let mut ctx = crate::primitives::ctx::NativeCtx::with_region_vm(
                ctx_region,
                unsafe { &mut *heap_ptr },
                vm_ptr,
            );
            crate::primitives::fiber_introspect::prim_fiber_cancel(&mut ctx, &[fiber_value])
        };
        assert!(bits.is_ok(), "cancelling a parked fiber succeeds");
        unsafe { &mut *heap_ptr }.decref_region_if_present(ctx_region);
        assert_eq!(handle.with(|f| f.status), FiberStatus::Error);
        assert!(
            handle.with(|f| f.suspended.is_none()),
            "the cancel consumed the parked chain"
        );
        let bumped_a = unsafe { &*heap_ptr }.generation_raw(rid_a.get()) > gen_a;
        let bumped_f = unsafe { &*heap_ptr }.generation_raw(rid_f.get()) > gen_f;
        assert!(
            bumped_a && bumped_f,
            "the cancel must free the parked frame's node AND the fiber node \
             (parked member freed: {bumped_a}, fiber member freed: {bumped_f})",
        );
        release_fiber_value(unsafe { &mut *heap_ptr }, fiber_value);

        // ── fiber/abort of a not-yet-started fiber ──
        let mut bc = Bytecode::new();
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Return);
        let (handle, fiber_value) = child_fiber(unsafe { &mut *heap_ptr }, fiber_body_closure(bc));
        let (_mn, rid_n) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let node_n = unsafe { &mut *heap_ptr }.new_runtime_region();
        unsafe { &mut *heap_ptr }.adopt_region(node_n, rid_n);
        handle.with_mut(|f| f.fiber_owner_node = Some(node_n));
        let gen_n = unsafe { &*heap_ptr }.generation_raw(rid_n.get());

        let ctx_region = unsafe { &mut *heap_ptr }.new_runtime_region();
        let (bits, _v) = {
            let mut ctx = crate::primitives::ctx::NativeCtx::with_region_vm(
                ctx_region,
                unsafe { &mut *heap_ptr },
                vm_ptr,
            );
            crate::primitives::fiber_introspect::prim_fiber_abort(&mut ctx, &[fiber_value])
        };
        assert!(bits.is_ok(), "aborting a :new fiber succeeds");
        unsafe { &mut *heap_ptr }.decref_region_if_present(ctx_region);
        assert_eq!(handle.with(|f| f.status), FiberStatus::Error);
        assert!(
            unsafe { &*heap_ptr }.generation_raw(rid_n.get()) > gen_n,
            "aborting a never-started fiber must free its fiber node's members",
        );
        release_fiber_value(unsafe { &mut *heap_ptr }, fiber_value);
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "the hard kills must reclaim everything each fiber owned — live region \
         count must not grow (baseline={baseline}, after 50 cycles={after})",
    );
}

/// A squelch/abort DISCARD frees the parked owner node
/// (docs/impl/region-model.md § "Owner nodes" — "A discard frees the parked
/// node"). The hand-emitted body adopts a fresh-region member into the
/// activation's node and yields; instead of resuming, the park is abandoned
/// through the one discard chokepoint (`VM::discard_suspended_frames`, the
/// path `enforce_squelch` takes on a signal-violation). The discarded frame's
/// continuation never runs, so the completion release never fires — the
/// chokepoint must run it at the discard: node + member freed (generation
/// bump), live region count bounded across repeated park-discard cycles, and
/// a second discard is a no-op. The counterfactual is the discard itself: a
/// chokepoint that merely drops the frames strands the Owned member (no count
/// for any other release route to reach), the generation never bumps, and the
/// count grows by 2 per cycle. The multi-frame chain half pins the per-frame
/// loop: BOTH parked activations' nodes are freed, not just the first.
#[test]
fn discard_frees_parked_activation_owner_node() {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use std::rc::Rc;

    // The adopt-then-yield body every cycle parks (same shape as
    // `activation_owner_node_survives_yield_resume_completion`).
    fn adopt_yield_code(child: crate::value::Value) -> crate::value::Code {
        let mut bc = Bytecode::new();
        let idx = bc.add_constant(child);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        bc.emit(Instruction::AdoptIntoActivation);
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Emit);
        bc.emit_u16(crate::value::fiber::SIG_YIELD.raw() as u16);
        bc.emit(Instruction::Return);
        crate::value::Code::new(
            Rc::new(bc.instructions),
            Rc::new(bc.constants),
            Rc::new(crate::error::LocationMap::new()),
            Rc::new(vec![]),
        )
    }

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    // ── single-frame chain: park, then discard ──
    for _ in 0..50 {
        let (child, child_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let gen_before = unsafe { &*heap_ptr }.generation_raw(child_rid.get());
        let code = adopt_yield_code(child);

        let result = vm.execute_bytecode_saving_stack(&code, &Rc::new(vec![]));
        assert!(
            result.bits.contains(crate::value::fiber::SIG_YIELD),
            "the body parks at the yield"
        );

        vm.discard_suspended_frames();
        assert!(
            vm.fiber.suspended.is_none(),
            "the discard consumed the parked chain"
        );
        let gen_after = unsafe { &*heap_ptr }.generation_raw(child_rid.get());
        assert!(
            gen_after > gen_before,
            "the adopted member's pages must be returned (generation bumped) by \
             the discard's subtree drop of the parked owner node \
             (gen {gen_before} -> {gen_after})",
        );
        // A second discard finds nothing — the release ran exactly once.
        vm.discard_suspended_frames();
    }

    // ── multi-frame chain: two parked activations, one discard frees both ──
    for _ in 0..50 {
        let (child_a, rid_a) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let (child_b, rid_b) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let gen_a = unsafe { &*heap_ptr }.generation_raw(rid_a.get());
        let gen_b = unsafe { &*heap_ptr }.generation_raw(rid_b.get());

        let result = vm.execute_bytecode_saving_stack(&adopt_yield_code(child_a), &Rc::new(vec![]));
        assert!(result.bits.contains(crate::value::fiber::SIG_YIELD));
        let mut chain = vm.fiber.suspended.take().expect("first park");

        let result = vm.execute_bytecode_saving_stack(&adopt_yield_code(child_b), &Rc::new(vec![]));
        assert!(result.bits.contains(crate::value::fiber::SIG_YIELD));
        chain.extend(vm.fiber.suspended.take().expect("second park"));

        vm.fiber.suspended = Some(chain);
        vm.discard_suspended_frames();
        let bumped_a = unsafe { &*heap_ptr }.generation_raw(rid_a.get()) > gen_a;
        let bumped_b = unsafe { &*heap_ptr }.generation_raw(rid_b.get()) > gen_b;
        assert!(
            bumped_a && bumped_b,
            "EVERY discarded frame's node must be freed, not just the first \
             (frame a freed: {bumped_a}, frame b freed: {bumped_b})",
        );
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "node + member must be reclaimed at each discard — live region count \
         must not grow (baseline={baseline}, after 100 park-discard cycles={after})",
    );
}
