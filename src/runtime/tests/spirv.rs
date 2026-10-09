// audited: 2026-10-06
//! The SPIR-V `git` and `mlir/compile-spirv` compile is cached on the VM under the closure's bytecode, a kernel per workgroup size.
//!
//! docs/impl/spirv.md

use super::*;
use crate::pipeline::compile_file;
use crate::vm::core::WorkgroupSize;

/// A GPU-eligible function, defined.
const GPU_ADD: &str = "(defn gpu-add [a b] (silence) (muffle :error) (numeric!) (%add a b))";

/// Run `GPU_ADD` and then `body` on `rt`, and answer the program's value. The
/// unit is dropped before this returns.
fn run(rt: &mut Runtime, body: &str) -> crate::value::Value {
    let source = format!("{GPU_ADD} {body}");
    let (vm, symbols, cctx) = rt.parts();
    let unit = compile_file(&source, symbols, cctx, "<spirv>").expect("the source compiles");
    vm.execute_scheduled(&unit, cctx).expect("the source runs")
}

/// A size other than the default.
fn size_64() -> WorkgroupSize {
    WorkgroupSize::new(64).expect("64 is a workgroup size")
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
    let f = run(&mut rt, "(git gpu-add)");
    let closure = f.as_closure().expect("git answers the closure").clone();
    let first = rt
        .vm()
        .spirv_for(&closure.template, WorkgroupSize::DEFAULT)
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
    let f = run(&mut rt, "(git gpu-add)");
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

/// The workgroup size is written into the kernel's entry point, so each size
/// is a kernel of its own.
///
/// The counter-factual is a cache keyed by the closure alone: the second
/// `git` finds the first size's kernel, returns early, and the 64-wide
/// dispatch runs a 256-wide kernel.
#[test]
fn each_workgroup_size_caches_a_kernel_of_its_own() {
    let mut rt = Runtime::new();
    let f = run(&mut rt, "(git gpu-add) (git gpu-add 64) gpu-add");
    let closure = f
        .as_closure()
        .expect("the program answers the closure")
        .clone();
    let wide = rt
        .vm()
        .spirv_for(&closure.template, WorkgroupSize::DEFAULT)
        .expect("the default size is cached")
        .to_vec();
    let narrow = rt
        .vm()
        .spirv_for(&closure.template, size_64())
        .expect("size 64 is cached")
        .to_vec();
    assert!(
        wide != narrow,
        "the two sizes are two kernels, so their bytes differ ({} and {} bytes)",
        wide.len(),
        narrow.len()
    );
    crate::value::arena::release_program_value(rt.heap(), f);
}

/// `fn/git?` and `disgit` answer for the size they are asked about, 256 when
/// none is given.
///
/// The counter-factual is a predicate that ignores the size: after a 64-wide
/// `git` it answers true for the default size, and `gpu:map` then reads a
/// kernel for a size it never compiled.
#[test]
fn fn_git_and_disgit_answer_for_one_size() {
    let mut rt = Runtime::new();
    let answer = run(
        &mut rt,
        "(git gpu-add 64) \
         [(fn/git? gpu-add) (fn/git? gpu-add 64) (bytes? (disgit gpu-add 64)) \
          ((protect (disgit gpu-add)) 0)]",
    );
    let expected = [false, true, true, false];
    let got: Vec<bool> = answer
        .as_array()
        .expect("the program answers an array")
        .iter()
        .map(|v| v.is_truthy())
        .collect();
    assert_eq!(
        got, expected,
        "only size 64 was compiled, so only size 64 answers: {answer}"
    );
    crate::value::arena::release_program_value(rt.heap(), answer);
}

/// `mlir/compile-spirv` fills the VM's pinned cache, as `git` does.
///
/// The counter-factual is a second cache behind the compile, keyed by address
/// and holding no pin: the VM's cache never sees the kernel, and the code
/// region frees under an entry that still names its address.
#[test]
fn compile_spirv_fills_the_pinned_cache() {
    let mut rt = Runtime::new();
    let f = run(&mut rt, "(mlir/compile-spirv gpu-add 64) gpu-add");
    let closure = f
        .as_closure()
        .expect("the program answers the closure")
        .clone();
    assert!(
        rt.vm().spirv_for(&closure.template, size_64()).is_some(),
        "the compile left its kernel in the VM's cache, at its size"
    );
    let id = rt.heap().region_of_ptr(closure.template.payload_backing());
    let generation = rt.heap().region_generation(id);

    crate::value::arena::release_program_value(rt.heap(), f);
    assert_eq!(
        rt.heap().region_generation(id),
        generation,
        "the entry the compile made pins the code region its key names"
    );

    rt.vm().spirv_cache.clear();
    assert_ne!(
        rt.heap().region_generation(id),
        generation,
        "with the entry gone nothing holds the code region, so it frees"
    );
}
