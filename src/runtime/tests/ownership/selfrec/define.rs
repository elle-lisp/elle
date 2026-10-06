// audited: 2026-10-06
//! A self-recursive `def` in a lambda releases its closure region once per call, by its body's route.
//!
//! docs/impl/selfrec.md

use super::*;

/// A cell-free self-recursive `def` nested in a lambda, exercised through ACTUAL per-call
/// recursion with heap-allocating arithmetic (`<`/`-`, whose stdlib bodies churn regions),
/// under the FULL stdlib — the universal shape of a module-level `(defn …)` that recurses
/// (every `lib/*.lisp` helper). This is the strong companion to the boolean
/// `…_no_double_free` test below: the boolean shape never allocates inside the recursion, so
/// a prematurely-freed closure region's page is not recycled before the recursion ends — the
/// use-after-free reads stale-but-intact memory and stays silent. With heap-churning
/// arithmetic that freed page is recycled mid-recursion, so the self-call re-dispatch (which
/// re-enters the executing closure living in that region) reads a foreign object and trips
/// the `tag/object mismatch` panic (src/value/arena.rs).
///
/// A self-recursive `def`'s closure region demises at the binding's last use, which for
/// this body is the `(loop k)` tail call — dead code past the frame replacement. So
/// `lower_define` STRANDS the binding (`stranded_self_bindings`) and the tail-call
/// deferred release is the sole, once-only release, reproducing the `letrec` path's
/// accounting. The gauge (region growth over 200 calls) additionally pins the region is
/// reclaimed per call — a leak would grow it unbounded.
///
/// Counterfactual: without the stranding, the dead-block release frees the closure region
/// before the `(loop k)` tail call re-enters it, and the run panics with `tag/object
/// mismatch`.
#[test]
fn self_recursive_define_with_arith_reclaims_per_call() {
    // Subject: a self-recursive `def` (not `letrec`) nested in a lambda, recursing with
    // heap-allocating stdlib `<`/`-` so a freed `R_cell` page is recycled mid-recursion.
    // A crash inside the run panics here; a leak grows the delta.
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

/// The OTHER `def` rows of the placement table (docs/impl/selfrec.md): a body that does
/// not tail-call the binding.
///
/// A `def` has no scope NODE, so the analysis leaves its closure region's demise at the
/// binding's last use — and a use of the binding as a CALLEE resolves through `last_use`
/// to the node that CONSUMES it, the call. The release is therefore emitted where that
/// call has returned and the recursion has completed, which is the ordinary live
/// `DecrefRegion` and needs no strand, no relocation and no deferral. Only when the
/// consuming call is itself the frame-replacing tail call (`…_with_arith_…` above) is
/// the point dead and the deferral its channel.
///
/// Three bodies, one per way the recursion's result can be consumed short of tail-calling
/// the binding: as a tail call's ARGUMENT, under a non-tail consumer, and discarded as a
/// statement. Run under the FULL stdlib with heap-allocating `<`/`-` so a prematurely
/// freed closure page is recycled mid-recursion and the latent use-after-free panics
/// (`tag/object mismatch`) instead of reading stale-but-intact memory.
///
/// Counterfactual: each reads ~200 — one stranded region per call — if the closure
/// region's ordinary release is withheld; ~0 with it standing.
#[test]
fn self_recursive_define_off_tail_reclaims_per_call() {
    let growth = |body: &str| {
        mid_run_growth(
            Runtime::new(),
            &format!(
                "(def sub1 (fn [x] (- x 1))) \
                 (def f (fn [k] \
                    (def loop (fn [m] (if (< m 1) 0 (loop (- m 1))))) \
                    {body}))"
            ),
            "(f 3)",
            "arena/region-count",
        )
    };
    let live = mid_run_discriminator(Runtime::new(), "arena/region-count");
    assert!(
        live > 150,
        "gauge-live: the self-referential accumulator retains every prior, so region \
         growth over 200 iterations must be ~200 — got {live}; if small the gauge is \
         dead and every assertion below is vacuous",
    );

    for (label, body) in [
        ("a tail call's argument", "(sub1 (loop k))"),
        ("a non-tail consumer", "(+ (loop k) 0)"),
        ("a discarded statement", "(begin (loop k) 0)"),
    ] {
        let g = growth(body);
        assert!(
            g < 50,
            "a cell-free self-recursive `def` whose body consumes the recursion through \
             {label} must be reclaimed per call by its ordinary release: region growth \
             over 200 calls must be ~0, got {g}",
        );
    }
}

/// The binder rule underneath the `def` rows above (docs/impl/region/mechanism.md): an
/// UNUSED `def` whose initializer allocates must still release it.
///
/// This is the `def` face of tests/impl/region-unused-let-binding.lisp. The last-use
/// narrowing pulls an unread binding's init demise back to where the value was made —
/// correct for every binder whose value is its BODY, and wrong for the one binder whose
/// value IS the initializer, because the lowerer then emits the release before
/// `lower_define` has stored the value and it reloads the binder's stamped `nil`.
/// Nothing is freed and the initializer's region is held to fiber teardown.
///
/// `Runtime::without_stdlib` keeps the reading to the shape under test, and the gauge is
/// the OBJECT count: the strand is one cons cell per call, in a region the pool hands
/// straight back out, so `arena/region-count` reads it as flat. Counterfactual: ~200 if the
/// demise sits on the initializer rather than on the `def`.
#[test]
fn unused_define_init_reclaims_per_call() {
    let live = mid_run_discriminator(Runtime::without_stdlib(), "arena/count");
    assert!(
        live > 150,
        "gauge-live: the self-referential accumulator retains every prior, so object \
         growth over 200 iterations must be ~200 — got {live}; if small the gauge is \
         dead and every assertion below is vacuous",
    );
    let unused = mid_run_growth(
        Runtime::without_stdlib(),
        "(def f (fn [k] (def x (list k k)) 0))",
        "(f 3)",
        "arena/count",
    );
    assert!(
        unused < 50,
        "the heap initializer of a `def` nothing reads must be released: object growth \
         over 200 calls must be ~0, got {unused} — narrowed onto the initializer the \
         release is emitted before the binder's slot store and reloads the stamped `nil`",
    );
}

/// A self-recursive `def` nested in a lambda is cell-free (docs/impl/selfrec.md), handled
/// exactly like a self-recursive `letrec`: no forward cell, the self-reference resolves to
/// the executing closure. `lower_define` STRANDS the binding (`stranded_self_bindings`) so
/// the tail-call deferred release is the sole release — the closure region must be freed
/// EXACTLY once. Both the dead-block decref and the deferral firing is a double-free,
/// which panics with `phantom region or double-free`
/// (src/value/fiberheap/regionstore/refcount.rs). This pins that the program runs to
/// completion.
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
    let (vm, _symbols, cctx) = rt.parts();
    let v = vm
        .execute_scheduled(&res, cctx)
        .expect("a cell-free self-recursive `def` must not double-free its closure region");
    assert!(
        v.is_keyword(),
        "the recursive `def` returns the :done keyword, got {v:?}"
    );
}

/// A self-recursive `def` that is ALSO captured by a sibling is NOT cell-free: the
/// sibling capture makes it `needs_capture`, so it keeps a forward cell whose cascade
/// owns the closure region's single release (docs/impl/selfrec.md).
/// `lower_define`/`lower_letrec` therefore must NOT strand it — stranding a cell-held
/// binding makes the tail-call deferred release decref its region a SECOND time, under
/// the still-live cell. This is the runtime peer to
/// tests/impl/region-selfrec-captured-tail-release.lisp: `loop` self-recurses AND is
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
    let (vm, _symbols, cctx) = rt.parts();
    let v = vm
        .execute_scheduled(&res, cctx)
        .expect("a sibling-captured self-recursive `def` must not double-free its closure region");
    assert!(
        v.is_keyword(),
        "the sibling-captured recursive `def` returns the :done keyword, got {v:?}"
    );
}
