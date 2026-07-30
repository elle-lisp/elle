use super::*;

/// End-to-end reclamation of the **mutually-recursive** immutable closure cycle by
/// the closure-cycle MERGE. A local `letrec` (`ping`/`pong`) builds an immutable
/// reference cycle: each closure's env references the other, captured at the
/// `letrec`, never mutated. Per-region RC cannot collect it (region/rules.md Rule
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
    let leak = leak_discriminator();
    assert!(
        leak > 0,
        "gauge live: the refused-cycle discriminator must leak (per-run region growth \
         {leak}); if 0 the gauge is not detecting per-run growth and the bounded \
         assertion below is vacuous",
    );
    // ping <-> pong: two closures whose envs reference each other (immutable cycle).
    let src = "(begin (letrec [ping (fn [n] (if (%lt n 1) :done (pong (%sub n 1)))) \
                               pong (fn [n] (ping n))] \
                        (ping 3)) \
                     nil)";
    let growth = steady_region_growth(src);
    assert!(
        growth <= 0,
        "the immutable ping/pong closure cycle must be reclaimed by the closure-cycle \
         merge — per-run live-region growth {growth} must be <= 0 (the discriminator \
         leaks {leak} per run, so the gauge is live)",
    );
}

/// PROMPTNESS of the closure-cycle merge's drop site (docs/impl/region/letrec.md
/// § "Drop site — the binding scope"). A *discarded*
/// top-level letrec closure cycle must be freed at its BINDING SCOPE — the `letrec`
/// that prebinds its capture cells — its true last use, NOT held to the enclosing
/// post-dominator (the file `Begin`, i.e. program teardown). This is also the
/// counterweight to the handed-out-member reading: a cycle nothing carries out of the
/// binding scope must keep the tight drop, never inherit a later one. The capture cell is
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
/// (the tail-call deferred release for a self-tail-loop), NOT the merge (which serves the
/// cell-bearing mutual cycle). Same bounded-vs-discriminator counterfactual as the
/// mutual case: reclaimed self-recursion reads bounded region growth beside a leaking
/// bare-@array-cycle discriminator, and — being cell-free RC/adopt, flag-independent
/// like the merge — bounded at both flag settings.
#[test]
fn region_ownership_reclaims_self_recursion_closure_cycle() {
    let leak = leak_discriminator();
    assert!(
        leak > 0,
        "gauge live: the refused-cycle discriminator must leak (per-run region growth \
         {leak}); if 0 the gauge is not detecting per-run growth and the bounded \
         assertion below is vacuous",
    );
    // loop references itself: a cell-free self-recursion (no forward cell, LoadSelf).
    let src = "(begin (letrec [loop (fn [n] (if (%lt n 1) :done (loop (%sub n 1))))] \
                        (loop 3)) \
                     nil)";
    let growth = steady_region_growth(src);
    assert!(
        growth <= 0,
        "the cell-free self-recursive closure must be reclaimed by ordinary RC / the \
         tail-call deferred release — per-run live-region growth {growth} must be <= 0 (the \
         discriminator leaks {leak} per run, so the gauge is live)",
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
/// dead code; the tail-call deferred release (`lir/lower/control/call.rs::tail_callee_defers_release`,
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
    // Subject: `f` wraps a self-recursive letrec closure; each `(f 3)` builds and discards
    // one cell↔closure cycle, which must be reclaimed per call.
    let call_growth = mid_run_growth(
        Runtime::new(),
        "(def f (fn [k] \
            (letrec [loop (fn [m] (if (%lt m 1) :done (loop (%sub m 1))))] \
              (loop k))))",
        "(f 3)",
        "arena/region-count",
    );
    // Discriminator: the self-referential accumulator legitimately retains every prior.
    let live_chain_growth = mid_run_discriminator(Runtime::new(), "arena/region-count");

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
         reclaimed per call by the tail-call deferred release — region growth over 200 calls must be \
         near zero, got {call_growth} (each call's stranded closure region leaks to program \
         teardown if the adopt does not supply its release)",
    );
}

/// Per-CALL reclamation of an in-lambda MUTUAL letrec cycle — the closure-cycle
/// merge's in-lambda case (docs/impl/region/letrec.md § The letrec closure-cycle
/// merge; oracle.lisp `recur-local-mutual`). Each `(f 3)` builds one ev↔od
/// cell↔closure cycle inside `f`'s body; the merge collapses the four members
/// (two closures + two forward cells) onto one arena, and — the letrec body
/// `(ev k)` being a tail call to a member — the tail-call deferred release releases that
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
         over 200 iterations must be large (~200) — got {live_chain_growth}; if \
         small, the gauge is dead and the assertions below are vacuous",
    );

    let rotating = mid_run_growth(Runtime::new(), prelude, "(f 3)", "arena/region-count");
    assert!(
        rotating < 50,
        "an in-lambda mutual letrec cycle must be reclaimed per call by the \
         closure-cycle merge + the tail-call deferred release — region growth over 200 calls \
         must be near zero, got {rotating} (each call's merged arena leaks if the \
         cycle is refused or the stranded binding-scope drop is never supplied)",
    );
    let base_case = mid_run_growth(Runtime::new(), prelude, "(f 0)", "arena/region-count");
    assert!(
        base_case < 50,
        "the base-case-only path (`(f 0)` — no sibling rotation) must also reclaim: \
         the ENTRY tail call's adopt is the sole release channel there — region \
         growth over 200 calls must be near zero, got {base_case}",
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
/// discarded `loop` closures; with the tail-scoped adopt (`tail_callee_defers_release` /
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
        (match (type-of b) :integer (match (type-of a) :integer (%sub b a) _ -1) _ -1)";
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
         leaks if the tail-call deferred release does not supply the tail-call-stranded scope-end \
         DecrefRegion",
    );
}

/// Per-call reclamation of a stranded recursive closure the recursion **RETURNS** —
/// the return-funded admission (docs/impl/selfrec.md § "The deferral's escape gate is
/// the fiber frontier alone"). Each subject's letrec/def body is a frame-replacing tail
/// call, so the closure region's scope-end `DecrefRegion` is dead (or suppressed) and
/// the tail-call deferred release is the region's only release channel. Returning the
/// closure does not withdraw that channel: the callee's `Return` mints the caller's
/// reference *before* `trampoline_loop` breaks and runs the deferred decref, so the
/// count between the two is the caller's and the deferral drops only the frame's own.
///
/// Two shapes, one per stranding route — a `letrec` self-loop (dead scope-end drop)
/// and a `def` self-loop (`suppressed_self_regions`). Each result is DISCARDED at the
/// call site, so nothing legitimately retains it and the whole per-call region must
/// come back. Boolean-only bodies keep them on `Runtime::without_stdlib()`, where no
/// trait dispatch churns the region count.
///
/// A returned member of a MUTUAL SCC is deliberately not here: it is not cell-free, so
/// its release is the closure-cycle merge's arena rather than this stranded-self channel.
/// The merge admits it on the same return-mint argument (region/letrec.md § The frontier
/// gate) and `region_ownership_reclaims_returned_mutual_cycle_per_call` is its gauge.
///
/// Counterfactual: each reads ~200 (one stranded region per call, closure + env
/// together) while the return facet blanket-refuses the deferral; ~0 once the refusal
/// narrows to the fiber frontier. The live accumulator beside them proves the gauge
/// sees per-call region growth at all.
#[test]
fn recursive_returned_closure_reclaims_per_call() {
    let growth = |prelude: &str, body: &str| {
        mid_run_growth(
            Runtime::without_stdlib(),
            prelude,
            body,
            "arena/region-count",
        )
    };

    let live = mid_run_discriminator(Runtime::without_stdlib(), "arena/region-count");
    assert!(
        live > 150,
        "gauge-live: the self-referential accumulator retains every prior, so region \
         growth over 200 iterations must be ~200 — got {live}; if small the gauge is \
         dead and every assertion below is vacuous",
    );

    // `letrec` self-loop: the scope-end DecrefRegion is emitted past the `(loop k)`
    // TailCall (dead code), so the deferral is the sole channel.
    let letrec_self = growth(
        "(def frec (fn [k] (letrec [loop (fn [m] (if m loop (loop true)))] (loop k))))",
        "(frec false)",
    );
    assert!(
        letrec_self < 50,
        "a returned cell-free self-recursive `letrec` closure must still be reclaimed \
         per call — its caller's reference is the `Return` mint, and the deferred \
         release drops only the frame's own: region growth over 200 discarded calls \
         must be ~0, got {letrec_self}",
    );

    // `def` self-loop: the would-be-live DecrefRegion is suppressed instead of dead,
    // reproducing the letrec accounting through the other stranding route.
    let define_self = growth(
        "(def fdef (fn [k] (def loop (fn [m] (if m loop (loop true)))) (loop k)))",
        "(fdef false)",
    );
    assert!(
        define_self < 50,
        "a returned cell-free self-recursive `def` closure must be reclaimed per call \
         (its release is suppressed, so the deferral is the only channel): region \
         growth over 200 discarded calls must be ~0, got {define_self}",
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
/// (`stranded_self_bindings`); the tail-call deferred release is then the sole, once-only release,
/// reproducing the `letrec` path's accounting. The gauge (region growth over 200 calls)
/// additionally pins the region is reclaimed per call — a leak would grow it unbounded.
///
/// Counterfactual: panics with the `tag/object mismatch` UAF before the `def`-stranding +
/// premature-decref-suppression fix; runs clean and bounded after.
#[test]
fn self_recursive_define_with_arith_reclaims_per_call() {
    // Subject: a self-recursive `def` (not `letrec`) nested in a lambda, recursing with
    // heap-allocating stdlib `<`/`-` so a freed `R_cell` page is recycled mid-recursion.
    // A crash inside the run panics here (the RED counterfactual); a leak grows the delta.
    let call_growth = mid_run_growth(
        Runtime::new(),
        "(def f (fn [k] \
            (def loop (fn [m] (if (< m 1) :done (loop (- m 1))))) \
            (loop k)))",
        "(f 3)",
        "arena/region-count",
    );
    // Discriminator: the self-referential accumulator legitimately retains every prior, so
    // the gauge MUST see large growth here — else the bounded assertion below is vacuous.
    let live_chain_growth = mid_run_discriminator(Runtime::new(), "arena/region-count");

    assert!(
        live_chain_growth > 150,
        "precondition: the live accumulator retains every prior, so region growth over 200 \
         iterations must be large (~200) — got {live_chain_growth}; a small value means the \
         gauge is dead and the assertion below is vacuous",
    );
    assert!(
        call_growth < 50,
        "a cell-free self-recursive `def` closure must be reclaimed per call by the \
         tail-call deferred release — region growth over 200 calls must be near zero, got {call_growth} \
         (its per-call closure region leaks, or worse, is freed before the `(loop k)` tail \
         call re-enters it)",
    );
}

/// A self-recursive `def` nested in a lambda is cell-free (docs/impl/selfrec.md), handled
/// exactly like a self-recursive `letrec`: no forward cell, the self-reference resolves to
/// the executing closure. `lower_define` STRANDS the binding (`stranded_self_bindings`) and
/// SUPPRESSES its closure region's would-be-live `DecrefRegion` (`suppressed_self_regions`)
/// so the tail-call deferred release is the sole release — the closure region must be freed EXACTLY
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

/// A self-recursive `def` that is ALSO captured by a sibling is NOT cell-free: the
/// sibling capture makes it `needs_capture`, so it keeps a forward cell whose cascade
/// owns the closure region's single release (docs/impl/selfrec.md § the cell-free gate).
/// `lower_define`/`lower_letrec` therefore must NOT strand it — stranding a cell-held
/// binding makes the tail-call deferred release decref its region a SECOND time, under the still-live
/// cell (the captured-self-tail double-free). This is the runtime peer to
/// `tests/elle/region-selfrec-captured-tail-release.lisp`: `loop` self-recurses AND is
/// captured by `other`, so it must run to completion with its region freed exactly once.
/// A regression that re-strands it trips the `tail_callee_defers_release` consumer assertion
/// (a loud panic at the seam) or, in release, the `DecrefRegion` double-free panic.
#[test]
fn self_recursive_and_sibling_captured_no_double_free() {
    use crate::pipeline::compile_file_repl;
    let src = "(def outer (fn [k] \
        (def loop (fn [m] (if m :done (loop true)))) \
        (def other (fn [] (loop k))) \
        (other))) \
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
        .expect("a sibling-captured self-recursive `def` must not double-free its closure region");
    assert!(
        v.is_keyword(),
        "the sibling-captured recursive `def` returns the :done keyword, got {v:?}"
    );
}

/// The closure-cycle merge's **return-funded admission**
/// (docs/impl/region/letrec.md § The frontier gate): the ev/od cycle with a member
/// RETURNED still reclaims per call. One base case apart from
/// `region_ownership_reclaims_mutual_recursion_closure_cycle`, `ev` hands back `ev`
/// itself, so `ev`'s region carries escape's return facet — which used to refuse the
/// whole SCC to Shared, where per-region RC cannot collect a cycle and both closures
/// and both forward cells stayed live once per call.
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
/// Same bounded-vs-discriminator counterfactual as the non-returning case: the merge is
/// unconditional, so the pin is per-run region growth beside the leaking bare-@array
/// cycle, whose slope proves the gauge is live.
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
/// (docs/impl/region/letrec.md § The frontier gate).
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
/// different reason; [`region_ownership_reclaims_returned_cycle_bound_out_of_tail_position`]
/// is its gauge. The bare-`@array` discriminator asserted first is what proves the
/// bounded assertions here are measuring reclamation rather than a dead gauge.
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
/// member's region directly (docs/impl/region/letrec.md § "Drop site — following a
/// handed-out member"). No mint stands between the letrec and the binding scope, so a
/// release pinned there would free the arena under `c`; the merge instead adopts the
/// point the last-use rule already computed for the handed-out member — the enclosing
/// `Return`, whose mint precedes that node's own releases — and waives the sole-held
/// proxy for the member it followed.
///
/// Two shapes, because the two halves of that reading are independent: the value is
/// RETURNED out of `f` (the release rides the `Return` pin), and the value is CALLED
/// inside `f` and never handed further (the release rides its ordinary last use). Both
/// reclaim the whole cycle — two closures and two forward cells — per call.
///
/// Counterfactual: both read the discriminator's slope while the cycle is refused to
/// Shared, where per-region RC cannot collect it.
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

/// Per-CALL reclamation of a **one-way** sibling capture — `go` calls `helper`,
/// `helper` does not call back. There is no SCC, so no merge and no cycle channel:
/// `helper` simply keeps a prebound forward cell for `go`'s benefit and per-region RC
/// reclaims the pair. What decides whether it does is where the CELL's release lands.
/// Its binding-scope `DecrefRegion` sits past the letrec body's frame-replacing tail
/// call, and the frame-exit relocation's count argument is read over holder BINDINGS —
/// which name the closure region a cell points at, never the cell's own. So the cell
/// carries its binding's verdict one indirection out, and stranding it strands the
/// sibling with it: the cell's reference is what holds that closure's region off zero
/// (docs/impl/region/mechanism.md § "A compiled capture cell is frame-held exactly as
/// its binding is").
///
/// Two shapes, one per admission half: nothing leaves the frame (the sole-held half),
/// and the capturer is RETURNED (the return-funded half, where the funding edge is
/// `closure ⊇ cell` and must name the cell as well as the closure it points at). Both
/// leak three objects in two regions per call — the cell, plus the sibling closure and
/// its env — when the cell's release stays in the dead block.
#[test]
fn region_ownership_reclaims_sibling_captured_forward_cell_per_call() {
    let live_chain = mid_run_discriminator(Runtime::new(), "arena/region-count");
    assert!(
        live_chain > 150,
        "precondition: the self-referential accumulator legitimately retains every \
         prior, so region growth over 200 iterations must be large (~200) — got \
         {live_chain}; if small the gauge is dead and the assertions below are vacuous",
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
        "a one-way sibling capture must be reclaimed per call — region growth over 200 \
         calls must be near zero, got {plain} (the forward cell's binding-scope release \
         is dead past the letrec body's tail call, and the sibling closure it holds \
         cannot reach zero until the cell does)",
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
        "the same pair with the CAPTURER handed back must also reclaim — escape marks \
         the sibling escaping by the return facet and no other, so the tail callee's \
         `closure ⊇ cell` edge funds the relocated release — but region growth over 200 \
         calls is {returned} (the discriminator grows {live_chain}, so the gauge is live)",
    );
}
