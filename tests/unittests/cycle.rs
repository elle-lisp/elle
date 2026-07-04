// Tests for cycle detection in Display, Debug, PartialEq, Hash, and Ord.
//
// These verify that cyclic mutable structures don't crash the process:
// traversal must terminate rather than recurse infinitely into a stack overflow.

use elle::value::fiberheap::FiberHeap;
use elle::value::{TableKey, Value};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Helper: create @[elements...], then push the array into itself.
///
/// The arena funnels take an explicit heap. The `TestHeap` ctx builds the array
/// on the persistent thread-root heap, which is the same heap a `Runtime`'s
/// `heap()` exposes, so the caller can thread `rt.heap()` into the funnel here.
fn self_referencing_array(heap: &mut FiberHeap, elements: &[Value]) -> Value {
    let h = elle::primitives::ctx::TestHeap::new();
    let arr = h.ctx().array_mut(elements.to_vec());
    elle::value::arena::push_with_incref(heap, arr, arr);
    arr
}

// =========================================================================
// Display
// =========================================================================

#[test]
fn display_self_referencing_array() {
    let mut rt = elle::runtime::Runtime::without_stdlib();
    let a = self_referencing_array(rt.heap(), &[]);
    let s = format!("{}", a);
    assert!(s.contains("<cycle>"), "expected <cycle>, got: {}", s);
}

#[test]
fn display_self_referencing_array_with_elements() {
    let mut rt = elle::runtime::Runtime::without_stdlib();
    let a = self_referencing_array(rt.heap(), &[Value::int(1), Value::int(2)]);
    let s = format!("{}", a);
    assert!(s.contains("1"), "got: {}", s);
    assert!(s.contains("2"), "got: {}", s);
    assert!(s.contains("<cycle>"), "got: {}", s);
}

#[test]
fn display_mutual_cycle_arrays() {
    let mut rt = elle::runtime::Runtime::without_stdlib();
    let h = elle::primitives::ctx::TestHeap::new();
    let heap = rt.heap();
    let a = h.ctx().array_mut(vec![]);
    let b = h.ctx().array_mut(vec![]);
    elle::value::arena::push_with_incref(heap, a, b);
    elle::value::arena::push_with_incref(heap, b, a);
    let s = format!("{}", a);
    assert!(s.contains("<cycle>"), "expected <cycle>, got: {}", s);
}

#[test]
fn display_self_referencing_struct() {
    let mut rt = elle::runtime::Runtime::without_stdlib();
    let h = elle::primitives::ctx::TestHeap::new();
    let t = h.ctx().struct_mut();
    elle::value::arena::struct_put_with_rebind(rt.heap(), t, TableKey::Keyword("self".to_string()), t);
    let s = format!("{}", t);
    assert!(s.contains("<cycle>"), "expected <cycle>, got: {}", s);
}

#[test]
fn display_self_referencing_lbox() {
    let mut rt = elle::runtime::Runtime::without_stdlib();
    let h = elle::primitives::ctx::TestHeap::new();
    let b = h.ctx().lbox(Value::NIL);
    elle::value::arena::lbox_store_with_rebind(rt.heap(), b, b);
    let s = format!("{}", b);
    assert!(s.contains("<cycle>"), "expected <cycle>, got: {}", s);
}

// =========================================================================
// Debug
// =========================================================================

#[test]
fn debug_self_referencing_array() {
    let mut rt = elle::runtime::Runtime::without_stdlib();
    let a = self_referencing_array(rt.heap(), &[Value::int(42)]);
    let s = format!("{:?}", a);
    assert!(s.contains("42"), "got: {}", s);
    assert!(s.contains("<cycle>"), "got: {}", s);
}

#[test]
fn debug_self_referencing_struct() {
    let mut rt = elle::runtime::Runtime::without_stdlib();
    let h = elle::primitives::ctx::TestHeap::new();
    let t = h.ctx().struct_mut();
    elle::value::arena::struct_put_with_rebind(rt.heap(), t, TableKey::Keyword("self".to_string()), t);
    let s = format!("{:?}", t);
    assert!(s.contains("<cycle>"), "expected <cycle>, got: {}", s);
}

// =========================================================================
// PartialEq
// =========================================================================

#[test]
fn eq_self_referencing_array_identity() {
    let mut rt = elle::runtime::Runtime::without_stdlib();
    let a = self_referencing_array(rt.heap(), &[]);
    // Same object: pointer-identity fast path
    assert_eq!(a, a);
}

#[test]
fn eq_mutual_cycle_arrays() {
    // a = @[b], b = @[a]  — structurally identical cycles
    let mut rt = elle::runtime::Runtime::without_stdlib();
    let h = elle::primitives::ctx::TestHeap::new();
    let heap = rt.heap();
    let a = h.ctx().array_mut(vec![]);
    let b = h.ctx().array_mut(vec![]);
    elle::value::arena::push_with_incref(heap, a, b);
    elle::value::arena::push_with_incref(heap, b, a);
    // Must not crash. cycle detection returns true (assume equal).
    assert_eq!(a, b);
}

#[test]
fn eq_asymmetric_cycle_arrays() {
    // a = @[1 b], b = @[2 a] — structurally different
    let mut rt = elle::runtime::Runtime::without_stdlib();
    let h = elle::primitives::ctx::TestHeap::new();
    let heap = rt.heap();
    let a = h.ctx().array_mut(vec![Value::int(1)]);
    let b = h.ctx().array_mut(vec![Value::int(2)]);
    elle::value::arena::push_with_incref(heap, a, b);
    elle::value::arena::push_with_incref(heap, b, a);
    // Must not crash. Elements differ, so not equal.
    assert_ne!(a, b);
}

#[test]
fn eq_self_referencing_lbox() {
    let mut rt = elle::runtime::Runtime::without_stdlib();
    let h = elle::primitives::ctx::TestHeap::new();
    let b = h.ctx().lbox(Value::NIL);
    elle::value::arena::lbox_store_with_rebind(rt.heap(), b, b);
    assert_eq!(b, b);
}

// =========================================================================
// Hash
// =========================================================================

fn compute_hash(v: &Value) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

#[test]
fn hash_self_referencing_array() {
    let mut rt = elle::runtime::Runtime::without_stdlib();
    let a = self_referencing_array(rt.heap(), &[]);
    // Must not crash
    let _ = compute_hash(&a);
}

#[test]
fn hash_mutual_cycle_arrays() {
    let mut rt = elle::runtime::Runtime::without_stdlib();
    let h = elle::primitives::ctx::TestHeap::new();
    let heap = rt.heap();
    let a = h.ctx().array_mut(vec![]);
    let b = h.ctx().array_mut(vec![]);
    elle::value::arena::push_with_incref(heap, a, b);
    elle::value::arena::push_with_incref(heap, b, a);
    // Must not crash
    let _ = compute_hash(&a);
    let _ = compute_hash(&b);
}

#[test]
fn hash_self_referencing_lbox() {
    let mut rt = elle::runtime::Runtime::without_stdlib();
    let h = elle::primitives::ctx::TestHeap::new();
    let b = h.ctx().lbox(Value::NIL);
    elle::value::arena::lbox_store_with_rebind(rt.heap(), b, b);
    let _ = compute_hash(&b);
}

// =========================================================================
// Ord
// =========================================================================

#[test]
fn ord_self_referencing_array() {
    let mut rt = elle::runtime::Runtime::without_stdlib();
    let a = self_referencing_array(rt.heap(), &[]);
    // Same object: pointer-identity fast path → Equal
    assert_eq!(a.cmp(&a), std::cmp::Ordering::Equal);
}

#[test]
fn ord_mutual_cycle_arrays() {
    let mut rt = elle::runtime::Runtime::without_stdlib();
    let h = elle::primitives::ctx::TestHeap::new();
    let heap = rt.heap();
    let a = h.ctx().array_mut(vec![]);
    let b = h.ctx().array_mut(vec![]);
    elle::value::arena::push_with_incref(heap, a, b);
    elle::value::arena::push_with_incref(heap, b, a);
    // Must not crash. Cycle detected → Equal.
    let _ = a.cmp(&b);
}

// =========================================================================
// Non-cyclic structures still work correctly
// =========================================================================

#[test]
fn display_non_cyclic_nested_arrays() {
    let h = elle::primitives::ctx::TestHeap::new();
    let inner = h.ctx().array_mut(vec![Value::int(1), Value::int(2)]);
    let outer = h.ctx().array_mut(vec![inner, Value::int(3)]);
    let s = format!("{}", outer);
    assert_eq!(s, "@[@[1 2] 3]");
}

#[test]
fn eq_non_cyclic_nested_arrays() {
    let h = elle::primitives::ctx::TestHeap::new();
    let a = h.ctx().array_mut(vec![Value::int(1)]);
    let b = h.ctx().array_mut(vec![Value::int(1)]);
    assert_eq!(a, b);
    let c = h.ctx().array_mut(vec![Value::int(2)]);
    assert_ne!(a, c);
}

#[test]
fn hash_equal_values_same_hash() {
    let h = elle::primitives::ctx::TestHeap::new();
    let a = h.ctx().array_mut(vec![Value::int(1), Value::int(2)]);
    let b = h.ctx().array_mut(vec![Value::int(1), Value::int(2)]);
    assert_eq!(compute_hash(&a), compute_hash(&b));
}

// =========================================================================
// Deep nesting (not cyclic) should not trigger false positives
// =========================================================================

#[test]
fn display_deeply_nested_arrays() {
    let h = elle::primitives::ctx::TestHeap::new();
    let mut v = Value::int(0);
    for _ in 0..100 {
        v = h.ctx().array_mut(vec![v]);
    }
    let s = format!("{}", v);
    assert!(!s.contains("<cycle>"), "false positive: {}", s);
}
