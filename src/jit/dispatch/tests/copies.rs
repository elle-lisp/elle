// audited: 2026-09-29
//! The copying helpers allocate into the region the emitter threads to them, and reach the VM through the threaded `JitCtx`.
//!
//! docs/impl/jit.md
//! docs/impl/region/ctx.md

use super::*;
use crate::jit::runtime::{elle_jit_freeze, elle_jit_has, elle_jit_put, elle_jit_thaw};

/// `elle_jit_freeze` allocates its fresh immutable copy into the SLOT region the
/// emitter threads to it, the region the matching `DecrefRegion(slot)` frees.
///
/// The counter-factual: build the source @array in a DISTINCT region `source`,
/// thread a different region `target` as the `region` argument, and assert the
/// result lands in `target`. A helper that ignored its region argument would
/// land the copy in `source` (or a fresh region), and `DecrefRegion(target)`
/// would then free an empty slot.
#[test]
fn freeze_allocates_into_threaded_region() {
    let mut vm = VM::new();
    let heap_ptr = vm.heap_ptr;
    let source = unsafe { (*heap_ptr).new_runtime_region() };
    let target = unsafe { (*heap_ptr).new_runtime_region() };
    assert_ne!(source, target);
    // A mutable @array born in `source`; freeze copies it into `target`.
    let arr = ctx_in(&mut vm, source).array_mut(vec![Value::int(1), Value::int(2)]);
    let mut jit_ctx = crate::jit::JitCtx::new(&mut vm as *mut VM);
    let result =
        elle_jit_freeze(arr.tag, arr.payload, target.get(), &mut jit_ctx as *mut _).to_value();
    assert!(result.is_array(), "freeze yields an immutable array");
    assert_eq!(
        region_of(&vm, result),
        Some(target),
        "elle_jit_freeze must allocate into the threaded SLOT region",
    );
    unsafe {
        (*heap_ptr).decref_region_if_present(source);
        (*heap_ptr).decref_region_if_present(target);
    }
}

/// `elle_jit_thaw` allocates its fresh mutable copy into the threaded SLOT
/// region, by the same counter-factual as the freeze test above.
#[test]
fn thaw_allocates_into_threaded_region() {
    let mut vm = VM::new();
    let heap_ptr = vm.heap_ptr;
    let source = unsafe { (*heap_ptr).new_runtime_region() };
    let target = unsafe { (*heap_ptr).new_runtime_region() };
    assert_ne!(source, target);
    // An immutable array born in `source`; thaw copies it into `target`.
    let arr = ctx_in(&mut vm, source).array(vec![Value::int(1), Value::int(2)]);
    let mut jit_ctx = crate::jit::JitCtx::new(&mut vm as *mut VM);
    let result =
        elle_jit_thaw(arr.tag, arr.payload, target.get(), &mut jit_ctx as *mut _).to_value();
    assert!(result.is_array_mut(), "thaw yields a mutable @array");
    assert_eq!(
        region_of(&vm, result),
        Some(target),
        "elle_jit_thaw must allocate into the threaded SLOT region",
    );
    unsafe {
        (*heap_ptr).decref_region_if_present(source);
        (*heap_ptr).decref_region_if_present(target);
    }
}

/// Each JIT intrinsic fast-path helper resolves its driving VM from the threaded
/// `JitCtx` handed up from compiled code — there is no process-shared VM slot to
/// consult. Covers all three VM-resolution paths — `run_alloc_intrinsic` (`put`),
/// `boundary_vm` (`has`), and `with_region_vm` (`freeze`).
#[test]
fn jit_intrinsics_use_threaded_vm() {
    let mut vm = VM::new();
    let heap_ptr = vm.heap_ptr;
    let region = unsafe { (*heap_ptr).new_runtime_region() };

    // put: an immutable struct gains a key, the fresh copy born via
    // run_alloc_intrinsic off the threaded VM. The source struct/@array are built
    // into `region` (the test's working region) via a NativeCtx over this VM.
    let mut jit_ctx = crate::jit::JitCtx::new(&mut vm as *mut VM);
    let ctx_ptr = &mut jit_ctx as *mut _;
    let empty = ctx_in(&mut vm, region).struct_from_sorted(vec![]);
    let key = Value::keyword("k");
    let one = Value::int(1);
    let put_val = elle_jit_put(
        empty.tag,
        empty.payload,
        key.tag,
        key.payload,
        one.tag,
        one.payload,
        ctx_ptr,
    )
    .to_value();
    assert!(put_val.is_struct(), "put yields an immutable struct");

    // has: queries the just-built struct (boundary_vm path).
    let has = elle_jit_has(put_val.tag, put_val.payload, key.tag, key.payload, ctx_ptr);
    assert_eq!(has, JitValue::bool_val(true), "has? finds the inserted key");

    // freeze: with_region_vm path, copies a mutable @array into `region`.
    let arr = ctx_in(&mut vm, region).array_mut(vec![Value::int(1)]);
    let fr_val = elle_jit_freeze(arr.tag, arr.payload, region.get(), ctx_ptr).to_value();
    assert!(fr_val.is_array(), "freeze yields an immutable array");

    unsafe {
        (*heap_ptr).decref_region_if_present(region);
    }
}
