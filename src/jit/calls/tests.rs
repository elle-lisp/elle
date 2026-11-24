// audited: 2026-11-23
//! What a compiled call does with a primitive's signal, a closure whose environment outlived its region, and its callee's code.
//!
//! docs/impl/jit.md

use super::*;
use crate::value::fiber::{SIG_DEBUG, SIG_IO, SIG_OK, SIG_YIELD};
use crate::vm::VM;
use std::sync::Arc;

fn make_vm() -> VM {
    VM::new()
}

// -- jit_handle_primitive_signal: composed signal coverage --

#[test]
fn sig_ok_returns_value() {
    let mut vm = make_vm();
    let result = jit_handle_primitive_signal(&mut vm, SIG_OK, Value::int(42), &[]);
    assert_eq!(result, JitValue::from_value(Value::int(42)));
    assert!(vm.fiber.signal.is_none());
}

#[test]
fn bare_sig_yield_stores_signal_returns_yield_sentinel() {
    let mut vm = make_vm();
    let result = jit_handle_primitive_signal(&mut vm, SIG_YIELD, Value::int(1), &[]);
    assert_eq!(result, YIELD_SENTINEL);
    let (sig, val) = vm.fiber.signal.take().unwrap();
    assert_eq!(sig, SIG_YIELD);
    assert_eq!(val.as_int(), Some(1));
}

#[test]
fn composed_sig_yield_io_stores_signal_returns_yield_sentinel() {
    let mut vm = make_vm();
    let bits = SIG_YIELD | SIG_IO;
    let result = jit_handle_primitive_signal(&mut vm, bits, Value::int(99), &[]);
    assert_eq!(result, YIELD_SENTINEL);
    let (sig, val) = vm.fiber.signal.take().unwrap();
    assert_eq!(sig, bits);
    assert_eq!(val.as_int(), Some(99));
}

#[test]
fn sig_halt_stores_signal_returns_nil() {
    let mut vm = make_vm();
    let result = jit_handle_primitive_signal(&mut vm, SIG_HALT, Value::int(0), &[]);
    assert_eq!(result, JitValue::nil());
    let (sig, _) = vm.fiber.signal.take().unwrap();
    assert_eq!(sig, SIG_HALT);
}

#[test]
fn sig_debug_treated_as_suspension() {
    let mut vm = make_vm();
    vm.fiber.signal = Some((SIG_DEBUG, Value::NIL));
    let result = jit_handle_primitive_signal(&mut vm, SIG_DEBUG, Value::NIL, &[]);
    assert_eq!(result, YIELD_SENTINEL);
    let (sig, _) = vm.fiber.signal.take().unwrap();
    assert_eq!(sig, SIG_DEBUG);
}

#[test]
fn user_defined_signal_treated_as_suspension() {
    let user_bit = SignalBits::from_bit(32);
    let mut vm = make_vm();
    vm.fiber.signal = Some((user_bit, Value::NIL));
    let result = jit_handle_primitive_signal(&mut vm, user_bit, Value::NIL, &[]);
    assert_eq!(result, YIELD_SENTINEL);
    let (sig, _) = vm.fiber.signal.take().unwrap();
    assert_eq!(sig, user_bit);
}

#[test]
fn bare_sig_error_stores_signal_returns_nil() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let mut vm = make_vm();
        let err = h.ctx().string("boom");
        let result = jit_handle_primitive_signal(&mut vm, SIG_ERROR, err, &[]);
        assert_eq!(result, JitValue::nil());
        let (sig, _) = vm.fiber.signal.take().unwrap();
        assert_eq!(sig, SIG_ERROR);
    });
}

#[test]
fn composed_sig_error_io_stores_signal_returns_nil() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let mut vm = make_vm();
        let bits = SIG_ERROR | SIG_IO;
        let result = jit_handle_primitive_signal(&mut vm, bits, h.ctx().string("io-error"), &[]);
        assert_eq!(result, JitValue::nil());
        let (sig, _) = vm.fiber.signal.take().unwrap();
        assert!(sig.intersects(SIG_ERROR));
        assert!(sig.intersects(SIG_IO));
    });
}

#[test]
fn sig_error_terminal_stored_as_error_not_panic() {
    crate::value::arena::with_test_region(|| {
        use crate::value::fiber::SIG_TERMINAL;
        let bits = SIG_ERROR | SIG_TERMINAL;
        let h = crate::primitives::ctx::TestHeap::new();
        let mut vm = make_vm();
        let result = jit_handle_primitive_signal(&mut vm, bits, h.ctx().string("terminal"), &[]);
        assert_eq!(result, JitValue::nil());
        let (sig, _) = vm.fiber.signal.take().unwrap();
        assert!(sig.intersects(SIG_ERROR));
        assert!(sig.intersects(SIG_TERMINAL));
    });
}

// -- jit_handle_primitive_signal: which suspend parks a runtime-built payload --

/// A portless `Sleep` request on the VM's own heap, built the way an io op
/// builds one.
fn sleep_request(vm: &mut VM) -> Value {
    let ctx = crate::primitives::ctx::Alloc::new(unsafe { &mut *vm.heap_ptr });
    crate::io::request::IoRequest::test_sleep(&ctx)
}

/// The compiled mirror of the interpreter's io op park: the request the op
/// built is recorded for the install that displaces it
/// (docs/impl/region/park.md).
#[test]
fn an_io_op_suspend_records_its_request() {
    crate::value::arena::with_test_region(|| {
        let mut vm = make_vm();
        let request = sleep_request(&mut vm);
        let result = jit_handle_primitive_signal(&mut vm, SIG_IO, request, &[Value::int(0)]);
        assert_eq!(result, YIELD_SENTINEL);
        assert_eq!(
            vm.fiber
                .delivery
                .bodyless()
                .map(|p| p.bit_identical(request)),
            Some(true),
            "a compiled io op's request is recorded like an interpreted one's",
        );
    });
}

/// The counter-factual: the `emit` primitive suspending on its own argument is
/// a relay, and its body owns that request. Recording it frees the child's
/// request region under the child.
#[test]
fn a_suspend_on_its_own_argument_records_nothing_to_release() {
    crate::value::arena::with_test_region(|| {
        let mut vm = make_vm();
        let request = sleep_request(&mut vm);
        let args = [Value::keyword("io"), request];
        jit_handle_primitive_signal(&mut vm, SIG_IO, request, &args);
        assert!(vm.fiber.delivery.bodyless().is_none());
    });
}

// ── The env-backing guard at the compiled-call boundary ─────────────
//
// Compiled code reads a closure's env by raw pointer — no region-of
// funnel, no generation check — so a stale env crosses the call boundary
// silently and detonates later at an unattributed load (or, on macOS,
// wedges the thread on the faulting instruction). The guard classifies
// the env backing at the boundary, where the generation check names the
// call site instead. That check is debug-only, so the test that drives it to
// panic is too — the two below run in both profiles because they assert that
// the guard stays quiet, which a release build's no-op also does.

/// An empty nullary code object on `heap`, for a closure whose body never runs.
fn nullary(heap: &mut crate::value::fiberheap::FiberHeap) -> crate::value::TemplateRef {
    crate::value::CodeBuilder::new(Vec::new(), crate::value::Arity::Exact(0), Vec::new())
        .build(heap)
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "stale region")]
fn env_backing_guard_panics_on_a_freed_env_region() {
    crate::value::arena::with_test_region(|| {
        let vm = make_vm();
        let heap = unsafe { &mut *vm.heap_ptr };
        let region = heap.new_runtime_region();
        let env = crate::value::arena::alloc_region_slice_in_region(
            heap,
            &[crate::value::Value::int(42)],
            region,
        );
        let template = nullary(heap);
        let closure = crate::value::Closure::new(template, env, crate::value::SignalBits::EMPTY);
        heap.decref_region(region); // the env's backing region is freed
        debug_check_env_backing(heap, &closure);
    });
}

#[test]
fn env_backing_guard_accepts_a_live_env_region() {
    crate::value::arena::with_test_region(|| {
        let vm = make_vm();
        let heap = unsafe { &mut *vm.heap_ptr };
        let region = heap.new_runtime_region();
        let env = crate::value::arena::alloc_region_slice_in_region(
            heap,
            &[crate::value::Value::int(42)],
            region,
        );
        let template = nullary(heap);
        let closure = crate::value::Closure::new(template, env, crate::value::SignalBits::EMPTY);
        debug_check_env_backing(heap, &closure);
        heap.decref_region(region);
    });
}

#[test]
fn env_backing_guard_ignores_an_empty_env() {
    crate::value::arena::with_test_region(|| {
        let vm = make_vm();
        let heap = unsafe { &mut *vm.heap_ptr };
        let template = nullary(heap);
        let closure = crate::value::Closure::new(
            template,
            crate::value::region_slice::RegionSlice::empty(),
            crate::value::SignalBits::EMPTY,
        );
        debug_check_env_backing(heap, &closure);
    });
}

// ── The callee's code ───────────────────────────────────────────────

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
