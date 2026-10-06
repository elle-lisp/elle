// audited: 2026-10-06
//! The SPIR-V `(git f)` compiles is cached on the VM under the closure's bytecode, and the entry pins its code region.
//!
//! docs/impl/jit.md

use super::*;
use crate::pipeline::compile_file;

/// A GPU-eligible function, GIT'd, as the program's value.
const GIT: &str = "(defn gpu-add [a b] (silence) (muffle :error) (numeric!) (%add a b)) \
                   (git gpu-add)";

/// Run `GIT` on `rt` and answer the closure it returns. The unit is dropped
/// before this returns.
fn gitted(rt: &mut Runtime) -> crate::value::Value {
    let (vm, symbols, cctx) = rt.parts();
    let unit = compile_file(GIT, symbols, cctx, "<spirv>").expect("the source compiles");
    vm.execute_scheduled(&unit, cctx).expect("the source runs")
}

/// `(git f)` leaves its SPIR-V in the VM's cache, keyed by `f`'s bytecode,
/// where any header over `f`'s payload finds it.
///
/// The counter-factual is a cache on the header's blueprint: a header with no
/// blueprint caches nothing, so every `(git f)` over such a header compiles
/// again and `fn/git?` answers false after a `git` that succeeded.
#[test]
fn git_leaves_its_spirv_in_the_vms_cache() {
    let mut rt = Runtime::new();
    let f = gitted(&mut rt);
    let closure = f.as_closure().expect("git answers the closure").clone();
    let first = rt
        .vm()
        .spirv_for(&closure.template)
        .expect("the VM caches the SPIR-V git compiled")
        .to_vec();
    assert!(!first.is_empty());
    crate::value::arena::release_program_value(rt.heap(), f);
}

/// A SPIR-V cache entry is keyed by bytecode address like a JIT cache entry,
/// so it pins the code region its key's payload lives in.
///
/// The second half is the counter-factual for the first: with the entry gone,
/// nothing else holds the region, so the generation the first half compared
/// against really would have moved.
#[test]
fn a_spirv_cache_entry_pins_its_code_region() {
    let mut rt = Runtime::new();
    let f = gitted(&mut rt);
    let closure = f.as_closure().expect("git answers the closure").clone();
    let id = rt.heap().region_of_ptr(closure.template.payload_backing());
    let generation = rt.heap().region_generation(id);

    crate::value::arena::release_program_value(rt.heap(), f);
    assert_eq!(
        rt.heap().region_generation(id),
        generation,
        "the closure and its unit are gone, but the SPIR-V cache entry still \
         pins the code region its key names"
    );

    rt.vm().spirv_cache.clear();
    assert_ne!(
        rt.heap().region_generation(id),
        generation,
        "with the entry gone nothing holds the code region, so it frees"
    );
}
