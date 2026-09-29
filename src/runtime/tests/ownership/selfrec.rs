// audited: 2026-09-29
//! A cell-free self-recursive closure's per-call region comes back, whatever tail its body ends in.
//!
//! docs/impl/selfrec.md

use super::*;

/// Companion to the mutual case (`region_ownership_reclaims_mutual_recursion_closure_cycle`,
/// selfrec/cycle.rs): a **self-recursive** `letrec` closure (`loop` references itself) — the
/// most pervasive recursive shape (every recursive local fn). Unlike the mutual cycle this is
/// **cell-free**: the self-edge does not mark `loop` captured (`mark_captured`,
/// src/hir/arena.rs), so it has no forward cell and no cell↔closure cycle — its self-reference
/// resolves to the executing closure (`LoadSelf` / a self-call). The per-call closure region
/// is reclaimed by ordinary RC (the tail-call deferred release for a self-tail-loop), NOT the
/// merge (which serves the cell-bearing mutual cycle). The counter-factual is the
/// discriminator, as in the mutual case: reclaimed self-recursion reads bounded region growth
/// beside a leaking bare-@array self-cycle.
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
/// varargs). `region_ownership_reclaims_self_recursion_closure_cycle` above builds a TOP-LEVEL
/// letrec and re-runs the whole program, so it never invokes a nested letrec; this drives a
/// nested one many times within one run.
///
/// A self-recursive `loop` is **cell-free**: its self-edge does not mark it captured
/// (`mark_captured`, src/hir/arena.rs), so there is no forward cell and no cell↔closure cycle —
/// its self-reference resolves to the executing closure (`LoadSelf` / a self-call). The closure
/// is an ordinary per-call region whose demise the recursive `TailCall` strands as dead code;
/// the tail-call deferred release (`tail_callee_defers_release`,
/// src/lir/lower/control/call/defer.rs; `stranded_self_bindings`) supplies the once-only
/// release at the recursion's normal completion, so the region is reclaimed per call —
/// RC-identical to a top-level recursive `defn`. (The merge is unrelated here: it serves the
/// cell-bearing MUTUAL cycle, not this cell-free self-recursion.)
///
/// Oracle: per-iteration live-region growth via `arena/region-count`, sampled mid-run BY
/// THE PROGRAM (after 50 then 250 invocations) and returned as the raw delta, exactly as
/// `reassign_toplevel_prior_release_is_bounded` does. The self-referential accumulator
/// `(assign acc (%pair n acc))` is the built-in discriminator: every prior IS live, so
/// its delta legitimately grows ~200 — proving the gauge detects per-iteration growth, so
/// a near-zero delta for the letrec call is real reclamation, not a dead gauge.
#[test]
fn closure_cycle_nested_letrec_reclaims_per_call() {
    // Subject: `f` wraps a self-recursive letrec closure; each `(f 3)` builds and
    // discards one closure, which must be reclaimed per call.
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
    let (vm, _symbols, cctx) = rt.parts();
    let delta = vm
        .execute_scheduled(&res.bytecode, cctx)
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

/// Per-call reclamation of the same cell-free closure where the `letrec` BODY's
/// tail is a **branch** whose arms each leave through a frame-replacing callee
/// (docs/impl/region/mechanism.md). The recursion has completed by the branch, so the
/// deferral channel does not apply here: the release the frame owes is the
/// scope-end one, emitted at a merge label no arm arrives at, and the frame-exit
/// relocation supplies it by replicating the release ahead of each arm's
/// `TailCall`. A replica counts once only where the run nil-stamps the slot it
/// read, so the closure region takes the value route of the slot its `letrec`
/// binder recorded rather than its default release by region id.
///
/// Boolean-only for `Runtime::without_stdlib()`, and both arms are driven, since
/// the claim is that exactly one release runs on each path. The closure and its
/// env are two regions per call, so a surviving strand grows the sample by ~20
/// over the ten discarded calls.
#[test]
fn self_recursive_loop_under_a_branch_tail_reclaims_per_call() {
    use crate::pipeline::compile_file_repl;
    // ONE compile, as `self_recursive_loop_reclaims_per_call_no_stdlib`: a fresh
    // REPL compile renumbers global slots.
    let src = "(def s (fn [] :a)) (def s2 (fn [] :b)) \
        (def f (fn [k t] \
          (letrec [loop (fn [m] (if m :done (loop true)))] \
            (loop k) \
            (if t (s) (s2))))) \
        (f false true) \
        (def a (arena/region-count)) \
        (f false true) (f false false) (f false true) (f false false) \
        (f false true) (f false false) (f false true) (f false false) \
        (f false true) (f false false) \
        (def b (arena/region-count)) \
        (match (type-of b) :integer (match (type-of a) :integer (%sub b a) _ -1) _ -1)";
    let mut rt = Runtime::without_stdlib();
    let res = {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file_repl(src, symbols, cctx, "<embed>")
            .expect("compiles")
            .0
    };
    let (vm, _symbols, cctx) = rt.parts();
    let delta = vm
        .execute_scheduled(&res.bytecode, cctx)
        .expect("runs")
        .as_int()
        .expect("program returns the region-count delta as an int");
    assert!(
        delta <= 2,
        "a cell-free self-recursive closure under a BRANCH body tail must be \
         reclaimed per call: live region growth over 10 discarded closures must be \
         ~0, got {delta} — its scope-end release lands at a merge label no arm \
         arrives at unless the relocation replicates it into each of them",
    );
}

/// Per-call reclamation of a stranded recursive closure the recursion **RETURNS** —
/// the return-funded admission (docs/impl/selfrec.md). Each subject's letrec/def body
/// is a frame-replacing tail call, so the closure region's scope-end `DecrefRegion`
/// is dead (or suppressed) and the tail-call deferred release is the region's only
/// release channel. Returning the closure does not withdraw that channel: the
/// callee's `Return` mints the caller's reference *before* `trampoline_loop` breaks
/// and runs the deferred decref, so the count between the two is the caller's and the
/// deferral drops only the frame's own.
///
/// Two shapes, one per binder face of the dead drop — a `letrec` self-loop (whose demise
/// is the letrec scope end) and a `def` self-loop (whose demise is the binding's last
/// use, which for this body IS the tail call). Each result is DISCARDED at the
/// call site, so nothing legitimately retains it and the whole per-call region must
/// come back. Boolean-only bodies keep them on `Runtime::without_stdlib()`, where no
/// trait dispatch churns the region count.
///
/// A returned member of a MUTUAL SCC is deliberately not here: it is not cell-free, so
/// its release is the closure-cycle merge's arena rather than this stranded-self channel.
/// The merge admits it on the same return-mint argument (docs/impl/region/letrec.md), and
/// `region_ownership_reclaims_returned_mutual_cycle_per_call` (selfrec/cycle/returned.rs) is
/// its gauge.
///
/// Counterfactual: each reads ~200 (one stranded region per call, closure + env
/// together) if the return facet refuses the deferral. The live accumulator
/// beside them proves the gauge sees per-call region growth at all.
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

    // `def` self-loop: the demise is the binding's last use, which for this body is the
    // `(loop k)` TailCall itself — the same dead block, reached through the other binder.
    let define_self = growth(
        "(def fdef (fn [k] (def loop (fn [m] (if m loop (loop true)))) (loop k)))",
        "(fdef false)",
    );
    assert!(
        define_self < 50,
        "a returned cell-free self-recursive `def` closure must be reclaimed per call \
         (its release is dead past the tail call, so the deferral is the only channel): \
         region growth over 200 discarded calls must be ~0, got {define_self}",
    );
}

/// Per-call reclamation of a stranded recursive closure that crosses the **fiber
/// frontier** — the other half of the unconditional deferral (docs/impl/selfrec.md). Each
/// subject's letrec body hands the closure
/// across a fiber boundary and then tail-calls it, so its scope-end `DecrefRegion` is
/// dead past the frame replacement and the deferral is the region's only channel.
///
/// Crossing the frontier is not a reason to withhold that channel, because the crossing
/// counts a reference of its own: an emitted value takes the park retain into
/// `fiber.signal` (which the resumer's result release consumes), and a `chan/send`
/// message is increfed at the send site until a receive builds the result carrying it.
/// The deferral drops the frame's own reference and no other, and it runs at the
/// recursion's normal completion — after the crossing, which is a node of the same body.
///
/// Three shapes: the emit seed (`yield`, driven by an external resume per op) through
/// both binder routes, and the `Sends` seed (`chan/send`, received in place). Each
/// discards the delivered closure, so nothing legitimately retains it and the whole
/// per-call region must come back.
///
/// Counterfactual: each reads ~200 — one stranded region per call, closure and env
/// together — if the deferral refuses a fiber-crossing binding. The live
/// accumulator beside them proves the gauge sees per-call region growth at all.
#[test]
fn fiber_crossing_recursive_closure_reclaims_per_call() {
    let live = mid_run_discriminator(Runtime::new(), "arena/region-count");
    assert!(
        live > 150,
        "gauge-live: the self-referential accumulator retains every prior, so region \
         growth over 200 iterations must be ~200 — got {live}; if small the gauge is \
         dead and every assertion below is vacuous",
    );

    // The emit seed: `go` is yielded to the resumer, then tail-called. The fiber body
    // loops forever, so one `(fiber/resume f nil)` runs exactly one `step` — the yield
    // parks it, and the next resume runs the recursion and reaches the next yield.
    let emitted = mid_run_growth(
        Runtime::new(),
        "(def step (fn [k] \
            (letrec [go (fn [m] (if (< m 1) 0 (go (- m 1))))] \
              (yield go) \
              (go k)))) \
         (def f (fiber/new (fn [] (forever (step 3))) |:yield|))",
        "(fiber/resume f nil)",
        "arena/region-count",
    );
    assert!(
        emitted < 50,
        "a YIELDED cell-free self-recursive closure must still be reclaimed per call — \
         the emit's park retain is the resumer's reference and the deferred release \
         drops only the frame's: region growth over 200 resumes must be ~0, got {emitted}",
    );

    // The `def` binder face of the same crossing: a self-recursive `def` reaches the
    // strand by the other route (its would-be-live release is suppressed rather than
    // left as dead code), so the crossing must be sound through both binders.
    let emitted_define = mid_run_growth(
        Runtime::new(),
        "(def step (fn [k] \
            (def go (fn [m] (if (< m 1) 0 (go (- m 1))))) \
            (yield go) \
            (go k))) \
         (def f (fiber/new (fn [] (forever (step 3))) |:yield|))",
        "(fiber/resume f nil)",
        "arena/region-count",
    );
    assert!(
        emitted_define < 50,
        "a YIELDED cell-free self-recursive `def` closure must be reclaimed per call \
         through the other binder route: region growth over 200 resumes must be ~0, \
         got {emitted_define}",
    );

    // The `Sends` seed: `go` is sent over a channel, then tail-called. The receive
    // pulls it back in the same op and discards it, so the send-site incref is
    // released before the next op and only the frame's reference is in question.
    let sent = mid_run_growth(
        Runtime::new(),
        "(def [snd rcv] (chan)) \
         (def step (fn [k] \
            (letrec [go (fn [m] (if (< m 1) 0 (go (- m 1))))] \
              (chan/send snd go) \
              (go k)))) \
         (def once (fn [] (step 3) (get (chan/recv rcv) 1)))",
        "(once)",
        "arena/region-count",
    );
    assert!(
        sent < 50,
        "a SENT cell-free self-recursive closure must be reclaimed per call — the send \
         site counts the message until it is received, so the deferral drops only the \
         frame's reference: region growth over 200 calls must be ~0, got {sent}",
    );
}

/// The strand's premise is a tail call to the binding, not a body that IS one
/// (docs/impl/selfrec.md; `lower_letrec`). A letrec body reaches
/// its tail call through statements — `(begin (stmt …) (go k))` — and ANF names that
/// call's result, so the body is a `Begin` whose last element binds the call and
/// returns the binding. Read as "the whole body is a tail call" that shape is not one,
/// the binding is never stranded, and its scope-end `DecrefRegion` sits past the
/// frame-replacing `TailCall` with no channel to supply it.
///
/// The ordinary demise reading covers the shape only while nothing else claims the
/// closure: as soon as it escapes — here by the RETURN facet, the recursion handing
/// itself back — the release no longer lands at the call node, and the strand is the
/// only channel left. Both subjects below discard the returned closure, so the whole
/// per-call region must come back.
///
/// Counterfactual: ~200 — one region per call, closure and env together — if the
/// marking demands a wholly-tail-call body.
#[test]
fn statement_bodied_letrec_strands_its_self_recursive_member() {
    let live = mid_run_discriminator(Runtime::new(), "arena/region-count");
    assert!(
        live > 150,
        "gauge-live: the self-referential accumulator retains every prior, so region \
         growth over 200 iterations must be ~200 — got {live}; if small the gauge is \
         dead and every assertion below is vacuous",
    );

    // A statement, then the tail call. `go` returns itself, so the return facet keeps
    // the demise off the call node and the strand is the only channel.
    let statement = mid_run_growth(
        Runtime::new(),
        "(def step (fn [k] \
            (letrec [go (fn [m] (if (< m 1) go (go (- m 1))))] \
              (+ k 1) \
              (go k))))",
        "(step 3)",
        "arena/region-count",
    );
    assert!(
        statement < 50,
        "a letrec whose body reaches its tail call through a statement must strand its \
         cell-free self-recursive member: region growth over 200 calls must be ~0, got \
         {statement}",
    );

    // The same through a BRANCH, where only one arm leaves through the tail call. The
    // other arm keeps the live scope-end release, so the two channels must stay
    // mutually exclusive per path rather than both firing.
    let branched = mid_run_growth(
        Runtime::new(),
        "(def step (fn [k] \
            (letrec [go (fn [m] (if (< m 1) go (go (- m 1))))] \
              (if (< k 0) go (go k)))))",
        "(step 3)",
        "arena/region-count",
    );
    assert!(
        branched < 50,
        "a letrec body only one arm of which tail-calls the member must reclaim per \
         call — the arm that falls through runs the live scope-end release and records \
         no deferral: region growth over 200 calls must be ~0, got {branched}",
    );
}

mod cycle;
mod define;
mod tailreturn;
