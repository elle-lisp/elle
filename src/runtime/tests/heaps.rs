// audited: 2026-10-06
//! Which heap compiles a unit and which heap runs it, on every path a compiled unit reaches a VM.
//!
//! docs/impl/region/template.md
//!
//! A code payload is region data, so it belongs to one heap. These pins record
//! where the compile heap (`CompileCtx::heap_ptr`) and the executing heap
//! (`VM::heap_ptr`) are one heap and where they are two, and that a closure's
//! payload lives on the heap that runs it either way.

use super::*;
use crate::pipeline::{compile_file, CompileCtx};
use crate::symbol::SymbolTable;
use crate::value::fiberheap::FiberHeap;
use crate::value::Value;
use crate::vm::VM;

/// The source every pin compiles: a file whose value is a closure.
const LAMBDA: &str = "(fn [x] x)";

/// Whether the payload of the closure `v` lies in a region of `heap`.
fn payload_on(heap: &FiberHeap, v: Value) -> bool {
    let closure = v.as_closure().expect("the program's value is a closure");
    heap.value_in_region_store(Value::from_heap_ptr(
        closure.template.payload_backing(),
        crate::value::repr::TAG_ARRAY,
    ))
}

/// A bare VM and a compile context of its own, the way the embedding API
/// builds them (src/lib.rs).
fn standalone() -> (VM, SymbolTable, CompileCtx) {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    crate::register_primitives(&mut vm, &mut symbols);
    (vm, symbols, CompileCtx::new())
}

#[test]
fn an_instance_compiles_on_the_heap_its_program_runs_on() {
    // The macro VM and the program VM share the instance's heap, so whatever
    // the compile builds on its heap is already on the executing one.
    let mut rt = Runtime::without_stdlib();
    let value = {
        let (vm, symbols, cctx) = rt.parts();
        assert_eq!(
            cctx.heap_ptr(),
            vm.heap_ptr,
            "an instance's compile heap is its program heap"
        );
        let unit = compile_file(LAMBDA, symbols, cctx, "<heaps>").expect("compiles");
        vm.execute(&unit).expect("runs")
    };
    assert!(payload_on(rt.heap(), value));
    crate::value::arena::release_program_value(rt.heap(), value);
}

#[test]
fn a_standalone_compile_context_compiles_on_a_heap_of_its_own() {
    // Counter-factual: a payload built on the compile heap and run as it
    // stands. The closure answers every call, and its payload is a region the
    // executing heap does not own, so no edge the executing heap records keeps
    // it alive and no teardown of that heap releases it.
    let (mut vm, mut symbols, mut cctx) = standalone();
    assert_ne!(
        cctx.heap_ptr(),
        vm.heap_ptr,
        "a standalone compile context runs its macro VM on a heap of its own"
    );
    let unit = compile_file(LAMBDA, &mut symbols, &mut cctx, "<heaps>").expect("compiles");
    let value = vm.execute(&unit).expect("runs");
    assert!(
        payload_on(vm.heap(), value),
        "the closure's payload lives on the heap that runs it"
    );
    assert!(
        !payload_on(unsafe { &*cctx.heap_ptr() }, value),
        "the closure's payload does not live on the compile heap"
    );
}

#[test]
fn eval_runs_on_the_callers_heap_whatever_heap_compiled_it() {
    // `eval` expands on the caller's VM and mints its syntax arena on the
    // compile context's heap; the two differ in the standalone shape.
    let (mut vm, mut symbols, mut cctx) = standalone();
    let value = crate::pipeline::eval(LAMBDA, &mut symbols, &mut vm, &mut cctx, "<heaps>")
        .expect("evaluates");
    assert!(payload_on(vm.heap(), value));
    assert!(!payload_on(unsafe { &*cctx.heap_ptr() }, value));
}

#[test]
fn the_scheduled_entry_runs_on_the_instance_heap() {
    // `execute_scheduled` wraps the unit in an entry thunk on the executing
    // VM, so the thunk and every closure the unit builds land on its heap.
    let mut rt = Runtime::new();
    let value = {
        let (vm, symbols, cctx) = rt.parts();
        let unit = compile_file(LAMBDA, symbols, cctx, "<heaps>").expect("compiles");
        vm.execute_scheduled(&unit, cctx).expect("runs")
    };
    assert!(payload_on(rt.heap(), value));
    crate::value::arena::release_program_value(rt.heap(), value);
}

#[test]
fn a_stdlib_cache_hit_rebuilds_the_library_on_the_loading_heap() {
    // The cache hit deserializes on the VM that runs the library, not on the
    // compile context's macro VM.
    let dir = tempfile::tempdir().expect("a scratch directory");
    let cache = crate::compiler::stdlib_cache::StdlibCache::Dir(dir.path().to_path_buf());
    drop(Runtime::with_stdlib_cache(cache.clone()));
    let mut rt = Runtime::with_stdlib_cache(cache);
    assert_eq!(
        rt.stdlib_source(),
        crate::primitives::module_init::StdlibSource::Cache,
        "the second instance must load the first one's cache"
    );
    let map = rt
        .compile()
        .lookup_stdlib_value(crate::value::SymbolId::of("map"))
        .expect("the stdlib exports map");
    assert!(payload_on(rt.heap(), map));
}

#[test]
fn a_received_closure_is_rebuilt_on_the_receiving_heap() {
    // `send` carries a closure to another instance's heap, which rebuilds its
    // code there.
    let mut sender = Runtime::without_stdlib();
    let value = {
        let (vm, symbols, cctx) = sender.parts();
        let unit = compile_file(LAMBDA, symbols, cctx, "<heaps>").expect("compiles");
        vm.execute(&unit).expect("runs")
    };
    let bundle =
        crate::value::send::SendBundle::from_value(value, sender.heap(), None).expect("sendable");
    crate::value::arena::release_program_value(sender.heap(), value);

    let mut receiver = Runtime::without_stdlib();
    let received = {
        let mut alloc = crate::primitives::ctx::Alloc::new(receiver.heap());
        bundle.into_value(&mut alloc, None)
    };
    assert!(payload_on(receiver.heap(), received));
    assert!(!payload_on(sender.heap(), received));
}
