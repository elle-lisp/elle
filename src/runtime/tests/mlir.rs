// audited: 2026-10-06
//! An MLIR engine and an MLIR rejection each pin the code region their key's payload lives in.
//!
//! docs/impl/mlir.md
//!
//! The MLIR tier keys both tables by the raw address of a code object's
//! bytecode. Without the pin, a dropped unit frees its code region, a later
//! unit lands another function's bytecode at the same address, and the tier
//! runs the old function's engine on the new function's arguments.

use super::*;
use crate::pipeline::compile_file;

/// An MLIR-CPU eligible function, defined.
const ADD: &str = "(defn mlir-add [a b] (silence) (muffle :error) (numeric!) (%add a b))";

/// Run `ADD` and then `body` on `rt`, and answer the program's value. The
/// unit is dropped before this returns.
fn run(rt: &mut Runtime, body: &str) -> crate::value::Value {
    let source = format!("{ADD} {body}");
    let (vm, symbols, cctx) = rt.parts();
    let unit = compile_file(&source, symbols, cctx, "<mlir>").expect("the source compiles");
    vm.execute_scheduled(&unit, cctx).expect("the source runs")
}

/// The id and generation of the code region `f`'s payload lives in.
fn code_region(rt: &mut Runtime, f: crate::value::Value) -> (u32, u32) {
    let closure = f.as_closure().expect("the program answers the closure");
    let id = rt.heap().region_of_ptr(closure.template.payload_backing());
    (id, rt.heap().region_generation(id))
}

/// A compiled engine pins its code region until `clear_code_pins` drops it.
///
/// The second half is the counter-factual for the first: with the entry gone,
/// nothing else holds the region, so the generation the first half compared
/// against really would have moved. It also proves `clear_code_pins` reaches
/// the MLIR tables, which teardown relies on.
#[test]
fn an_mlir_engine_pins_its_code_region() {
    let mut rt = Runtime::new();
    let f = run(
        &mut rt,
        "(assert (= (compile/run-on :mlir-cpu mlir-add 3 4) 7)) mlir-add",
    );
    let (id, generation) = code_region(&mut rt, f);

    crate::value::arena::release_program_value(rt.heap(), f);
    assert_eq!(
        rt.heap().region_generation(id),
        generation,
        "the closure and its unit are gone, but the engine keyed by its \
         bytecode still pins the code region"
    );

    rt.vm().clear_code_pins();
    assert_ne!(
        rt.heap().region_generation(id),
        generation,
        "with the engine gone nothing holds the code region, so it frees"
    );
}

/// A rejection is keyed the same way, so it pins the same way: a rejected
/// address that another function reuses would refuse that function forever.
#[test]
fn an_mlir_rejection_pins_its_code_region() {
    let mut rt = Runtime::new();
    let f = run(&mut rt, "mlir-add");
    let (id, generation) = code_region(&mut rt, f);
    let template = f
        .as_closure()
        .expect("the program answers the closure")
        .template;
    let pin = crate::value::CodePin::of(rt.heap(), &template);
    rt.vm()
        .mlir_cache
        .get_or_insert_with(crate::mlir::MlirCache::new)
        .reject(pin, crate::mlir::MlirSig::default());

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
