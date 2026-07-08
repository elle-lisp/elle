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
         tail-call adopt — per-run live-region growth {growth} must be <= 0 (the \
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
/// merge's in-lambda case (docs/impl/region/letrec.md § The letrec closure-cycle
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

    // Run `body` 50 then 250 times in one program, sampling `arena/region-count` at
    // each point; returns c250 − c50.
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

    let prelude = "(def f (fn [k] \
        (letrec [ev (fn [m] (if (%lt m 1) :even (od (%sub m 1)))) \
                 od (fn [m] (if (%lt m 1) :odd (ev (%sub m 1))))] \
          (ev k))))";

    // Discriminator: the self-referential accumulator legitimately retains every
    // prior, proving the gauge detects per-iteration region growth.
    let live_chain_growth = region_growth("(def @acc nil)", "(assign acc (%pair n acc))");
    assert!(
        live_chain_growth > 150,
        "precondition: the live accumulator retains every prior, so region growth \
         over 200 iterations must be large (~200) — got {live_chain_growth}; if \
         small, the gauge is dead and the assertions below are vacuous",
    );

    let rotating = region_growth(prelude, "(f 3)");
    assert!(
        rotating < 50,
        "an in-lambda mutual letrec cycle must be reclaimed per call by the \
         closure-cycle merge + the tail-call adopt — region growth over 200 calls \
         must be near zero, got {rotating} (each call's merged arena leaks if the \
         cycle is refused or the stranded binding-scope drop is never supplied)",
    );
    let base_case = region_growth(prelude, "(f 0)");
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

/// A self-recursive `def` that is ALSO captured by a sibling is NOT cell-free: the
/// sibling capture makes it `needs_capture`, so it keeps a forward cell whose cascade
/// owns the closure region's single release (docs/impl/selfrec.md § the cell-free gate).
/// `lower_define`/`lower_letrec` therefore must NOT strand it — stranding a cell-held
/// binding makes the tail-call adopt decref its region a SECOND time, under the still-live
/// cell (the captured-self-tail double-free). This is the runtime peer to
/// `tests/elle/region-selfrec-captured-tail-adopt.lisp`: `loop` self-recurses AND is
/// captured by `other`, so it must run to completion with its region freed exactly once.
/// A regression that re-strands it trips the `tail_callee_adopts` consumer assertion
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
