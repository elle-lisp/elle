// DEFENSE: Value type is the foundation - must be rock solid
use elle::value::{cons, list, Arity, Value};

#[test]
fn test_value_equality() {
    // Integers
    assert_eq!(Value::int(42), Value::int(42));
    assert_ne!(Value::int(42), Value::int(43));

    // Floats
    assert_eq!(
        Value::float(std::f64::consts::PI),
        Value::float(std::f64::consts::PI)
    );
    assert_ne!(
        Value::float(std::f64::consts::PI),
        Value::float(std::f64::consts::E)
    );

    // Booleans
    assert_eq!(Value::bool(true), Value::bool(true));
    assert_ne!(Value::bool(true), Value::bool(false));

    // Nil
    assert_eq!(Value::NIL, Value::NIL);

    // Cross-type inequality
    assert_ne!(Value::int(0), Value::bool(false));
    assert_ne!(Value::int(0), Value::NIL);
}

// ============================================================================
// TRUTHINESS SEMANTICS - DO NOT CHANGE WITHOUT UNDERSTANDING THE DESIGN
// ============================================================================
// nil is FALSY (represents absence, logical false)
// () (empty list) is TRUTHY (it's a list, just empty - distinct from nil)
// #f is FALSY (explicit boolean false)
// 0 is TRUTHY (numbers are always truthy)
// ============================================================================

#[test]
fn test_truthiness() {
    // Truthy values
    assert!(Value::int(0).is_truthy());
    assert!(Value::int(1).is_truthy());
    assert!(Value::float(0.0).is_truthy());
    assert!(Value::bool(true).is_truthy());
    assert!(cons(Value::int(1), Value::EMPTY_LIST).is_truthy());
    assert!(Value::EMPTY_LIST.is_truthy()); // Empty list is truthy

    // Falsy values
    assert!(!Value::bool(false).is_truthy());
    assert!(!Value::NIL.is_truthy()); // nil is falsy
}

#[test]
fn test_nil_check() {
    assert!(Value::NIL.is_nil());
    assert!(!Value::int(0).is_nil());
    assert!(!Value::bool(false).is_nil());
}

#[test]
fn test_type_conversions() {
    // Int conversion
    assert_eq!(Value::int(42).as_int().unwrap(), 42);
    assert!(Value::float(std::f64::consts::PI).as_int().is_none());

    // Float conversion
    assert_eq!(
        Value::float(std::f64::consts::PI).as_float().unwrap(),
        std::f64::consts::PI
    );
    assert_eq!(Value::int(42).as_number().unwrap(), 42.0); // Coercion
}

#[test]
fn test_cons_cell() {
    let cons_cell = cons(Value::int(1), Value::int(2));
    let cons_ref = cons_cell.as_cons().unwrap();

    assert_eq!(cons_ref.first, Value::int(1));
    assert_eq!(cons_ref.rest, Value::int(2));
}

#[test]
fn test_list_construction() {
    let l = list(vec![Value::int(1), Value::int(2), Value::int(3)]);

    assert!(l.is_list());

    let vec = l.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
}

#[test]
fn test_empty_list() {
    let empty = Value::EMPTY_LIST;
    assert!(empty.is_list());
    assert!(empty.is_empty_list()); // Verify it's empty list
    assert!(!empty.is_nil()); // Verify it's NOT nil
    assert_eq!(empty.list_to_vec().unwrap().len(), 0);
}

#[test]
fn test_improper_list() {
    // (1 . 2) is not a proper list
    let improper = cons(Value::int(1), Value::int(2));
    assert!(!improper.is_list());
    assert!(improper.list_to_vec().is_err());
}

#[test]
fn test_nested_lists() {
    // ((1 2) (3 4))
    let inner1 = list(vec![Value::int(1), Value::int(2)]);
    let inner2 = list(vec![Value::int(3), Value::int(4)]);
    let outer = list(vec![inner1, inner2]);

    assert!(outer.is_list());
    let vec = outer.list_to_vec().unwrap();
    assert_eq!(vec.len(), 2);
    assert!(vec[0].is_list());
    assert!(vec[1].is_list());
}

#[test]
fn test_vector() {
    let vec = vec![Value::int(1), Value::int(2), Value::int(3)];
    let v = Value::vector(vec);

    let vec_ref = v.as_vector().unwrap();
    let borrowed = vec_ref.borrow();
    assert_eq!(borrowed.len(), 3);
    assert_eq!(borrowed[0], Value::int(1));
}

#[test]
fn test_string() {
    let s = Value::string("hello");
    match s.as_string() {
        Some(str_ref) => assert_eq!(str_ref, "hello"),
        None => panic!("Expected string"),
    }
}

#[test]
fn test_arity_matching() {
    // Exact arity
    let exact = Arity::Exact(2);
    assert!(exact.matches(2));
    assert!(!exact.matches(1));
    assert!(!exact.matches(3));

    // At least
    let at_least = Arity::AtLeast(1);
    assert!(!at_least.matches(0));
    assert!(at_least.matches(1));
    assert!(at_least.matches(2));
    assert!(at_least.matches(100));

    // Range
    let range = Arity::Range(1, 3);
    assert!(!range.matches(0));
    assert!(range.matches(1));
    assert!(range.matches(2));
    assert!(range.matches(3));
    assert!(!range.matches(4));
}

#[test]
fn test_cons_sharing() {
    // Cons cells should allow efficient sharing
    let tail = cons(Value::int(2), Value::EMPTY_LIST);
    let list1 = cons(Value::int(1), tail);
    let list2 = cons(Value::int(0), tail);

    // Both lists share the same tail
    assert!(list1.is_list());
    assert!(list2.is_list());
}

#[test]
fn test_large_list() {
    // Test with 1000 elements
    let values: Vec<Value> = (0..1000).map(Value::int).collect();
    let l = list(values);

    assert!(l.is_list());
    let vec = l.list_to_vec().unwrap();
    assert_eq!(vec.len(), 1000);
    assert_eq!(vec[0], Value::int(0));
    assert_eq!(vec[999], Value::int(999));
}

#[test]
fn test_is_nil_semantics() {
    // (is-nil nil) → true
    assert!(Value::NIL.is_nil());

    // (is-nil '()) → FALSE (empty list is NOT nil)
    assert!(!Value::empty_list().is_nil());

    // (is-nil 0) → false
    assert!(!Value::int(0).is_nil());

    // (is-nil #f) → false
    assert!(!Value::bool(false).is_nil());
}

#[test]
fn test_is_empty_list_semantics() {
    // Empty list should be detected
    assert!(Value::empty_list().is_empty_list());

    // Nil should NOT be an empty list
    assert!(!Value::NIL.is_empty_list());

    // Non-empty list should not be empty
    let non_empty = list(vec![Value::int(1)]);
    assert!(!non_empty.is_empty_list());
}
