// audited: 2026-10-07
//! A rejected function's compile-attempt count goes with its rejection, and the pin that rejection holds.
//!
//! docs/impl/jit.md

use super::*;
use crate::pipeline::compile_file;

/// The counter-factual: a count kept beside the rejections, in a map keyed by
/// bytecode address with no pin of its own. It survives the rejection and its
/// pin, so a later function that lands at the freed address starts with the
/// old function's attempts, and `:attempts` reads 2 for a function compiled
/// once.
#[test]
fn a_compile_attempt_count_goes_with_its_rejection() {
    let mut rt = Runtime::new();
    rt.vm().runtime_config.jit = crate::config::JitPolicy::Eager;
    let source = "(defn returns-closure [x] (fn [] x)) \
                  (returns-closure 1) (jit/rejections) (returns-closure 2) \
                  returns-closure";
    let f = {
        let (vm, symbols, cctx) = rt.parts();
        let unit = compile_file(source, symbols, cctx, "<attempts>").expect("the source compiles");
        vm.execute_scheduled(&unit, cctx).expect("the source runs")
    };
    let key = f
        .as_closure()
        .expect("the program answers the closure")
        .template
        .bytecode()
        .as_ptr();
    assert_eq!(
        rt.vm().jit_attempts(key),
        1,
        "the JIT refused MakeClosure once, and the negative cache stopped a second try"
    );

    rt.vm().clear_code_pins();
    assert_eq!(
        rt.vm().jit_attempts(key),
        0,
        "the rejection and its pin are gone, so no count names the address"
    );
    crate::value::arena::release_program_value(rt.heap(), f);
}
