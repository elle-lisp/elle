// audited: 2026-09-09
// ── Which body tails the closure-cycle merge admits ──────────────────
//
// docs/impl/region/letrec.md
//
// A `letrec` body's tail call replaces the frame, which strands the merged
// arena's binding-scope `DecrefRegion` as dead code. Every such tail must
// therefore supply a release channel: a MEMBER callee rides the existing
// stranded-cycle deferral, a NON-member callee rides the explicit arena adopt
// (`RegionInfo::cycle_tail_release`). What the non-member channel still refuses
// is a cycle member carried into the call BY-MOVE, whose new activation's
// owned-parameter release would decref the arena a second time — and that is a
// claim about a callee which can REPLACE the frame, so a callee the compiler
// reads as a native is admitted with the members riding in.

use super::*;

#[test]
fn merge_admits_in_lambda_cycle_with_foreign_tail_callee() {
    // INVERTED from the old tail-strand refusal: a letrec body tail-calling a
    // NON-member closure `g` (a foreign fn) now MERGES. The frame-replacing
    // TailCall strands the binding-scope drop, but the non-member release channel —
    // `RegionInfo::cycle_tail_release` → `TailCall::deferred_release_slot` — is wired, so a
    // closure callee's new activation takes over the arena's release, freeing it at recursion
    // completion. The tail argument is `(ev k)`'s RESULT (a value), not a member, so
    // no member flows in by-move (contrast
    // `merge_refuses_member_passed_by_move_to_foreign_tail`). `g` is a user closure,
    // so its `(g r)` tail is an ordinary `Call`.
    let mut symbols = SymbolTable::new();
    let (hir, arena) = compile_fhir(
        "(def g (fn [x] x)) \
         (def f (fn [k] (letrec [ev (fn [m] (if (%lt m 1) :even (od (%sub m 1)))) \
                                 od (fn [m] (if (%lt m 1) :odd (ev (%sub m 1))))] \
                          (g (ev k))))) \
         (f 3)",
        &mut symbols,
    );
    let info = analyze_regions(&hir, &arena);
    let cells = ev_od_cells(&hir, &arena, &symbols, &info);
    assert_eq!(
        cells.len(),
        2,
        "precondition: two compiled forward cells; got {cells:?}"
    );
    let roots: rustc_hash::FxHashSet<Region> = cells.iter().map(|&c| info.merged_root(c)).collect();
    assert_eq!(
        roots.len(),
        1,
        "a foreign-closure body tail ((g (ev k))) must now MERGE the cycle — the \
         non-member tail release slot supplies the stranded release; cells={cells:?} \
         merged_parent={:?}",
        info.merged_parent,
    );
    let root = roots.into_iter().next().unwrap();
    assert!(
        !cells.contains(&root),
        "the merged root must be a closure region, not a cell; root=r{} cells={cells:?}",
        root.0,
    );
    // The non-member tail site is recorded, keyed to the merged root — the datum the
    // lowerer reads to set `deferred_release_slot`.
    assert!(
        info.cycle_tail_release.values().any(|&r| r == root),
        "the (g r) tail site must record cycle_tail_release → merged root r{}; got {:?}",
        root.0,
        info.cycle_tail_release,
    );
}

#[test]
fn merge_admits_native_tail() {
    // The native body tail `(%freeze (ev k))`: a copying `%`-op compiles as a native
    // funnel `Call`, so in tail position it is a frame-replacing `TailCall` (an inline
    // arith `%`-op would be an `Intrinsic` node and not a Call tail at all). The cycle
    // must MERGE and record the `%freeze` site in `cycle_tail_release`: at runtime the
    // native keeps the frame and the live scope-exit drop frees the arena, but the
    // release slot is carried anyway (the compiler never classifies the callee), so a
    // rebound `%freeze` closure is also covered. This is the native-tail shape the
    // whole class regressed on.
    let mut symbols = SymbolTable::new();
    let (hir, arena, info) = analyze_cycle_with_effects(
        "(def f (fn [k] (letrec [ev (fn [m] (if (%lt m 1) :even (od (%sub m 1)))) \
                                 od (fn [m] (if (%lt m 1) :odd (ev (%sub m 1))))] \
                          (%freeze (ev k))))) \
         (f 3)",
        &mut symbols,
    );
    let cells = ev_od_cells(&hir, &arena, &symbols, &info);
    assert_eq!(
        cells.len(),
        2,
        "precondition: two compiled forward cells; got {cells:?}"
    );
    let roots: rustc_hash::FxHashSet<Region> = cells.iter().map(|&c| info.merged_root(c)).collect();
    assert_eq!(
        roots.len(),
        1,
        "a native body tail ((%freeze (ev k))) must MERGE the cycle; \
         cells={cells:?} merged_parent={:?}",
        info.merged_parent,
    );
    let root = roots.into_iter().next().unwrap();
    assert!(
        info.cycle_tail_release.values().any(|&r| r == root),
        "the (%freeze …) tail site must record cycle_tail_release → merged root r{}; got {:?}",
        root.0,
        info.cycle_tail_release,
    );
}

#[test]
fn merge_refuses_member_passed_by_move_to_foreign_tail() {
    // THE SAFETY BOUNDARY. A member closure `od` passed BY-MOVE as an argument to a
    // non-member tail call `(g od)` must REFUSE the merge. Freeing the arena at the
    // recursion's completion (the deferred release) collides with `od`'s own move/return
    // machinery — which also decrefs the merged arena — a double-free. The escape
    // gate does NOT catch this (an opaque callee's argument is not a return/fiber
    // Shared-seed), and the ANF hoist temp aliasing `od` is a synthetic holder
    // excluded from the sole-held count, so this by-move refusal is the tail gate's
    // own: `arg_bindings` sees a binding whose source region is in the SCC. Contrast
    // `merge_admits_in_lambda_cycle_with_foreign_tail_callee`, where the argument is a
    // value (`(ev k)`'s result), not the member itself.
    // `od` is used in value position (`(g od)`), so call-site forwarding cannot
    // prove its `m` — the diverging guard does (ev stays callee-only and is
    // proven by forwarding from od's `(ev (%sub m 1))`).
    let mut symbols = SymbolTable::new();
    let (hir, arena) = compile_fhir(
        "(def g (fn [x] x)) \
         (def f (fn [k] (letrec [ev (fn [m] (if (%lt m 1) :even (od (%sub m 1)))) \
                                 od (fn [m] (when (%not (%int? m)) (error :m)) \
                                      (if (%lt m 1) :odd (ev (%sub m 1))))] \
                          (g od)))) \
         (f 3)",
        &mut symbols,
    );
    let info = analyze_regions(&hir, &arena);
    let cells = ev_od_cells(&hir, &arena, &symbols, &info);
    assert_eq!(
        cells.len(),
        2,
        "precondition: two compiled forward cells; got {cells:?}"
    );
    for &c in &cells {
        assert_eq!(
            info.merged_root(c),
            c,
            "a cycle passing a member (od) BY-MOVE into a non-member tail (g od) must \
             NOT merge — the deferred release would double-free the arena against od's own \
             move/return release; cell r{} merged; merged_parent={:?}",
            c.0,
            info.merged_parent,
        );
    }
    assert!(
        info.cycle_tail_release.is_empty(),
        "a refused cycle records no non-member tail release site; got {:?}",
        info.cycle_tail_release,
    );
}

#[test]
fn merge_admits_member_passed_by_move_to_native_tail() {
    // THE FACTORY. The same by-move shape as
    // `merge_refuses_member_passed_by_move_to_foreign_tail`, one callee apart: the
    // letrec body's tail is a struct literal over BOTH members, which compiles to a
    // tail call on the `struct` native with `ev` and `od` as direct arguments. The
    // refusal's argument is a callee that REPLACES the frame — its new activation's
    // owned-parameter release is what would decref the arena a second time against the
    // deferred release. A native does neither: it borrows its arguments and it keeps
    // the frame, so the binding-scope `DecrefRegion` stays live and is the arena's
    // single release. The counter-factual is the whole arena: refusing left this cycle
    // Shared and leaked its four regions — two closures, two forward cells — per call,
    // which is the everyday `letrec`-of-closures factory.
    // Both members are used in value position (the struct carries them out), so
    // call-site forwarding proves neither `m` — each takes a diverging guard.
    let mut symbols = SymbolTable::new();
    let (hir, arena, info) = analyze_cycle_with_effects(
        "(def f (fn [k] (letrec [ev (fn [m] (when (%not (%int? m)) (error :m)) \
                                     (if (%lt m 1) :even (od (%sub m 1)))) \
                                 od (fn [m] (when (%not (%int? m)) (error :m)) \
                                      (if (%lt m 1) :odd (ev (%sub m 1))))] \
                          {:a ev :b od}))) \
         (f 3)",
        &mut symbols,
    );
    let cells = ev_od_cells(&hir, &arena, &symbols, &info);
    assert_eq!(
        cells.len(),
        2,
        "precondition: two compiled forward cells; got {cells:?}"
    );
    let roots: rustc_hash::FxHashSet<Region> = cells.iter().map(|&c| info.merged_root(c)).collect();
    assert_eq!(
        roots.len(),
        1,
        "a struct-literal body tail ({{:a ev :b od}}) carries both members by-move into \
         a NATIVE, which cannot replace the frame — the cycle must MERGE; cells={cells:?} \
         merged_parent={:?}",
        info.merged_parent,
    );
    let root = roots.into_iter().next().unwrap();
    assert!(
        !cells.contains(&root),
        "the merged root must be a closure region, not a cell; root=r{} cells={cells:?}",
        root.0,
    );
    // The tail site is recorded like any other non-member tail: the merge classifies
    // the callee to decide the REFUSAL, never to pick the release channel, so a
    // `struct` rebound to a closure still finds its deferred release wired.
    assert!(
        info.cycle_tail_release.values().any(|&r| r == root),
        "the struct-literal tail site must record cycle_tail_release → merged root r{}; \
         got {:?}",
        root.0,
        info.cycle_tail_release,
    );
}
