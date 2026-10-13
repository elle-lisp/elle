// audited: 2026-10-11
//! The closure-cycle merge reclaims a mutual-recursion cycle; a one-way sibling capture needs only RC.
//!
//! docs/impl/region/letrec.md
//! docs/impl/region/relocate.md

use super::*;

/// End-to-end reclamation of the **mutually-recursive** immutable closure cycle by
/// the closure-cycle MERGE. A local `letrec` (`ping`/`pong`) builds an immutable
/// reference cycle: each closure's env references the other, captured at the
/// `letrec` and never mutated. Per-region RC cannot collect it
/// (docs/impl/region/rules.md Rule 8). A mutable cycle needs an owner to free it.
/// An immutable one needs none: the merge collapses the closures and cells onto
/// one arena, freed at the enclosing scope.
///
/// The merge is unconditional, so the counter-factual is the discriminator. The
/// cycle must read bounded while `LEAK_DISCRIMINATOR`, a fiber cycle no cut
/// reclaims, leaks. That proves the gauge detects per-run region growth.
#[test]
fn region_ownership_reclaims_mutual_recursion_closure_cycle() {
    let leak = leak_discriminator();
    assert!(
        leak > 0,
        "gauge live: the refused-cycle discriminator must leak, and its per-run \
         region growth is {leak}. At 0 the gauge does not detect per-run growth, so \
         the bounded assertion below is vacuous",
    );
    // ping <-> pong: two closures whose envs reference each other (immutable cycle).
    let src = "(begin (letrec [ping (fn [n] (if (%lt n 1) :done (pong (%sub n 1)))) \
                               pong (fn [n] (ping n))] \
                        (ping 3)) \
                     nil)";
    let growth = steady_region_growth(src);
    assert!(
        growth <= 0,
        "the closure-cycle merge must reclaim the immutable ping/pong closure cycle: \
         per-run live-region growth {growth} must be <= 0. The discriminator leaks \
         {leak} per run, so the gauge is live",
    );
}

/// PROMPTNESS of the closure-cycle merge's drop site (docs/impl/region/letrec.md).
/// A *discarded* top-level letrec closure cycle must be freed at its BINDING SCOPE:
/// the `letrec` that prebinds its capture cells, which is its true last use. It
/// must not be held to the enclosing post-dominator, the file `Begin`, which is
/// program teardown. This is also the counterweight to the handed-out-member
/// reading: a cycle nothing carries out of the binding scope keeps the tight drop.
///
/// The capture cell is keyed by the letrec NODE, whose enclosing-scope stack
/// excludes itself. Dropped at the allocation-site post-dominator, the cycle would
/// live to the letrec's PARENT, the file Begin for a top-level cycle. Summed over
/// many such cycles, that program-duration over-keep is unbounded RSS.
///
/// Oracle: build N distinct top-level letrec cycles between two
/// `arena/region-count` samples. Each merged cycle is one region. DISCARDED (used,
/// then dropped), each must free at its own letrec, so the count delta stays near
/// zero.
///
/// The DISCRIMINATOR retains each cycle's closure in a program-lifetime array. That
/// cross-region store RC-pins the merged region, so the delta grows ~N, which
/// proves the gauge detects per-cycle region retention. Without it a dead gauge
/// would pass the discarded case. The merge fires identically in both; only the
/// external RC holder differs. The store is also the shape that makes the
/// enclosing-Begin drop sound: a foreign reference into the merged region is
/// RC-counted and outlives the single decref.
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
            // RETAIN uses r{k} in value position (the push), which disables
            // call-site argument forwarding — the diverging guard proves `m` instead.
            src.push_str(&format!(
                "(letrec [r{k} (fn [m] (when (%not (%int? m)) (error :m)) \
                   (if (%lt m 1) :done (r{k} (%sub m 1))))] {body})\n"
            ));
        }
        // `arena/region-count` results are opaque to inference; the match-arm
        // dispatch proves them :integer for the closing `%sub` (no stdlib `-` here).
        src.push_str(
            "(def c1 (arena/region-count))\n\
             (match (type-of c1) :integer (match (type-of c0) :integer (%sub c1 c0) _ -1) _ -1)",
        );
        let result = {
            let (_vm, symbols, cctx) = rt.parts();
            compile_file_repl(&src, symbols, cctx, "<embed>")
                .expect("compiles")
                .0
        };
        let (vm, _symbols, cctx) = rt.parts();
        vm.execute_scheduled(&result, cctx)
            .expect("runs")
            .as_int()
            .expect("program returns the region-count delta as an int")
    }

    let discarded = region_growth(false);
    let retained = region_growth(true);
    assert!(
        retained > 150,
        "precondition: retaining each cycle's closure in a program-lifetime array \
         grows the live region count ~N, and it grew {retained}. If small, the gauge \
         does not detect per-cycle retention, so the assertion below is vacuous",
    );
    assert!(
        discarded < 50,
        "a discarded top-level letrec closure cycle must be freed at its binding-scope \
         letrec, not held to program teardown. Region growth over 200 cycles is \
         {discarded} and must be near zero. A reading of ~200 means each merged cycle \
         survives to the file Begin's scope exit, the coarse allocation-site drop",
    );
}

/// Per-CALL reclamation of an in-lambda MUTUAL letrec cycle, the closure-cycle
/// merge's in-lambda case (docs/impl/region/letrec.md; the `recur-local-mutual`
/// row of tests/impl/probe/tailcall.lisp). Each `(f 3)` builds one ev↔od
/// cell↔closure cycle inside `f`'s body. The merge collapses the four members (two
/// closures and two forward cells) onto one arena. The letrec body `(ev k)` is a
/// tail call to a member, so the tail-call deferred release releases that arena
/// once, at the recursion's normal completion.
///
/// `(f 0)` is the base-case-only path: the recursion never rotates to a sibling,
/// so the ENTRY call's adopt is the sole release channel. A marking that covered
/// only interior rotations would leak exactly this path.
///
/// The merge is unconditional (it rides `compute_merges`). Oracle: per-iteration
/// live-region growth via `arena/region-count`, sampled mid-run BY THE PROGRAM
/// after 50 and then 250 calls. Beside it, the growth of the self-referential
/// accumulator discriminator proves the gauge is live.
#[test]
fn region_ownership_reclaims_nested_mutual_recursion_per_call() {
    let prelude = "(def f (fn [k] \
        (letrec [ev (fn [m] (if (%lt m 1) :even (od (%sub m 1)))) \
                 od (fn [m] (if (%lt m 1) :odd (ev (%sub m 1))))] \
          (ev k))))";

    // Discriminator: the self-referential accumulator legitimately retains every
    // prior, proving the gauge detects per-iteration region growth.
    let live_chain_growth = mid_run_discriminator(Runtime::new(), "arena/region-count");
    assert!(
        live_chain_growth > 150,
        "precondition: the live accumulator retains every prior, so region growth \
         over 200 iterations must be large (~200), and it is {live_chain_growth}. If \
         small, the gauge is dead and the assertions below are vacuous",
    );

    let rotating = mid_run_growth(Runtime::new(), prelude, "(f 3)", "arena/region-count");
    assert!(
        rotating < 50,
        "the closure-cycle merge and the tail-call deferred release must reclaim an \
         in-lambda mutual letrec cycle per call. Region growth over 200 calls is \
         {rotating} and must be near zero. Each call's merged arena leaks if the \
         cycle is refused or nothing supplies the stranded binding-scope drop",
    );
    let base_case = mid_run_growth(Runtime::new(), prelude, "(f 0)", "arena/region-count");
    assert!(
        base_case < 50,
        "the base-case-only path, `(f 0)` with no sibling rotation, must also \
         reclaim: the ENTRY tail call's adopt is the sole release channel there. \
         Region growth over 200 calls is {base_case} and must be near zero",
    );
}

/// Per-CALL reclamation where ONE tail call carries BOTH deferred channels
/// (docs/impl/region/letrec.md). `go` is self-recursive AND sibling-captured. So
/// it keeps a forward cell, which the single-closure self-edge admission collapses
/// onto its closure region. `outer` captures that cell, and the letrec body
/// tail-calls `outer`. That `TailCall` therefore names the arena in
/// `deferred_release_slot` and flags `defer_callee_release` for `outer`'s own
/// per-call region. Those are two regions and two references the frame owns, and
/// neither substitutes for the other.
///
/// Running only one of them reclaims nothing. `outer`'s counted `closure ⊇ cell`
/// edge holds the arena off zero, so the arena's decref frees nothing while
/// `outer`'s region survives. The gauge is per-iteration live-region growth beside
/// the live-chain discriminator. So a regression that drops either channel reads
/// as ~5 objects in 2 regions per call.
#[test]
fn region_ownership_reclaims_sibling_captured_member_per_call() {
    let prelude = "(def f (fn [k] \
        (letrec [go (fn [m] (if (%lt m 1) :done (go (%sub m 1)))) \
                 outer (fn [m] (go m))] \
          (outer k))))";

    let live_chain_growth = mid_run_discriminator(Runtime::new(), "arena/region-count");
    assert!(
        live_chain_growth > 150,
        "precondition: the live accumulator retains every prior, so region growth \
         over 200 iterations must be large (~200), and it is {live_chain_growth}. If \
         small, the gauge is dead and the assertions below are vacuous",
    );

    let growth = mid_run_growth(Runtime::new(), prelude, "(f 3)", "arena/region-count");
    assert!(
        growth < 50,
        "a letrec whose sibling captures a self-recursive member must reclaim both \
         the merged arena and the sibling callee per call. Region growth over 200 \
         calls is {growth} and must be near zero. Either deferred channel alone \
         reclaims neither: the sibling's counted edge holds the arena off zero",
    );
    let base_case = mid_run_growth(Runtime::new(), prelude, "(f 0)", "arena/region-count");
    assert!(
        base_case < 50,
        "the base-case-only path must also reclaim both regions, because the body's \
         single tail call to the sibling is the sole release point either way. \
         Region growth over 200 calls is {base_case} and must be near zero",
    );
}

/// Per-CALL reclamation of a **one-way** sibling capture: `go` calls `helper`, and
/// `helper` does not call back. There is no SCC, so no merge and no cycle channel.
/// `helper` simply keeps a prebound forward cell for `go`'s benefit, and
/// per-region RC reclaims the pair. Where the CELL's release lands decides whether
/// it does.
///
/// Its binding-scope `DecrefRegion` sits past the letrec body's frame-replacing
/// tail call. The frame-exit relocation's count argument is read over holder
/// BINDINGS, which name the closure region a cell points at, never the cell's own.
/// So the cell carries its binding's verdict one indirection out, and stranding it
/// strands the sibling with it. The cell's reference is what holds that closure's
/// region off zero (docs/impl/region/relocate.md).
///
/// Two shapes, one per face: nothing leaves the frame, and the capturer is
/// RETURNED. In the second, the counted `closure ⊇ cell` edge is what stands
/// between the relocated release and the caller's minted reference. Both leak
/// three objects in two regions per call when the cell's release stays in the dead
/// block: the cell, the sibling closure and its env.
#[test]
fn region_ownership_reclaims_sibling_captured_forward_cell_per_call() {
    let live_chain = mid_run_discriminator(Runtime::new(), "arena/region-count");
    assert!(
        live_chain > 150,
        "precondition: the self-referential accumulator retains every prior, so \
         region growth over 200 iterations must be large (~200), and it is \
         {live_chain}. If small, the gauge is dead and the assertions below are vacuous",
    );

    let plain = mid_run_growth(
        Runtime::new(),
        "(def f (fn [k] \
            (letrec [helper (fn [x] (%sub x 1)) \
                     go (fn [m] (helper m))] \
              (go k))))",
        "(f 3)",
        "arena/region-count",
    );
    assert!(
        plain < 50,
        "a one-way sibling capture must be reclaimed per call. Region growth over 200 \
         calls is {plain} and must be near zero. A forward cell's binding-scope \
         release past the letrec body's tail call is dead, and the sibling closure \
         the cell holds cannot reach zero until the cell does",
    );

    let returned = mid_run_growth(
        Runtime::new(),
        "(def f (fn [k] \
            (letrec [helper (fn [x] (when (%not (%int? x)) (error :x)) (%sub x 1)) \
                     go (fn [m] (when (%not (%int? m)) (error :m)) \
                          (if (%lt m 1) go (go (helper m))))] \
              (go k))))",
        "(f 3)",
        "arena/region-count",
    );
    assert!(
        returned < 50,
        "the same pair with the CAPTURER handed back must also reclaim. Escape marks \
         the sibling escaping by the return facet alone, so the tail callee's \
         `closure ⊇ cell` edge funds the relocated release. Region growth over 200 \
         calls is {returned}, and the discriminator grows {live_chain}",
    );
}

/// Per-CALL reclamation of the closure-as-module factory
/// (docs/impl/region/letrec.md; the `defn-module-factory` row of
/// tests/impl/probe/tailcall.lisp). The factory is a constructor that defines
/// mutually recursive helpers over its own mutable state and hands back a struct
/// of them. The helpers are local `defn`s, so the `Begin` they sit in prebinds
/// their forward cells, not a `Letrec` node. The merge must collapse the same
/// SCC ∪ cells onto one arena.
///
/// The factory's value makes this the interesting half. The struct holds the
/// members, a FOREIGN capture of the arena that is RC-counted. So the arena
/// outlives the single binding-scope decref and dies with the struct. Each call
/// therefore builds and drops one whole module, which is the shape the async
/// scheduler is built as.
///
/// Oracle: per-iteration live-region growth via `arena/region-count`, sampled
/// mid-run BY THE PROGRAM. Beside it, the growth of the self-referential
/// accumulator discriminator proves the gauge is live.
#[test]
fn region_ownership_reclaims_defn_module_factory_per_call() {
    let prelude = "(def mk (fn [] \
        (let [t @{}] \
          (defn fa [m] (when (%not (%int? m)) (error :m)) \
                       (if (%lt m 1) t (fb (%sub m 1)))) \
          (defn fb [m] (when (%not (%int? m)) (error :m)) \
                       (put t m true) (fa (%sub m 1))) \
          (let [s {:a fa :b fb}] s))))";

    // Discriminator: the self-referential accumulator legitimately retains every
    // prior, proving the gauge detects per-iteration region growth.
    let live_chain_growth = mid_run_discriminator(Runtime::new(), "arena/region-count");
    assert!(
        live_chain_growth > 150,
        "precondition: the live accumulator retains every prior, so region growth \
         over 200 iterations must be large (~200), and it is {live_chain_growth}. If \
         small, the gauge is dead and the assertions below are vacuous",
    );

    let built = mid_run_growth(Runtime::new(), prelude, "(mk)", "arena/region-count");
    assert!(
        built < 50,
        "a factory whose local `defn`s form a mutual-recursion cycle must reclaim \
         that cycle per call. Region growth over 200 constructions is {built} and \
         must be near zero. A run read as a shape other than a letrec's refuses the \
         merge and strands every closure and forward cell it built",
    );
    let used = mid_run_growth(
        Runtime::new(),
        prelude,
        "((get (mk) :a) 2)",
        "arena/region-count",
    );
    assert!(
        used < 50,
        "the same holds when a member is CALLED through the struct the factory \
         returned — the struct's hold is a counted reference out of the arena, and \
         it dies with the struct — got {used}",
    );
}

mod returned;
