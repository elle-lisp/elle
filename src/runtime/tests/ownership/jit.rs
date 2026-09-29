// audited: 2026-09-29
//! VM≡JIT parity for the ownership forest: what each cut still reclaims when the
//! function that owes the release runs compiled.
//!
//! docs/impl/region/owner.md
//! docs/impl/region/relocate.md

use super::*;

/// VM≡JIT parity for the `AdoptRegion`/`FreeRegionGroup` ownership ops: the SAME
/// never-mergeable shapes the VM tests in subtree.rs pin, but the reclaiming
/// function runs through the JIT. So the `elle_jit_adopt_region` /
/// `elle_jit_free_region_group` helpers and their translate arms
/// (src/jit/translate/instr/regionops.rs) carry the ops, mirroring the
/// interpreter handlers.
///
/// `body` is wrapped in an immediately-invoked lambda `((fn [] body))` whose body
/// carries the never-mergeable owned shape. A single compile is re-run. The first
/// run submits the lambda for background JIT compilation (eager → hot on its
/// first call), and `drain_jit_pending` blocks until that compile finishes. So
/// the steady-state measurement dispatches the lambda through cached native code,
/// not the interpreter.
///
/// The lambda is inline, not a `def`-bound `f`: a fresh REPL compile renumbers
/// global slots, so a separate `(f)` compile would mis-resolve `f` (see
/// `self_recursive_loop_reclaims_per_call_no_stdlib`). Re-running the same
/// program keeps the closure-template bytecode pointer stable, so hotness
/// accumulates onto the cached compile.
///
/// Returns `(per_run_region_delta, jit_compiled)`. `jit_compiled` guards against a
/// vacuous reading. Without a translate arm, the lambda's `AdoptRegion` or
/// `FreeRegionGroup` hits `unreachable!` in the background worker, which dies
/// before it can cache anything. Then `jit_cache` stays empty and `jit_compiled`
/// is false, even though the interpreter fallback still reclaims.
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
        let (vm, _symbols, cctx) = rt.parts();
        let v = vm
            .execute_scheduled(&prog.bytecode, cctx)
            .expect("runs (submits the JIT task)");
        assert!(v.is_nil(), "the discarded-shape lambda returns nil");
    }
    rt.vm().drain_jit_pending();
    let jit_compiled = !rt.vm().jit_cache.is_empty();

    // Warmup (the lambda body now dispatches to cached native code), then measure.
    {
        let (vm, _symbols, cctx) = rt.parts();
        let v = vm.execute_scheduled(&prog.bytecode, cctx).expect("runs");
        assert!(v.is_nil());
    }
    let baseline = rt.heap().active_region_count() as i64;
    for _ in 0..50 {
        let (vm, _symbols, cctx) = rt.parts();
        let v = vm.execute_scheduled(&prog.bytecode, cctx).expect("runs");
        assert!(v.is_nil());
    }
    let delta = rt.heap().active_region_count() as i64 - baseline;
    (delta, jit_compiled)
}

/// [`mid_run_growth`] with the driving lambda COMPILED, plus the receipt that says
/// it was.
///
/// The program is compiled once and run twice. The first run calls `body` 250
/// times, which submits every closure it reaches for background compilation;
/// `drain_jit_pending` then blocks until those finish, so the second run — the one
/// whose mid-run delta is returned — dispatches through cached native code. Re-running
/// the same bytecode is what keeps the closure templates' pointers stable, so the
/// cache keyed by them still hits after the `def`s rebuild their closures.
///
/// The program's last form must be `[delta caller]`: the gauge delta, and the very
/// closure whose compiled body the reading is about. Handing that closure back is
/// what makes the receipt exact — `jit_code_for` on its own bytecode answers whether
/// THIS function ran compiled, where an "is the cache non-empty" test would pass on
/// any stdlib closure the program happened to warm.
#[cfg(feature = "jit")]
fn jit_mid_run_growth(prelude: &str, body: &str, gauge: &str) -> (i64, bool) {
    use crate::config::JitPolicy;
    use crate::pipeline::compile_file_repl;
    let mut rt = Runtime::new();
    rt.vm().runtime_config.jit = JitPolicy::Eager;
    let src = format!(
        "{prelude} (var n 0) \
         (while (%lt n 50) {body} (assign n (%add n 1))) \
         (def c50 ({gauge})) \
         (while (%lt n 250) {body} (assign n (%add n 1))) \
         (def c250 ({gauge})) \
         [(match (type-of c250) :integer \
            (match (type-of c50) :integer (%sub c250 c50) _ -1) _ -1) caller]"
    );
    let prog = {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file_repl(&src, symbols, cctx, "<embed>")
            .expect("compiles")
            .0
    };
    {
        let (vm, _symbols, cctx) = rt.parts();
        vm.execute_scheduled(&prog.bytecode, cctx)
            .expect("runs (submits the JIT tasks)");
    }
    rt.vm().drain_jit_pending();
    let (vm, _symbols, cctx) = rt.parts();
    let pair = vm.execute_scheduled(&prog.bytecode, cctx).expect("runs");
    let (delta, caller_bytecode) = {
        let slots = pair.as_array().expect("the program returns [delta caller]");
        (
            slots[0].as_int().expect("the delta is an integer"),
            slots[1]
                .as_closure()
                .expect("the program hands back the driving closure")
                .template
                .bytecode(),
        )
    };
    let compiled = vm.jit_code_for(caller_bytecode.as_ptr()).is_some();
    (delta, compiled)
}

/// Per-call reclamation of the closure-as-module factory when a member is called
/// back through the struct it handed out, with the CALLING function compiled.
///
/// The factory itself carries a `MakeClosure` and so never compiles; the caller
/// does, and its tail call into the member is where the tiers can disagree. That
/// call strands the merged cycle arena's release, and the compiled tier has to hand
/// the deferral to the activation that runs the member, its own having popped its
/// region-remap frame at the tail-call sentinel (docs/impl/region/relocate.md).
///
/// The counter-factual: with the hand-off missing, the caller's activation records
/// nothing, no clean break ever runs the arena's decref, and each call strands the
/// cycle's two closures, its two forward cells and the table they capture — two
/// regions per call, which is a growth of ~400 over the measured window. The VM
/// reading of the same driver is
/// `selfrec::region_ownership_reclaims_defn_module_factory_per_call`, so a
/// regression that reads bounded there and open here is exactly the tier gap.
#[cfg(feature = "jit")]
#[test]
fn region_ownership_defn_module_member_call_under_jit() {
    let prelude = "(def mk (fn [] \
        (let [t @{}] \
          (defn fa [m] (when (%not (%int? m)) (error :m)) \
                       (if (%lt m 1) t (fb (%sub m 1)))) \
          (defn fb [m] (when (%not (%int? m)) (error :m)) \
                       (put t m true) (fa (%sub m 1))) \
          (let [s {:a fa :b fb}] s)))) \
        (def caller (fn [] ((get (mk) :a) 2)))";

    let live_chain_growth = mid_run_discriminator(Runtime::new(), "arena/region-count");
    assert!(
        live_chain_growth > 150,
        "precondition: the live accumulator retains every prior, so region growth \
         over 200 iterations must be large (~200) — got {live_chain_growth}; if \
         small, the gauge is dead and the assertion below is vacuous",
    );

    let (used, jit_compiled) = jit_mid_run_growth(prelude, "(caller)", "arena/region-count");
    assert!(
        jit_compiled,
        "the calling lambda must run compiled for this to read the JIT tail-call \
         path — its bytecode is absent from the jit cache, so the reading below is \
         the interpreter's and proves nothing about the tier",
    );
    assert!(
        used < 50,
        "calling a member back through the returned struct must reclaim the merged \
         cycle arena per call on the compiled tier too — region growth over 200 \
         calls is {used} (the discriminator grows {live_chain_growth})",
    );
}

/// VM≡JIT parity for the **flat store-adopt**. A discarded mutable container
/// `(@array)` takes an immutable value `(array 1 2)`. Both are call-result regions
/// no slot can name, so MERGE cannot collapse them and the cut emits
/// `AdoptRegion(container, value)`. Run through the JIT, it must reclaim the
/// container+value subtree each call (bounded growth) and be panic-clean. A broken
/// `elle_jit_adopt_region` or subtree drop would free the value early or twice,
/// tripping a debug generation/decref assert. This is a SOUNDNESS guard: the
/// immutable value is RC-reclaimable, so no leaking counterfactual exists here.
#[cfg(feature = "jit")]
#[test]
fn region_ownership_adopt_subtree_drop_under_jit() {
    let body = "(begin (%array-push (@array) (array 1 2)) nil)";
    let (on, jit_compiled) = jit_region_growth(body);
    assert!(
        jit_compiled,
        "the lambda must JIT-compile for this to test the JIT adopt path (empty \
         jit_cache means the background worker died, for example on a missing AdoptRegion \
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
/// cannot collect the a↔b cycle (docs/impl/region/rules.md), so the cut adopts
/// `a` and `b` by `root`, whose JIT subtree drop reclaims the cycle. The leak
/// discriminator proves the gauge is live, and `jit_compiled` proves the JIT path
/// ran: without the translate arm, the lambda cannot JIT-compile.
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
/// other (`a ⊇ b`, `b ⊇ a`) with NO container parent, so no member owns another,
/// and one `FreeRegionGroup` reclaims them. Per-region RC cannot collect the
/// cycle, so the JIT `elle_jit_free_region_group` helper frees it wholesale. The
/// leak discriminator and the `jit_compiled` guard work as above.
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
