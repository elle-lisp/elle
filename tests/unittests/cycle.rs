// audited: 2026-10-22
// Display, Debug, PartialEq, Hash and Ord terminate on cyclic mutable values.
//
// docs/impl/values.md
//
// Each traversal must detect the cycle rather than recurse into a stack
// overflow. A funnel takes the heap its container lives on, so every test
// grows its values through the heap of the `TestHeap` that built them.

use elle::primitives::ctx::TestHeap;
use elle::value::{TableKey, Value};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// `@[elements...]` with the array pushed into itself, and the heap it lives
/// on, which must outlive it.
fn self_referencing_array(elements: &[Value]) -> (TestHeap, Value) {
    let h = TestHeap::new();
    let arr = h.ctx().array_mut(elements.to_vec());
    elle::value::arena::push_with_incref(h.heap(), arr, arr);
    (h, arr)
}

// =========================================================================
// Display
// =========================================================================

#[test]
fn display_self_referencing_array() {
    let (_h, a) = self_referencing_array(&[]);
    let s = format!("{}", a);
    assert!(s.contains("<cycle>"), "expected <cycle>, got: {}", s);
}

#[test]
fn display_self_referencing_array_with_elements() {
    let (_h, a) = self_referencing_array(&[Value::int(1), Value::int(2)]);
    let s = format!("{}", a);
    assert!(s.contains("1"), "got: {}", s);
    assert!(s.contains("2"), "got: {}", s);
    assert!(s.contains("<cycle>"), "got: {}", s);
}

#[test]
fn display_mutual_cycle_arrays() {
    let h = TestHeap::new();
    let heap = h.heap();
    let a = h.ctx().array_mut(vec![]);
    let b = h.ctx().array_mut(vec![]);
    elle::value::arena::push_with_incref(heap, a, b);
    elle::value::arena::push_with_incref(heap, b, a);
    let s = format!("{}", a);
    assert!(s.contains("<cycle>"), "expected <cycle>, got: {}", s);
}

#[test]
fn display_self_referencing_struct() {
    let h = TestHeap::new();
    let t = h.ctx().struct_mut();
    elle::value::arena::struct_put_with_rebind(h.heap(), t, TableKey::keyword("self"), t);
    let s = format!("{}", t);
    assert!(s.contains("<cycle>"), "expected <cycle>, got: {}", s);
}

#[test]
fn display_self_referencing_lbox() {
    let h = TestHeap::new();
    let b = h.ctx().lbox(Value::NIL);
    elle::value::arena::lbox_store_with_rebind(h.heap(), b, b);
    let s = format!("{}", b);
    assert!(s.contains("<cycle>"), "expected <cycle>, got: {}", s);
}

// =========================================================================
// Debug
// =========================================================================

#[test]
fn debug_self_referencing_array() {
    let (_h, a) = self_referencing_array(&[Value::int(42)]);
    let s = format!("{:?}", a);
    assert!(s.contains("42"), "got: {}", s);
    assert!(s.contains("<cycle>"), "got: {}", s);
}

#[test]
fn debug_self_referencing_struct() {
    let h = TestHeap::new();
    let t = h.ctx().struct_mut();
    elle::value::arena::struct_put_with_rebind(h.heap(), t, TableKey::keyword("self"), t);
    let s = format!("{:?}", t);
    assert!(s.contains("<cycle>"), "expected <cycle>, got: {}", s);
}

// =========================================================================
// PartialEq
// =========================================================================

#[test]
fn eq_self_referencing_array_identity() {
    let (_h, a) = self_referencing_array(&[]);
    // Same object: pointer-identity fast path
    assert_eq!(a, a);
}

#[test]
fn eq_mutual_cycle_arrays() {
    // a = @[b], b = @[a]  — structurally identical cycles
    let h = TestHeap::new();
    let heap = h.heap();
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
    let h = TestHeap::new();
    let heap = h.heap();
    let a = h.ctx().array_mut(vec![Value::int(1)]);
    let b = h.ctx().array_mut(vec![Value::int(2)]);
    elle::value::arena::push_with_incref(heap, a, b);
    elle::value::arena::push_with_incref(heap, b, a);
    // Must not crash. Elements differ, so not equal.
    assert_ne!(a, b);
}

#[test]
fn eq_self_referencing_lbox() {
    let h = TestHeap::new();
    let b = h.ctx().lbox(Value::NIL);
    elle::value::arena::lbox_store_with_rebind(h.heap(), b, b);
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
    let (_h, a) = self_referencing_array(&[]);
    // Must not crash
    let _ = compute_hash(&a);
}

#[test]
fn hash_mutual_cycle_arrays() {
    let h = TestHeap::new();
    let heap = h.heap();
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
    let h = TestHeap::new();
    let b = h.ctx().lbox(Value::NIL);
    elle::value::arena::lbox_store_with_rebind(h.heap(), b, b);
    let _ = compute_hash(&b);
}

// =========================================================================
// Ord
// =========================================================================

#[test]
fn ord_self_referencing_array() {
    let (_h, a) = self_referencing_array(&[]);
    // Same object: pointer-identity fast path → Equal
    assert_eq!(a.cmp(&a), std::cmp::Ordering::Equal);
}

#[test]
fn ord_mutual_cycle_arrays() {
    let h = TestHeap::new();
    let heap = h.heap();
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
    let h = TestHeap::new();
    let inner = h.ctx().array_mut(vec![Value::int(1), Value::int(2)]);
    let outer = h.ctx().array_mut(vec![inner, Value::int(3)]);
    let s = format!("{}", outer);
    assert_eq!(s, "@[@[1 2] 3]");
}

#[test]
fn eq_non_cyclic_nested_arrays() {
    let h = TestHeap::new();
    let a = h.ctx().array_mut(vec![Value::int(1)]);
    let b = h.ctx().array_mut(vec![Value::int(1)]);
    assert_eq!(a, b);
    let c = h.ctx().array_mut(vec![Value::int(2)]);
    assert_ne!(a, c);
}

#[test]
fn hash_equal_values_same_hash() {
    let h = TestHeap::new();
    let a = h.ctx().array_mut(vec![Value::int(1), Value::int(2)]);
    let b = h.ctx().array_mut(vec![Value::int(1), Value::int(2)]);
    assert_eq!(compute_hash(&a), compute_hash(&b));
}

// =========================================================================
// Deep nesting (not cyclic) should not trigger false positives
// =========================================================================

#[test]
fn display_deeply_nested_arrays() {
    let h = TestHeap::new();
    let mut v = Value::int(0);
    for _ in 0..100 {
        v = h.ctx().array_mut(vec![v]);
    }
    let s = format!("{}", v);
    assert!(!s.contains("<cycle>"), "false positive: {}", s);
}
