use super::*;

// =========================================================================
// Pair roundtrip
// =========================================================================

#[test]
fn cons_roundtrip_simple() {
    let h = elle::primitives::ctx::TestHeap::new();
    let first = Value::int(1);
    let rest = Value::int(2);
    let pair = h.ctx().pair(first, rest);
    assert!(pair.is_pair());
    assert!(pair.is_heap());
    let c = pair.as_pair().unwrap();
    assert_eq!(c.first, first);
    assert_eq!(c.rest, rest);
}

#[test]
fn cons_roundtrip_min_max() {
    let h = elle::primitives::ctx::TestHeap::new();
    let first = Value::int(i64::MIN);
    let rest = Value::int(i64::MAX);
    let pair = h.ctx().pair(first, rest);
    assert!(pair.is_pair());
    assert!(pair.is_heap());
    let c = pair.as_pair().unwrap();
    assert_eq!(c.first, first);
    assert_eq!(c.rest, rest);
}

#[test]
fn cons_roundtrip_zero() {
    let h = elle::primitives::ctx::TestHeap::new();
    let first = Value::int(0);
    let rest = Value::int(0);
    let pair = h.ctx().pair(first, rest);
    assert!(pair.is_pair());
    assert!(pair.is_heap());
    let c = pair.as_pair().unwrap();
    assert_eq!(c.first, first);
    assert_eq!(c.rest, rest);
}

// =========================================================================
// String roundtrip
// =========================================================================

#[test]
fn string_roundtrip_empty() {
    let h = elle::primitives::ctx::TestHeap::new();
    let v = h.ctx().string("");
    assert!(v.is_string());
    assert_eq!(v.with_string(|s| s.to_string()), Some("".to_string()));
}

#[test]
fn string_roundtrip_simple() {
    let h = elle::primitives::ctx::TestHeap::new();
    let v = h.ctx().string("hello");
    assert!(v.is_string());
    assert_eq!(v.with_string(|s| s.to_string()), Some("hello".to_string()));
}

#[test]
fn string_roundtrip_with_spaces() {
    let h = elle::primitives::ctx::TestHeap::new();
    let v = h.ctx().string("hello world");
    assert!(v.is_string());
    assert_eq!(
        v.with_string(|s| s.to_string()),
        Some("hello world".to_string())
    );
}

// =========================================================================
// List construction roundtrip
// =========================================================================

#[test]
fn list_roundtrip_empty() {
    let h = elle::primitives::ctx::TestHeap::new();
    let list_val = h.ctx().list(vec![]);
    let back = list_val.list_to_vec().unwrap();
    assert_eq!(back.len(), 0);
}

#[test]
fn list_roundtrip_single() {
    let h = elle::primitives::ctx::TestHeap::new();
    let values = vec![Value::int(42)];
    let list_val = h.ctx().list(values.clone());
    let back = list_val.list_to_vec().unwrap();
    assert_eq!(back.len(), 1);
    assert_eq!(back[0], Value::int(42));
}

#[test]
fn list_roundtrip_multiple() {
    let h = elle::primitives::ctx::TestHeap::new();
    let values = vec![Value::int(1), Value::int(2), Value::int(3)];
    let list_val = h.ctx().list(values.clone());
    let back = list_val.list_to_vec().unwrap();
    assert_eq!(back.len(), 3);
    assert_eq!(back[0], Value::int(1));
    assert_eq!(back[1], Value::int(2));
    assert_eq!(back[2], Value::int(3));
}

#[test]
fn list_roundtrip_negative() {
    let h = elle::primitives::ctx::TestHeap::new();
    let values = vec![Value::int(-5), Value::int(0), Value::int(7)];
    let list_val = h.ctx().list(values.clone());
    let back = list_val.list_to_vec().unwrap();
    assert_eq!(back.len(), 3);
    assert_eq!(back[0], Value::int(-5));
    assert_eq!(back[1], Value::int(0));
    assert_eq!(back[2], Value::int(7));
}

// =========================================================================
// Array roundtrip
// =========================================================================

#[test]
fn array_roundtrip_empty() {
    let h = elle::primitives::ctx::TestHeap::new();
    let arr = h.ctx().array_mut(vec![]);
    assert!(arr.is_array_mut());
    let borrowed = arr.as_array_mut().unwrap().borrow();
    assert_eq!(borrowed.len(), 0);
}

#[test]
fn array_roundtrip_single() {
    let h = elle::primitives::ctx::TestHeap::new();
    let values = vec![Value::int(42)];
    let arr = h.ctx().array_mut(values.clone());
    assert!(arr.is_array_mut());
    let borrowed = arr.as_array_mut().unwrap().borrow();
    assert_eq!(borrowed.len(), 1);
    assert_eq!(borrowed[0], Value::int(42));
}

#[test]
fn array_roundtrip_multiple() {
    let h = elle::primitives::ctx::TestHeap::new();
    let values = vec![Value::int(1), Value::int(2), Value::int(3)];
    let arr = h.ctx().array_mut(values.clone());
    assert!(arr.is_array_mut());
    let borrowed = arr.as_array_mut().unwrap().borrow();
    assert_eq!(borrowed.len(), 3);
    assert_eq!(borrowed[0], Value::int(1));
    assert_eq!(borrowed[1], Value::int(2));
    assert_eq!(borrowed[2], Value::int(3));
}

#[test]
fn array_roundtrip_negative() {
    let h = elle::primitives::ctx::TestHeap::new();
    let values = vec![Value::int(-5), Value::int(0), Value::int(7)];
    let arr = h.ctx().array_mut(values.clone());
    assert!(arr.is_array_mut());
    let borrowed = arr.as_array_mut().unwrap().borrow();
    assert_eq!(borrowed.len(), 3);
    assert_eq!(borrowed[0], Value::int(-5));
    assert_eq!(borrowed[1], Value::int(0));
    assert_eq!(borrowed[2], Value::int(7));
}

// =========================================================================
// Constants (not inside proptest! because they don't need generation)
// =========================================================================

#[test]
fn nil_is_falsy() {
    assert!(!Value::NIL.is_truthy());
}

#[test]
fn false_is_falsy() {
    assert!(!Value::FALSE.is_truthy());
}

#[test]
fn empty_list_is_truthy() {
    assert!(Value::EMPTY_LIST.is_truthy());
}

#[test]
fn nil_not_equal_empty_list() {
    assert_ne!(Value::NIL, Value::EMPTY_LIST);
}

#[test]
fn nil_not_equal_false() {
    assert_ne!(Value::NIL, Value::FALSE);
}
