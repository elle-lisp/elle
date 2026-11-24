// audited: 2026-11-24
//! Two closure-cycle arenas one tail call strands are reclaimed per call, on the interpreter and with the caller compiled.
//!
//! docs/impl/region/cycletail.md

use super::*;

/// Per-CALL reclamation where ONE tail call strands TWO merged arenas, for two
/// cycles in one scope and for two in nested scopes. `(f 3)` takes the arm whose
/// tail call to `g` replaces the frame, so each arena rides that call's deferred
/// release. `(f 0)` takes the other arm, where both scope-exit drops run.
///
/// The trap: a call that carries one arena leaves the other's release to the
/// frame-exit hoist, which replicates it into the arms by value, and a debug
/// build refuses that compile. Refusing the second cycle instead leaves it
/// Shared, and it leaks two regions per call. The graft stays off, because a
/// grafted `f` folds its branch on the constant argument and the tail call with
/// it (docs/impl/graft.md).
#[test]
fn region_ownership_reclaims_two_cycles_behind_one_tail_call() {
    use crate::hir::testkit::{TWO_CYCLES_NESTED, TWO_CYCLES_ONE_SCOPE};
    let live_chain = mid_run_discriminator(Runtime::new(), "arena/region-count");
    assert!(
        live_chain > 150,
        "precondition: the live accumulator retains every prior, so region growth \
         over 200 iterations must be large (~200), and it is {live_chain}. If \
         small, the gauge is dead and the assertions below are vacuous",
    );
    for (shape, prelude) in [
        ("one scope", TWO_CYCLES_ONE_SCOPE),
        ("nested scopes", TWO_CYCLES_NESTED),
    ] {
        for call in ["(f 3)", "(f 0)"] {
            let mut rt = Runtime::new();
            rt.compile().set_graft(crate::config::GraftPolicy::Off);
            let growth = mid_run_growth(rt, prelude, call, "arena/region-count");
            assert!(
                growth < 50,
                "{shape}, {call}: both arenas one tail call strands must be reclaimed \
                 per call. Region growth over 200 calls is {growth} and must be near \
                 zero; the discriminator grows {live_chain}",
            );
        }
    }
}

/// The same two arenas with the function that makes the call COMPILED. The JIT
/// hands the call's arena slots to its helper in a stack slot, and the helper
/// hands each arena on to the activation that runs the callee
/// (docs/impl/region/relocate.md).
///
/// The counter-factual: a helper that read only the first slot leaves the
/// second arena with no release on the closure path, and its cycle's two
/// closures and two forward cells strand per call.
#[cfg(feature = "jit")]
#[test]
fn region_ownership_two_cycles_behind_one_tail_call_under_jit() {
    use crate::hir::testkit::{TWO_CYCLES_NESTED, TWO_CYCLES_ONE_SCOPE};
    let live_chain = mid_run_discriminator(Runtime::new(), "arena/region-count");
    assert!(
        live_chain > 150,
        "precondition: the live accumulator retains every prior, so region growth \
         over 200 iterations must be large (~200) — got {live_chain}; if small, the \
         gauge is dead and the assertion below is vacuous",
    );
    for (shape, source) in [
        ("one scope", TWO_CYCLES_ONE_SCOPE),
        ("nested scopes", TWO_CYCLES_NESTED),
    ] {
        let prelude = format!("{source} (def caller f)");
        let (growth, compiled) =
            super::jit::jit_mid_run_growth(&prelude, "(caller 3)", "arena/region-count");
        assert!(
            compiled,
            "{shape}: `f` must run compiled for this to read the JIT tail-call path",
        );
        assert!(
            growth < 50,
            "{shape}: both arenas the compiled tail call strands must be reclaimed per \
             call — region growth over 200 calls is {growth} (the discriminator grows \
             {live_chain})",
        );
    }
}
