use super::*;

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
pub(super) fn jit_region_growth(body: &str) -> (i64, bool) {
    use crate::config::JitPolicy;
    use crate::pipeline::compile_file_repl;
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
    let (on, jit_compiled) = jit_region_growth(body);
    assert!(
        jit_compiled,
        "the lambda must JIT-compile for this to test the JIT adopt path (empty \
         jit_cache means the background worker died — e.g. on a missing AdoptRegion \
         translate arm)",
    );
    assert!(
        on <= 0,
        "the Owned container+value subtree must be reclaimed by the JIT subtree \
         drop each call — per-run live-region growth {on} must be <= 0",
    );
}

/// VM≡JIT parity for the **interior-cycle adopt**. A container `root` directly
/// holds `a` and `b`, which reference each other (`a ⊇ b`, `b ⊇ a`). Per-region RC
/// cannot collect the a↔b cycle (region/rules.md Rule 8), so flag-OFF it leaks
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
    let (on, jit_compiled) = jit_region_growth(body);
    assert!(
        jit_compiled,
        "the lambda must JIT-compile — an empty jit_cache means the AdoptRegion \
         translate arm is missing (the worker hit `unreachable!`)",
    );
    let leak = leak_discriminator();
    assert!(
        leak > 0,
        "gauge live: the refused-cycle discriminator must leak (per-run region growth \
         {leak}); if 0 the gauge is dead and the bounded assertion below is vacuous",
    );
    assert!(
        on <= 0,
        "the interior cycle must be reclaimed by the JIT subtree drop — per-run \
         live-region growth {on} must be <= 0 (the discriminator leaks {leak})",
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
    let (on, jit_compiled) = jit_region_growth(body);
    assert!(
        jit_compiled,
        "the lambda must JIT-compile — an empty jit_cache means the FreeRegionGroup \
         translate arm is missing (the worker hit `unreachable!`)",
    );
    let leak = leak_discriminator();
    assert!(
        leak > 0,
        "gauge live: the refused-cycle discriminator must leak (per-run region growth \
         {leak}); if 0 the gauge is dead and the bounded assertion below is vacuous",
    );
    assert!(
        on <= 0,
        "the bare cycle must be reclaimed by the JIT co-owned group free — per-run \
         live-region growth {on} must be <= 0 (the discriminator leaks {leak})",
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
/// RETAINING every closure in a program-lifetime `%pair` CHAIN — each new pair links
/// the last, so every closure stays reachable — then reading object-count growth
/// (`arena/count`) across 200 retained builds, sampled mid-run by the program exactly
/// as `reassign_toplevel_prior_release_is_bounded` samples its gauge. The returned
/// closure escapes via return, so it is the caller that holds it.
///
/// The retain route is load-bearing and must stay a chain rather than a push into a
/// container. A `%pair` records a COUNTED cross-region edge to the closure, so the
/// closure's region stays in the active accounting `arena/count` sums. A container
/// store instead lets the ownership forest ADOPT the stored region as a member of the
/// container's subtree, and an Owned member carries no count — it leaves the sums
/// entirely (retained values, invisible objects). A control retained that way reads
/// as *shrinking*, and the gap below stops measuring cells at all.
///
/// Each retained build therefore pins THREE objects per call — the closure, its
/// one-entry env, and the linking pair — and TWO regions (the closure's and the
/// pair's). What the gap pins is that both shapes pay the same: a per-call forward
/// cell would add a fourth object to the self-recursive shape alone. A fresh-pair
/// retain is also the live-growth discriminator: it must grow ~1 object/call, proving
/// `arena/count` tracks per-call allocation (else every reading is void).
#[test]
fn self_recursive_loop_is_cell_free() {
    let retained_growth = |prelude: &str, body: &str, gauge: &str| {
        mid_run_growth(Runtime::without_stdlib(), prelude, body, gauge)
    };

    // Subject: a self-recursive in-lambda `loop`. Its initializer references only
    // itself, a self-edge that does not mark it captured, so `loop` is cell-free —
    // its self-reference resolves to the executing closure. `(frec false)` recurses
    // to the base case and RETURNS the `loop` closure (escaping), which `@keep` pins.
    let rec_prelude = "(def @keep nil) \
        (def frec (fn [k] (letrec [loop (fn [m] (if m loop (loop true)))] (loop k))))";
    // Cell-free analog of equal capture arity: `h` captures one upvalue (the
    // immediate `k`), not itself — likewise a closure + one-entry env, no cell. With
    // self-recursion also cell-free, the only structural difference is gone.
    let for_prelude = "(def @keep nil) \
        (def ffor (fn [k] (let [h (fn [m] (if m k k))] h)))";
    let rec_body = "(assign keep (%pair (frec false) keep))";
    let for_body = "(assign keep (%pair (ffor false) keep))";

    let rec_obj = retained_growth(rec_prelude, rec_body, "arena/count");
    let for_obj = retained_growth(for_prelude, for_body, "arena/count");
    // Gauge-live discriminator: the chain accumulator retains every prior pair by
    // REFERENCE (each new pair links the last), so the object count must grow ~1
    // per call. A pushed fresh pair cannot serve here — nor can it retain the
    // closures above, for the same reason: the `%array-push` funnel store-adopts
    // the pushed region, and an Owned member leaves the active accounting
    // `arena/count` sums — retained values, invisible objects.
    let pair_obj = mid_run_discriminator(Runtime::without_stdlib(), "arena/count");
    let rec_reg = retained_growth(rec_prelude, rec_body, "arena/region-count");
    let for_reg = retained_growth(for_prelude, for_body, "arena/region-count");

    // If the discriminator reads small, `arena/count` is not tracking per-call
    // allocation and every assertion below is vacuous.
    assert!(
        pair_obj > 150,
        "gauge-live: chaining 200 fresh pairs must grow the object count ~200, \
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

    // The absolute baseline: each retained build pins ~3 objects per call — the
    // closure, its one-entry env, and the pair linking it into the chain (~600/200) —
    // the same as the foreign-capture control. A forward cell would make it ~4/call.
    assert!(
        (450..=750).contains(&rec_obj),
        "cell-free baseline: 200 retained self-recursive `loop` closures pin ~600 \
         objects (3/call: closure + env + the retaining pair, no forward cell), \
         got {rec_obj}",
    );

    // Region growth matches between the shapes too (the closure and its env share one
    // region, and the retaining pair adds one), so a per-call cell would show as an
    // extra REGION as well as an extra object.
    assert!(
        (rec_reg - for_reg).abs() < 50,
        "region growth must match between the self-recursive and foreign-capture \
         shapes (closure + env share one region, plus the retaining pair): \
         self-recursive {rec_reg} vs foreign-capture {for_reg}",
    );
}
