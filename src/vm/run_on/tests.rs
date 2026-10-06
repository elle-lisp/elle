// audited: 2026-10-06
//! `compile/run-on` names a tier the build does not compile in as `:feature-disabled`, on every build.
//!
//! docs/impl/differential.md
//!
//! The runner probes a build's tiers with a trivial closure and drops every
//! tier that answers `:feature-disabled` (docs/test-runner.md). A tier that
//! answers `:ineligible` instead stays in the run. The counter-factual is a
//! build without the `jit` feature whose stub answers `:ineligible`: the
//! runner then keeps a JIT tier the build lacks, sets `vm/config-set :jit
//! :eager` for every whole-file script, and fails each one.

use crate::pipeline::compile_file_repl;
use crate::runtime::Runtime;

/// What `(compile/run-on TIER (fn [] 1))` answers: 0 when it runs the closure,
/// 1 when it rejects the tier as `:feature-disabled`, and 2 for any other
/// refusal.
fn probe(tier: &str) -> i64 {
    let src = format!(
        "(let [[ok? e] (protect (compile/run-on :{tier} (fn [] 1)))] \
           (if ok? 0 (if (= (get e :reason) :feature-disabled) 1 2)))"
    );
    let mut rt = Runtime::new();
    let result = {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file_repl(&src, symbols, cctx, "<probe>")
            .expect("compiles")
            .0
    };
    let (vm, _symbols, cctx) = rt.parts();
    vm.execute_scheduled(&result, cctx)
        .expect("runs")
        .as_int()
        .expect("the probe answers an int")
}

#[test]
fn a_tier_the_build_lacks_answers_feature_disabled() {
    for (tier, carried) in [
        ("jit", cfg!(feature = "jit")),
        ("wasm", cfg!(feature = "wasm")),
        ("mlir-cpu", cfg!(feature = "mlir")),
    ] {
        let answer = probe(tier);
        if carried {
            assert_ne!(
                answer, 1,
                ":{tier} is compiled in, and compile/run-on calls it feature-disabled"
            );
        } else {
            assert_eq!(
                answer, 1,
                ":{tier} is not compiled in, and compile/run-on answers {answer} \
                 (0 ran it, 2 refused it as something else)"
            );
        }
    }
}
