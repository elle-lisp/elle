// FFI Type Marshaling Integration Tests
// Tests for enhanced C type marshaling support

use elle::ffi::marshal::{CValue, Marshal};
use elle::ffi::types::CType;
use elle::value::cons;
use elle::Value;

#[test]
fn test_marshal_integers_to_c_values() {
    // Test marshaling various integer types
    let values = vec![
        (Value::Int(0), CType::Int, true),
        (Value::Int(42), CType::Int, true),
        (Value::Int(-5), CType::Int, true),
        (Value::Int(256), CType::Short, true),
        (Value::Int(65536), CType::Long, true),
    ];

    for (value, ctype, should_succeed) in values {
        let result = Marshal::elle_to_c(&value, &ctype);
        if should_succeed {
            assert!(
                result.is_ok(),
                "Failed to marshal {:?} to {:?}",
                value,
                ctype
            );
        }
    }
}

#[test]
fn test_marshal_unsigned_integers_validation() {
    // Unsigned integers should reject negative values
    let test_cases = vec![
        (Value::Int(0), CType::UInt, true),
        (Value::Int(100), CType::UInt, true),
        (Value::Int(-1), CType::UInt, false),
        (Value::Int(-100), CType::UChar, false),
        (Value::Int(256), CType::UChar, true), // Will truncate but not error
    ];

    for (value, ctype, should_succeed) in test_cases {
        let result = Marshal::elle_to_c(&value, &ctype);
        assert_eq!(
            result.is_ok(),
            should_succeed,
            "Unexpected result for {:?} to {:?}",
            value,
            ctype
        );
    }
}

#[test]
fn test_marshal_floats() {
    // Test marshaling floating point values
    let test_cases = vec![
        Value::Float(0.0),
        Value::Float(1.5),
        Value::Float(-2.5),
        Value::Float(4.2),
    ];

    for value in test_cases {
        let result_f32 = Marshal::elle_to_c(&value, &CType::Float);
        let result_f64 = Marshal::elle_to_c(&value, &CType::Double);

        assert!(result_f32.is_ok());
        assert!(result_f64.is_ok());
    }
}

#[test]
fn test_marshal_booleans() {
    // Test marshaling boolean values
    let test_cases = vec![
        (Value::Bool(true), true),
        (Value::Bool(false), false),
        (Value::Int(1), true),
        (Value::Int(0), false),
        (Value::Nil, false),
    ];

    for (value, expected_bool) in test_cases {
        let result = Marshal::elle_to_c(&value, &CType::Bool).unwrap();
        match result {
            CValue::Int(n) => assert_eq!(n != 0, expected_bool),
            _ => panic!("Expected CValue::Int for bool"),
        }
    }
}

#[test]
fn test_marshal_strings_to_pointers() {
    // Test marshaling Elle strings to C char* pointers
    let strings = vec!["hello", "world", "test", ""];

    for s in strings {
        let value = Value::String(s.into());
        let result = Marshal::elle_to_c(&value, &CType::Pointer(Box::new(CType::Char)));

        assert!(
            result.is_ok(),
            "Failed to marshal string '{}' to pointer",
            s
        );
        match result.unwrap() {
            CValue::String(bytes) => {
                assert!(!bytes.is_empty(), "String bytes should not be empty");
                assert_eq!(
                    bytes[bytes.len() - 1],
                    0,
                    "String should be null-terminated"
                );
            }
            _ => panic!("Expected CValue::String"),
        }
    }
}

#[test]
fn test_marshal_vectors_to_arrays() {
    // Test marshaling Elle vectors to C arrays
    let vector = Value::Vector(std::rc::Rc::new(vec![
        Value::Int(1),
        Value::Int(2),
        Value::Int(3),
        Value::Int(4),
        Value::Int(5),
    ]));

    let result = Marshal::elle_to_c(&vector, &CType::Array(Box::new(CType::Int), 5));
    assert!(result.is_ok());

    match result.unwrap() {
        CValue::Array(elems) => {
            assert_eq!(elems.len(), 5);
            // Verify each element
            for (i, elem) in elems.iter().enumerate() {
                match elem {
                    CValue::Int(n) => assert_eq!(*n, (i + 1) as i64),
                    _ => panic!("Expected CValue::Int"),
                }
            }
        }
        _ => panic!("Expected CValue::Array"),
    }
}

#[test]
fn test_marshal_cons_lists_to_arrays() {
    // Test marshaling Elle cons lists to C arrays
    let list = cons(
        Value::Int(10),
        cons(Value::Int(20), cons(Value::Int(30), Value::Nil)),
    );

    let result = Marshal::elle_to_c(&list, &CType::Array(Box::new(CType::Int), 3));
    assert!(result.is_ok());

    match result.unwrap() {
        CValue::Array(elems) => {
            assert_eq!(elems.len(), 3);
            assert_eq!(elems[0], CValue::Int(10));
            assert_eq!(elems[1], CValue::Int(20));
            assert_eq!(elems[2], CValue::Int(30));
        }
        _ => panic!("Expected CValue::Array"),
    }
}

#[test]
fn test_marshal_nil_as_null_pointer() {
    // Test that nil marshals to null pointer
    let result = Marshal::elle_to_c(&Value::Nil, &CType::Pointer(Box::new(CType::Int)));
    assert!(result.is_ok());

    match result.unwrap() {
        CValue::Pointer(p) => assert!(p.is_null()),
        _ => panic!("Expected CValue::Pointer"),
    }
}

#[test]
fn test_unmarshal_integers() {
    // Test unmarshaling C values back to Elle integers
    let test_cases = vec![
        (CValue::Int(0), CType::Int),
        (CValue::Int(42), CType::Int),
        (CValue::Int(-10), CType::Int),
        (CValue::UInt(100), CType::UInt),
    ];

    for (cvalue, ctype) in test_cases {
        let result = Marshal::c_to_elle(&cvalue, &ctype);
        assert!(
            result.is_ok(),
            "Failed to unmarshal {:?} from {:?}",
            cvalue,
            ctype
        );
    }
}

#[test]
fn test_unmarshal_floats() {
    // Test unmarshaling C values back to Elle floats
    let test_cases = vec![
        (CValue::Float(0.0), CType::Float),
        (CValue::Float(1.5), CType::Double),
        (CValue::Float(-2.5), CType::Double),
    ];

    for (cvalue, ctype) in test_cases {
        let result = Marshal::c_to_elle(&cvalue, &ctype);
        assert!(result.is_ok());

        if let Value::Float(_f) = result.unwrap() {
            // Float is correct type
        } else {
            panic!("Expected Value::Float");
        }
    }
}

#[test]
fn test_unmarshal_arrays() {
    // Test unmarshaling C arrays back to Elle vectors
    let carray = CValue::Array(vec![
        CValue::Int(5),
        CValue::Int(10),
        CValue::Int(15),
        CValue::Int(20),
    ]);

    let result = Marshal::c_to_elle(&carray, &CType::Array(Box::new(CType::Int), 4));
    assert!(result.is_ok());

    match result.unwrap() {
        Value::Vector(vec) => {
            assert_eq!(vec.len(), 4);
            assert_eq!(vec[0], Value::Int(5));
            assert_eq!(vec[1], Value::Int(10));
            assert_eq!(vec[2], Value::Int(15));
            assert_eq!(vec[3], Value::Int(20));
        }
        _ => panic!("Expected Value::Vector"),
    }
}

#[test]
fn test_unmarshal_null_pointer_to_nil() {
    // Test that null pointers unmarshal to nil
    let result = Marshal::c_to_elle(
        &CValue::Pointer(std::ptr::null()),
        &CType::Pointer(Box::new(CType::Int)),
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::Nil);
}

#[test]
fn test_roundtrip_marshal_unmarshal_integers() {
    // Test roundtrip marshaling of integers
    let original = Value::Int(42);
    let marshaled = Marshal::elle_to_c(&original, &CType::Int).unwrap();
    let unmarshaled = Marshal::c_to_elle(&marshaled, &CType::Int).unwrap();
    assert_eq!(original, unmarshaled);
}

#[test]
fn test_roundtrip_marshal_unmarshal_floats() {
    // Test roundtrip marshaling of floats
    let original = Value::Float(2.4);
    let marshaled = Marshal::elle_to_c(&original, &CType::Double).unwrap();
    let unmarshaled = Marshal::c_to_elle(&marshaled, &CType::Double).unwrap();

    if let Value::Float(f) = unmarshaled {
        assert!((f - 2.4).abs() < 0.0001);
    } else {
        panic!("Expected Value::Float");
    }
}

#[test]
fn test_roundtrip_marshal_unmarshal_arrays() {
    // Test roundtrip marshaling of arrays
    let original = Value::Vector(std::rc::Rc::new(vec![
        Value::Int(1),
        Value::Int(2),
        Value::Int(3),
    ]));

    let marshaled = Marshal::elle_to_c(&original, &CType::Array(Box::new(CType::Int), 3)).unwrap();

    let unmarshaled =
        Marshal::c_to_elle(&marshaled, &CType::Array(Box::new(CType::Int), 3)).unwrap();

    match unmarshaled {
        Value::Vector(vec) => {
            assert_eq!(vec.len(), 3);
            assert_eq!(vec[0], Value::Int(1));
            assert_eq!(vec[1], Value::Int(2));
            assert_eq!(vec[2], Value::Int(3));
        }
        _ => panic!("Expected Value::Vector"),
    }
}

#[test]
fn test_error_on_type_mismatch() {
    // Test that type mismatches produce errors
    let test_cases = vec![
        (Value::String("hello".into()), CType::Int),
        (
            Value::Symbol(elle::SymbolTable::new().intern("sym")),
            CType::Float,
        ),
        (Value::Bool(true), CType::Pointer(Box::new(CType::Int))),
    ];

    for (value, ctype) in test_cases {
        let result = Marshal::elle_to_c(&value, &ctype);
        assert!(result.is_err(), "Should error on type mismatch");
    }
}

#[test]
fn test_signed_unsigned_handling() {
    // Test proper handling of signed vs unsigned types

    // Positive values should work for both
    let positive = Value::Int(100);
    assert!(Marshal::elle_to_c(&positive, &CType::Int).is_ok());
    assert!(Marshal::elle_to_c(&positive, &CType::UInt).is_ok());

    // Negative values should only work for signed
    let negative = Value::Int(-50);
    assert!(Marshal::elle_to_c(&negative, &CType::Int).is_ok());
    assert!(Marshal::elle_to_c(&negative, &CType::UInt).is_err());

    // Unsigned chars should reject negatives
    assert!(Marshal::elle_to_c(&negative, &CType::UChar).is_err());
}
