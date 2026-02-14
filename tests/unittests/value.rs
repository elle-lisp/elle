// DEFENSE: Value type is the foundation - must be rock solid
use elle::value::{cons, list, Arity, Value};
use std::rc::Rc;

#[test]
fn test_value_equality() {
    // Integers
    assert_eq!(Value::Int(42), Value::Int(42));
    assert_ne!(Value::Int(42), Value::Int(43));

    // Floats
    assert_eq!(
        Value::Float(std::f64::consts::PI),
        Value::Float(std::f64::consts::PI)
    );
    assert_ne!(
        Value::Float(std::f64::consts::PI),
        Value::Float(std::f64::consts::E)
    );

    // Booleans
    assert_eq!(Value::Bool(true), Value::Bool(true));
    assert_ne!(Value::Bool(true), Value::Bool(false));

    // Nil
    assert_eq!(Value::Nil, Value::Nil);

    // Cross-type inequality
    assert_ne!(Value::Int(0), Value::Bool(false));
    assert_ne!(Value::Int(0), Value::Nil);
}

#[test]
fn test_truthiness() {
    // Truthy values
    assert!(Value::Int(0).is_truthy());
    assert!(Value::Int(1).is_truthy());
    assert!(Value::Float(0.0).is_truthy());
    assert!(Value::Bool(true).is_truthy());
    assert!(cons(Value::Int(1), Value::Nil).is_truthy());
    assert!(Value::Nil.is_truthy()); // Empty list is truthy (matching Janet/modern Lisps)

    // Falsy values
    assert!(!Value::Bool(false).is_truthy());
}

#[test]
fn test_nil_check() {
    assert!(Value::Nil.is_nil());
    assert!(!Value::Int(0).is_nil());
    assert!(!Value::Bool(false).is_nil());
}

#[test]
fn test_type_conversions() {
    // Int conversion
    assert_eq!(Value::Int(42).as_int().unwrap(), 42);
    assert!(Value::Float(std::f64::consts::PI).as_int().is_err());

    // Float conversion
    assert_eq!(
        Value::Float(std::f64::consts::PI).as_float().unwrap(),
        std::f64::consts::PI
    );
    assert_eq!(Value::Int(42).as_float().unwrap(), 42.0); // Coercion
}

#[test]
fn test_cons_cell() {
    let cons_cell = cons(Value::Int(1), Value::Int(2));
    let cons_ref = cons_cell.as_cons().unwrap();

    assert_eq!(cons_ref.first, Value::Int(1));
    assert_eq!(cons_ref.rest, Value::Int(2));
}

#[test]
fn test_list_construction() {
    let l = list(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);

    assert!(l.is_list());

    let vec = l.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::Int(1));
    assert_eq!(vec[1], Value::Int(2));
    assert_eq!(vec[2], Value::Int(3));
}

#[test]
fn test_empty_list() {
    let empty = Value::Nil;
    assert!(empty.is_list());
    assert_eq!(empty.list_to_vec().unwrap().len(), 0);
}

#[test]
fn test_improper_list() {
    // (1 . 2) is not a proper list
    let improper = cons(Value::Int(1), Value::Int(2));
    assert!(!improper.is_list());
    assert!(improper.list_to_vec().is_err());
}

#[test]
fn test_nested_lists() {
    // ((1 2) (3 4))
    let inner1 = list(vec![Value::Int(1), Value::Int(2)]);
    let inner2 = list(vec![Value::Int(3), Value::Int(4)]);
    let outer = list(vec![inner1, inner2]);

    assert!(outer.is_list());
    let vec = outer.list_to_vec().unwrap();
    assert_eq!(vec.len(), 2);
    assert!(vec[0].is_list());
    assert!(vec[1].is_list());
}

#[test]
fn test_vector() {
    let vec = vec![Value::Int(1), Value::Int(2), Value::Int(3)];
    let v = Value::Vector(Rc::new(vec));

    let vec_ref = v.as_vector().unwrap();
    assert_eq!(vec_ref.len(), 3);
    assert_eq!(vec_ref[0], Value::Int(1));
}

#[test]
fn test_string() {
    let s = Value::String(Rc::from("hello"));
    match s {
        Value::String(ref str_ref) => assert_eq!(&**str_ref, "hello"),
        _ => panic!("Expected string"),
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
    // Rc should allow efficient sharing
    let tail = cons(Value::Int(2), Value::Nil);
    let list1 = cons(Value::Int(1), tail.clone());
    let list2 = cons(Value::Int(0), tail.clone());

    // Both lists share the same tail
    assert!(list1.is_list());
    assert!(list2.is_list());
}

#[test]
fn test_large_list() {
    // Test with 1000 elements
    let values: Vec<Value> = (0..1000).map(Value::Int).collect();
    let l = list(values);

    assert!(l.is_list());
    let vec = l.list_to_vec().unwrap();
    assert_eq!(vec.len(), 1000);
    assert_eq!(vec[0], Value::Int(0));
    assert_eq!(vec[999], Value::Int(999));
}
