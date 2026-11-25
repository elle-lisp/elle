// audited: 2026-11-25
//! A warm `[a b]` or `@[a b]` allocates nothing on the Rust heap, from its region's mint to its free.
//!
//! docs/regions/performance.md

use super::{prim_array, prim_tuple};
use crate::primitives::ctx::NativeCtx;
use crate::value::fiber::SignalBits;
use crate::value::Value;
use crate::vm::VM;

/// A primitive the tests call the way the VM's native dispatch does.
type Native = fn(&mut NativeCtx<'_>, &[Value]) -> (SignalBits, Value);

// The counter-factual is a constructor that copies its arguments into a Vec
// before it copies them into the region, and a region whose entry the store
// builds afresh: each costs allocations on every call, so a warm call reads
// several, not zero.

/// Call `prim` with a string from another region and an int, as `[kept 1]`
/// does, into a region minted for the call, then free that region. Do it once
/// to warm the store, then again, and answer how many allocations the second
/// call made.
fn call_allocations(prim: Native) -> u64 {
    let mut vm = VM::new();
    let vm_ptr: *mut VM = &mut vm;
    let heap_ptr = vm.heap_ptr;
    // SAFETY: the heap outlives the VM's use here, and every reborrow below
    // ends before the next one is taken.
    let kept = {
        let heap = unsafe { &mut *heap_ptr };
        let region = heap.new_runtime_region();
        crate::value::build::string(heap, "x", region)
    };
    let call = || {
        let region = unsafe { &mut *heap_ptr }.new_runtime_region();
        {
            let mut ctx = NativeCtx::with_region_vm(region, unsafe { &mut *heap_ptr }, vm_ptr);
            let (bits, _) = prim(&mut ctx, &[kept, Value::int(1)]);
            assert!(bits.is_empty(), "the constructor succeeds");
        }
        unsafe { &mut *heap_ptr }.decref_region_if_present(region);
    };
    call();
    let ((), made) = crate::testalloc::allocations(call);
    made
}

#[test]
fn a_tuple_allocates_nothing() {
    assert_eq!(
        call_allocations(prim_tuple),
        0,
        "[kept 1] copies its arguments into its region and frees it"
    );
}

#[test]
fn an_array_allocates_nothing() {
    assert_eq!(
        call_allocations(prim_array),
        0,
        "@[kept 1] copies its arguments into its region and frees it"
    );
}
