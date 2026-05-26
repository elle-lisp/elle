//! Tests for FiberHeap.

use super::*;
use crate::value::heap::{HeapObject, Pair};

#[test]
fn test_fiber_heap_alloc_in_region() {
    let mut heap = FiberHeap::new();
    let v = heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)), 2);
    assert_eq!(heap.len(), 1);
    assert!(v.is_heap());
}

#[test]
fn test_fiber_heap_clear() {
    let mut heap = FiberHeap::new();
    heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)), 2);
    heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)), 2);
    heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)), 3);
    assert_eq!(heap.len(), 3);
    heap.clear();
    assert_eq!(heap.len(), 0);
    assert!(heap.is_empty());
}

#[test]
fn test_fiber_heap_needs_drop_exhaustive() {
    assert!(!needs_drop(HeapTag::Pair));
    assert!(!needs_drop(HeapTag::Float));
    assert!(!needs_drop(HeapTag::NativeFn));
    assert!(!needs_drop(HeapTag::LibHandle));
    assert!(!needs_drop(HeapTag::ManagedPointer));
    assert!(!needs_drop(HeapTag::Parameter));

    assert!(needs_drop(HeapTag::LBox));
    assert!(needs_drop(HeapTag::CaptureCell));
    assert!(needs_drop(HeapTag::LString));
    assert!(needs_drop(HeapTag::LArrayMut));
    assert!(needs_drop(HeapTag::LStructMut));
    assert!(needs_drop(HeapTag::LStruct));
    assert!(needs_drop(HeapTag::Closure));
    assert!(needs_drop(HeapTag::LArray));
    assert!(needs_drop(HeapTag::LStringMut));
    assert!(needs_drop(HeapTag::LBytes));
    assert!(needs_drop(HeapTag::LBytesMut));
    assert!(needs_drop(HeapTag::Syntax));
    assert!(needs_drop(HeapTag::Fiber));
    assert!(needs_drop(HeapTag::ThreadHandle));
    assert!(needs_drop(HeapTag::FFISignature));
    assert!(needs_drop(HeapTag::FFIType));
    assert!(needs_drop(HeapTag::External));
    assert!(needs_drop(HeapTag::LSet));
    assert!(needs_drop(HeapTag::LSetMut));
}

#[test]
fn test_install_and_uninstall() {
    let mut heap = Box::new(FiberHeap::new());
    let ptr = &mut *heap as *mut FiberHeap;
    unsafe {
        install_fiber_heap(ptr);
    }
    assert!(is_fiber_heap_installed());
    assert!(with_current_heap_mut(|h| h.len()).is_some());
    uninstall_fiber_heap();
    assert!(!is_fiber_heap_installed());
}

#[test]
fn test_no_heap_by_default() {
    uninstall_fiber_heap();
    assert!(!is_fiber_heap_installed());
    assert!(with_current_heap_mut(|h| h.len()).is_none());
}

#[test]
fn test_ensure_root_heap_idempotent() {
    let p1 = ensure_root_heap();
    let p2 = ensure_root_heap();
    let p3 = ensure_root_heap();
    assert!(!p1.is_null());
    assert_eq!(p1, p2);
    assert_eq!(p2, p3);
}

#[test]
fn test_vm_new_installs_root_heap() {
    use crate::vm::core::VM;
    let _vm = VM::new();
    assert!(is_fiber_heap_installed());
    uninstall_fiber_heap();
}

#[test]
fn test_alloc_without_installed_heap_lazy_inits() {
    crate::value::arena::with_test_region(|| {
        uninstall_fiber_heap();
        let v = Value::string("lazy-test");
        assert!(v.is_heap());
        assert!(is_fiber_heap_installed());
        uninstall_fiber_heap();
    });
}

#[test]
fn free_region_physical_frees_matching_slots() {
    let mut heap = FiberHeap::new();
    heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)), 1);
    let v2 = heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::int(2), Value::NIL)), 2);
    assert!(v2.is_heap());
    heap.free_region_physical(1);
    heap.free_region_physical(2);
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "free_region called with region_id 0")]
fn free_region_zero_panics() {
    let mut heap = FiberHeap::new();
    let ptr = &mut heap as *mut FiberHeap;
    unsafe { install_fiber_heap(ptr) };
    free_region(0);
}
