// audited: 2026-10-06
//! A tiered WASM module and a tiered WASM rejection each pin the code region their key's payload lives in.
//!
//! docs/impl/wasm.md
//!
//! The tier keys both by the raw address of a code object's bytecode. Without
//! the pin, a dropped unit frees its code region, a later unit lands another
//! function's bytecode at the same address, and the tier runs the old
//! function's module on it, or refuses it for good.

use super::*;
use crate::pipeline::compile_file;

/// Run `source` on `rt`, and answer the program's value. The unit is dropped
/// before this returns.
fn run(rt: &mut Runtime, source: &str) -> crate::value::Value {
    let (vm, symbols, cctx) = rt.parts();
    let unit = compile_file(source, symbols, cctx, "<wasm>").expect("the source compiles");
    vm.execute_scheduled(&unit, cctx).expect("the source runs")
}

/// The id and generation of the code region `f`'s payload lives in.
fn code_region(rt: &mut Runtime, f: crate::value::Value) -> (u32, u32) {
    let closure = f.as_closure().expect("the program answers the closure");
    let id = rt.heap().region_of_ptr(closure.template.payload_backing());
    (id, rt.heap().region_generation(id))
}

/// A module the tier compiled pins its code region until `clear_code_pins`.
///
/// The VM holds the tier before the run, so `compile/run-on :wasm` leaves the
/// module in it rather than in a tier of its own that it drops. The second
/// half is the counter-factual for the first: with the module gone, nothing
/// else holds the region, so the generation really would have moved.
#[test]
fn a_tiered_wasm_module_pins_its_code_region() {
    let mut rt = Runtime::new();
    rt.vm().wasm_tier = Some(crate::wasm::lazy::WasmTier::new().expect("a tier starts"));
    let f = run(
        &mut rt,
        "(defn wasm-add [a b] (silence) (muffle :error) (numeric!) (%add a b)) \
         (assert (= (compile/run-on :wasm wasm-add 3 4) 7)) \
         wasm-add",
    );
    let (id, generation) = code_region(&mut rt, f);

    crate::value::arena::release_program_value(rt.heap(), f);
    assert_eq!(
        rt.heap().region_generation(id),
        generation,
        "the closure and its unit are gone, but the module keyed by its \
         bytecode still pins the code region"
    );

    rt.vm().clear_code_pins();
    assert_ne!(
        rt.heap().region_generation(id),
        generation,
        "with the module gone nothing holds the code region, so it frees"
    );
}

/// A closure the tier refused is keyed the same way, so it pins the same way.
///
/// The tier refuses a tail call, and `(+ a 1)` in tail position is one. The
/// tier is on from the first call, so the program's own call meets it.
#[test]
fn a_tiered_wasm_rejection_pins_its_code_region() {
    let mut rt = Runtime::new();
    rt.vm().runtime_config.wasm = crate::config::WasmPolicy::Lazy { threshold: 0 };
    rt.vm().wasm_tier = Some(crate::wasm::lazy::WasmTier::new().expect("a tier starts"));
    let f = run(&mut rt, "(defn refused [a] (+ a 1)) (refused 1) refused");
    let key = f
        .as_closure()
        .expect("the program answers the closure")
        .template
        .bytecode()
        .as_ptr();
    assert!(
        rt.vm().wasm_rejections.contains_key(&key),
        "the tier refused the closure's tail call"
    );
    let (id, generation) = code_region(&mut rt, f);

    crate::value::arena::release_program_value(rt.heap(), f);
    assert_eq!(
        rt.heap().region_generation(id),
        generation,
        "the closure and its unit are gone, but the rejection keyed by its \
         bytecode still pins the code region"
    );

    rt.vm().clear_code_pins();
    assert_ne!(
        rt.heap().region_generation(id),
        generation,
        "with the rejection gone nothing holds the code region, so it frees"
    );
}
