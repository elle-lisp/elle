// FFI Callback Integration Tests
// Tests for C function pointer callback functionality

use elle::ffi::callback::{
    create_callback, get_callback, register_callback, unregister_callback, CCallback,
};
use elle::ffi::types::CType;
use elle::value::cons;
use elle::Value;
use std::rc::Rc;

#[test]
fn test_callback_creation_and_retrieval() {
    // Create a callback ID
    let (id, info) = create_callback(vec![CType::Int], CType::Int);

    // Verify the callback info
    assert_eq!(info.id, id);
    assert_eq!(info.arg_types, vec![CType::Int]);
    assert_eq!(info.return_type, CType::Int);
}

#[test]
fn test_callback_wrapper() {
    // Create a callback wrapper
    let callback = CCallback::new(42, vec![CType::Int, CType::Float], CType::Double);

    // Verify wrapper properties
    assert_eq!(callback.id, 42);
    assert_eq!(callback.arg_types.len(), 2);
    assert_eq!(callback.return_type, CType::Double);

    // Test pointer conversion
    let ptr = callback.as_ptr();
    let id = CCallback::from_ptr(ptr);
    assert_eq!(id, 42);
}

#[test]
fn test_callback_with_closure_registration() {
    // Create a closure value
    let closure = Rc::new(Value::int(100));

    // Create and register callback
    let (id, _info) = create_callback(vec![], CType::Int);
    assert!(register_callback(id, closure.clone()));

    // Retrieve and verify
    let retrieved = get_callback(id);
    assert!(retrieved.is_some());
    assert_eq!(*retrieved.unwrap(), Value::int(100));

    // Cleanup
    assert!(unregister_callback(id));
    assert!(get_callback(id).is_none());
}

#[test]
fn test_multiple_callbacks_registration() {
    // Create multiple closures
    let closure1 = Rc::new(Value::bool(true));
    let closure2 = Rc::new(Value::float(std::f64::consts::PI));
    let closure3 = Rc::new(Value::string("callback"));

    // Register all callbacks
    let (id1, _) = create_callback(vec![], CType::Bool);
    let (id2, _) = create_callback(vec![], CType::Float);
    let (id3, _) = create_callback(vec![], CType::Pointer(Box::new(CType::Char)));

    assert!(register_callback(id1, closure1.clone()));
    assert!(register_callback(id2, closure2.clone()));
    assert!(register_callback(id3, closure3.clone()));

    // Verify all retrieved
    assert_eq!(*get_callback(id1).unwrap(), Value::bool(true));
    if let Some(f) = get_callback(id2).unwrap().as_ref().as_float() {
        assert!((f - std::f64::consts::PI).abs() < 0.01)
    } else {
        panic!("Expected Float")
    }
    let val3 = get_callback(id3).unwrap();
    if let Some(s) = val3.as_ref().as_string() {
        assert_eq!(s, "callback");
    } else {
        panic!("Expected String");
    }
    assert!(unregister_callback(id1));
    assert!(unregister_callback(id2));
    assert!(unregister_callback(id3));
}

#[test]
fn test_callback_prevents_duplicate_registration() {
    let closure = Rc::new(Value::int(42));
    let (id, _) = create_callback(vec![], CType::Int);

    // First registration should succeed
    assert!(register_callback(id, closure.clone()));

    // Second registration should fail
    let closure2 = Rc::new(Value::int(99));
    assert!(!register_callback(id, closure2));

    // Original closure should still be there
    assert_eq!(*get_callback(id).unwrap(), Value::int(42));

    // Cleanup
    unregister_callback(id);
}

#[test]
fn test_callback_various_signatures() {
    // Test callbacks with various type signatures
    let test_cases = vec![
        (vec![], CType::Void),
        (vec![CType::Int], CType::Int),
        (vec![CType::Float], CType::Float),
        (vec![CType::Bool], CType::Bool),
        (vec![CType::Int, CType::Float], CType::Double),
        (vec![CType::Int, CType::Int, CType::Int], CType::Int),
        (
            vec![CType::Pointer(Box::new(CType::Char))],
            CType::Pointer(Box::new(CType::Int)),
        ),
    ];

    for (arg_types, return_type) in test_cases {
        let (_id, info) = create_callback(arg_types.clone(), return_type.clone());
        assert_eq!(info.arg_types, arg_types);
        assert_eq!(info.return_type, return_type);
    }
}

#[test]
fn test_callback_lifecycle() {
    // Test full callback lifecycle
    let closure = Rc::new(Value::string("lifecycle test"));
    let (id, _) = create_callback(vec![CType::Int], CType::Pointer(Box::new(CType::Char)));

    // Initially not registered
    assert!(get_callback(id).is_none());

    // Register
    assert!(register_callback(id, closure));
    assert!(get_callback(id).is_some());

    // Cannot re-register
    let closure2 = Rc::new(Value::string("new"));
    assert!(!register_callback(id, closure2));

    // Original still there
    let retrieved = get_callback(id).unwrap();
    if let Some(s) = retrieved.as_ref().as_string() {
        assert_eq!(s, "lifecycle test");
    } else {
        panic!("Expected String");
    }
    // Unregister
    assert!(unregister_callback(id));
    assert!(get_callback(id).is_none());

    // Can register again after unregistering
    let closure3 = Rc::new(Value::string("second round"));
    assert!(register_callback(id, closure3));
    assert!(get_callback(id).is_some());

    // Cleanup
    unregister_callback(id);
}

#[test]
fn test_callback_with_complex_values() {
    // Test callbacks with complex Elle values
    let list = cons(
        Value::int(1),
        cons(Value::int(2), cons(Value::int(3), Value::NIL)),
    );
    let arr = Value::array(vec![Value::int(10), Value::int(20), Value::int(30)]);

    let (id1, _) = create_callback(vec![], CType::Pointer(Box::new(CType::Int)));
    let (id2, _) = create_callback(vec![], CType::Pointer(Box::new(CType::Int)));

    assert!(register_callback(id1, Rc::new(list)));
    assert!(register_callback(id2, Rc::new(arr)));

    // Verify retrieval
    assert!(get_callback(id1).is_some());
    assert!(get_callback(id2).is_some());

    // Cleanup
    unregister_callback(id1);
    unregister_callback(id2);
}

#[test]
fn test_callback_error_cases() {
    // Test error cases
    let (id, _) = create_callback(vec![], CType::Int);

    // Get non-existent callback
    assert!(get_callback(9999).is_none());

    // Unregister non-existent callback
    assert!(!unregister_callback(9999));

    // Register with valid ID
    let closure = Rc::new(Value::int(42));
    assert!(register_callback(id, closure));

    // Unregister valid callback
    assert!(unregister_callback(id));

    // Unregister again (should fail)
    assert!(!unregister_callback(id));
}

#[test]
fn test_callback_ptr_round_trip() {
    // Test converting callback ID to/from pointer
    for original_id in 1..=100 {
        let callback = CCallback::new(original_id, vec![], CType::Void);
        let ptr = callback.as_ptr();
        let retrieved_id = CCallback::from_ptr(ptr);
        assert_eq!(original_id, retrieved_id);
    }
}
