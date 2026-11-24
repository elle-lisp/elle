// audited: 2026-11-23
//! A compiled call runs its callee's cached code without taking a count on it.
//!
//! docs/impl/jit.md

use super::*;
use std::sync::Arc;

/// Code that answers how many counted references its own cache entry's code
/// has while it runs.
extern "C" fn answers_its_count(
    _env: *const Value,
    _args: *const Value,
    _nargs: u32,
    vm: *mut (),
    self_tag: u64,
    self_payload: u64,
) -> JitValue {
    let vm = unsafe { &*(vm as *const VM) };
    let me = Value {
        tag: self_tag,
        payload: self_payload,
    };
    let key = me
        .as_closure()
        .expect("the callee is a closure")
        .template
        .bytecode()
        .as_ptr();
    let count = vm
        .jit_cache
        .get(&key)
        .map_or(0, |entry| Arc::strong_count(&entry.code));
    JitValue::from_value(Value::int(count as i64))
}

/// A compiled call borrows its callee's code from the cache, which holds it
/// until teardown (docs/impl/jit.md).
///
/// The counter-factual is a call that clones the entry's `Arc` to keep the
/// code alive while it runs: the code then reads 2, and every compiled call
/// pays a locked increment and decrement on one counter.
///
/// The trap: `elle_jit_call` interprets the callee while less than its native
/// reserve remains, and this callee has no bytecode to interpret. The test
/// runs on a thread whose stack it sizes, so the harness's choice of stack
/// cannot send it down that arm.
#[test]
fn a_compiled_call_takes_no_count_on_its_code() {
    let answer = std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(|| {
            crate::value::arena::with_test_region(|| {
                let mut vm = make_vm();
                let vm_ptr = &mut vm as *mut VM;
                let heap = unsafe { &mut *vm.heap_ptr };
                let template = nullary(heap);
                vm.install_jit_code(
                    (*template).clone(),
                    Arc::new(crate::jit::JitCode::test_with_entry(answers_its_count)),
                );
                let region = heap.new_runtime_region();
                let closure = crate::value::Closure::new(
                    template,
                    crate::value::region_slice::RegionSlice::empty(),
                    SignalBits::EMPTY,
                );
                let f = crate::primitives::ctx::NativeCtx::with_region_vm(region, heap, vm_ptr)
                    .closure(closure);
                let got = elle_jit_call(
                    f.tag,
                    f.payload,
                    std::ptr::null(),
                    0,
                    vm_ptr as *mut (),
                    1,
                    std::ptr::null_mut(),
                );
                assert!(vm.fiber.signal.is_none(), "the call raised nothing");
                got.to_value().as_int()
            })
        })
        .expect("the test thread starts")
        .join()
        .expect("the test thread finishes");
    assert_eq!(
        answer,
        Some(1),
        "the cache entry holds the only counted reference to the running code"
    );
}
