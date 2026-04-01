// Tests for cycle detection in Display, Debug, PartialEq, Hash, and Ord.
//
// These verify that cyclic mutable structures don't crash the process
// (previously: infinite recursion → stack overflow → SIGABRT).

use elle::value::{TableKey, Value};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Helper: create @[elements...], then push the array into itself.
fn self_referencing_array(elements: &[Value]) -> Value {
    let arr = Value::array_mut(elements.to_vec());
    arr.as_array_mut().unwrap().borrow_mut().push(arr);
    arr
}

// =========================================================================
// Display
// =========================================================================

#[test]
fn display_self_referencing_array() {
    let a = self_referencing_array(&[]);
    let s = format!("{}", a);
    assert!(s.contains("<cycle>"), "expected <cycle>, got: {}", s);
}

#[test]
fn display_self_referencing_array_with_elements() {
    let a = self_referencing_array(&[Value::int(1), Value::int(2)]);
    let s = format!("{}", a);
    assert!(s.contains("1"), "got: {}", s);
    assert!(s.contains("2"), "got: {}", s);
    assert!(s.contains("<cycle>"), "got: {}", s);
}

#[test]
fn display_mutual_cycle_arrays() {
    let a = Value::array_mut(vec![]);
    let b = Value::array_mut(vec![]);
    a.as_array_mut().unwrap().borrow_mut().push(b);
    b.as_array_mut().unwrap().borrow_mut().push(a);
    let s = format!("{}", a);
    assert!(s.contains("<cycle>"), "expected <cycle>, got: {}", s);
}

#[test]
fn display_self_referencing_struct() {
    let t = Value::struct_mut();
    t.as_struct_mut()
        .unwrap()
        .borrow_mut()
        .insert(TableKey::Keyword("self".to_string()), t);
    let s = format!("{}", t);
    assert!(s.contains("<cycle>"), "expected <cycle>, got: {}", s);
}

#[test]
fn display_self_referencing_lbox() {
    let b = Value::lbox(Value::NIL);
    *b.as_lbox().unwrap().borrow_mut() = b;
    let s = format!("{}", b);
    assert!(s.contains("<cycle>"), "expected <cycle>, got: {}", s);
}

// =========================================================================
// Debug
// =========================================================================

#[test]
fn debug_self_referencing_array() {
    let a = self_referencing_array(&[Value::int(42)]);
    let s = format!("{:?}", a);
    assert!(s.contains("42"), "got: {}", s);
    assert!(s.contains("<cycle>"), "got: {}", s);
}

#[test]
fn debug_self_referencing_struct() {
    let t = Value::struct_mut();
    t.as_struct_mut()
        .unwrap()
        .borrow_mut()
        .insert(TableKey::Keyword("self".to_string()), t);
    let s = format!("{:?}", t);
    assert!(s.contains("<cycle>"), "expected <cycle>, got: {}", s);
}

// =========================================================================
// PartialEq
// =========================================================================

#[test]
fn eq_self_referencing_array_identity() {
    let a = self_referencing_array(&[]);
    // Same object: pointer-identity fast path
    assert_eq!(a, a);
}

#[test]
fn eq_mutual_cycle_arrays() {
    // a = @[b], b = @[a]  — structurally identical cycles
    let a = Value::array_mut(vec![]);
    let b = Value::array_mut(vec![]);
    a.as_array_mut().unwrap().borrow_mut().push(b);
    b.as_array_mut().unwrap().borrow_mut().push(a);
    // Must not crash. cycle detection returns true (assume equal).
    assert_eq!(a, b);
}

#[test]
fn eq_asymmetric_cycle_arrays() {
    // a = @[1 b], b = @[2 a] — structurally different
    let a = Value::array_mut(vec![Value::int(1)]);
    let b = Value::array_mut(vec![Value::int(2)]);
    a.as_array_mut().unwrap().borrow_mut().push(b);
    b.as_array_mut().unwrap().borrow_mut().push(a);
    // Must not crash. Elements differ, so not equal.
    assert_ne!(a, b);
}

#[test]
fn eq_self_referencing_lbox() {
    let b = Value::lbox(Value::NIL);
    *b.as_lbox().unwrap().borrow_mut() = b;
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
    let a = self_referencing_array(&[]);
    // Must not crash
    let _ = compute_hash(&a);
}

#[test]
fn hash_mutual_cycle_arrays() {
    let a = Value::array_mut(vec![]);
    let b = Value::array_mut(vec![]);
    a.as_array_mut().unwrap().borrow_mut().push(b);
    b.as_array_mut().unwrap().borrow_mut().push(a);
    // Must not crash
    let _ = compute_hash(&a);
    let _ = compute_hash(&b);
}

#[test]
fn hash_self_referencing_lbox() {
    let b = Value::lbox(Value::NIL);
    *b.as_lbox().unwrap().borrow_mut() = b;
    let _ = compute_hash(&b);
}

// =========================================================================
// Ord
// =========================================================================

#[test]
fn ord_self_referencing_array() {
    let a = self_referencing_array(&[]);
    // Same object: pointer-identity fast path → Equal
    assert_eq!(a.cmp(&a), std::cmp::Ordering::Equal);
}

#[test]
fn ord_mutual_cycle_arrays() {
    let a = Value::array_mut(vec![]);
    let b = Value::array_mut(vec![]);
    a.as_array_mut().unwrap().borrow_mut().push(b);
    b.as_array_mut().unwrap().borrow_mut().push(a);
    // Must not crash. Cycle detected → Equal.
    let _ = a.cmp(&b);
}

// =========================================================================
// Non-cyclic structures still work correctly
// =========================================================================

#[test]
fn display_non_cyclic_nested_arrays() {
    let inner = Value::array_mut(vec![Value::int(1), Value::int(2)]);
    let outer = Value::array_mut(vec![inner, Value::int(3)]);
    let s = format!("{}", outer);
    assert_eq!(s, "@[@[1 2] 3]");
}

#[test]
fn eq_non_cyclic_nested_arrays() {
    let a = Value::array_mut(vec![Value::int(1)]);
    let b = Value::array_mut(vec![Value::int(1)]);
    assert_eq!(a, b);
    let c = Value::array_mut(vec![Value::int(2)]);
    assert_ne!(a, c);
}

#[test]
fn hash_equal_values_same_hash() {
    let a = Value::array_mut(vec![Value::int(1), Value::int(2)]);
    let b = Value::array_mut(vec![Value::int(1), Value::int(2)]);
    assert_eq!(compute_hash(&a), compute_hash(&b));
}

// =========================================================================
// Deep nesting (not cyclic) should not trigger false positives
// =========================================================================

#[test]
fn display_deeply_nested_arrays() {
    let mut v = Value::int(0);
    for _ in 0..100 {
        v = Value::array_mut(vec![v]);
    }
    let s = format!("{}", v);
    assert!(!s.contains("<cycle>"), "false positive: {}", s);
}
