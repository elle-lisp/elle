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
        (Value::int(0), CType::Int, true),
        (Value::int(42), CType::Int, true),
        (Value::int(-5), CType::Int, true),
        (Value::int(256), CType::Short, true),
        (Value::int(65536), CType::Long, true),
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
        (Value::int(0), CType::UInt, true),
        (Value::int(100), CType::UInt, true),
        (Value::int(-1), CType::UInt, false),
        (Value::int(-100), CType::UChar, false),
        (Value::int(256), CType::UChar, true), // Will truncate but not error
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
        Value::float(0.0),
        Value::float(1.5),
        Value::float(-2.5),
        Value::float(4.2),
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
        (Value::bool(true), true),
        (Value::bool(false), false),
        (Value::int(1), true),
        (Value::int(0), false),
        (Value::NIL, false),
    ];

    for (value, expected_bool) in test_cases {
        let result = Marshal::elle_to_c(&value, &CType::Bool).unwrap();
        if let CValue::Int(n) = result {
            assert_eq!(n != 0, expected_bool)
        } else {
            panic!("Expected CValue::Int for bool")
        }
    }
}

#[test]
fn test_marshal_strings_to_pointers() {
    // Test marshaling Elle strings to C char* pointers
    let strings = vec!["hello", "world", "test", ""];

    for s in strings {
        let value = Value::string(s);
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
fn test_marshal_arrays_to_c_arrays() {
    // Test marshaling Elle arrays to C arrays
    let arr = Value::array(vec![
        Value::int(1),
        Value::int(2),
        Value::int(3),
        Value::int(4),
        Value::int(5),
    ]);

    let result = Marshal::elle_to_c(&arr, &CType::Array(Box::new(CType::Int), 5));
    assert!(result.is_ok());

    match result.unwrap() {
        CValue::Array(elems) => {
            assert_eq!(elems.len(), 5);
            // Verify each element
            for (i, elem) in elems.iter().enumerate() {
                if let CValue::Int(n) = elem {
                    assert_eq!(*n, (i + 1) as i64)
                } else {
                    panic!("Expected CValue::Int")
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
        Value::int(10),
        cons(Value::int(20), cons(Value::int(30), Value::NIL)),
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
    let result = Marshal::elle_to_c(&Value::NIL, &CType::Pointer(Box::new(CType::Int)));
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

        if let Some(_f) = result.unwrap().as_float() {
            // Float is correct type
        } else {
            panic!("Expected Value::Float");
        }
    }
}

#[test]
fn test_unmarshal_arrays() {
    // Test unmarshaling C arrays back to Elle arrays
    let carray = CValue::Array(vec![
        CValue::Int(5),
        CValue::Int(10),
        CValue::Int(15),
        CValue::Int(20),
    ]);

    let result = Marshal::c_to_elle(&carray, &CType::Array(Box::new(CType::Int), 4));
    assert!(result.is_ok());

    if let Some(vec_ref) = result.unwrap().as_array() {
        let vec = vec_ref.borrow();
        assert_eq!(vec.len(), 4);
        assert_eq!(vec[0], Value::int(5));
        assert_eq!(vec[1], Value::int(10));
        assert_eq!(vec[2], Value::int(15));
        assert_eq!(vec[3], Value::int(20));
    } else {
        panic!("Expected Value::Array");
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
    assert_eq!(result.unwrap(), Value::NIL);
}

#[test]
fn test_roundtrip_marshal_unmarshal_integers() {
    // Test roundtrip marshaling of integers
    let original = Value::int(42);
    let marshaled = Marshal::elle_to_c(&original, &CType::Int).unwrap();
    let unmarshaled = Marshal::c_to_elle(&marshaled, &CType::Int).unwrap();
    assert_eq!(original, unmarshaled);
}

#[test]
fn test_roundtrip_marshal_unmarshal_floats() {
    // Test roundtrip marshaling of floats
    let original = Value::float(2.4);
    let marshaled = Marshal::elle_to_c(&original, &CType::Double).unwrap();
    let unmarshaled = Marshal::c_to_elle(&marshaled, &CType::Double).unwrap();

    if let Some(f) = unmarshaled.as_float() {
        assert!((f - 2.4).abs() < 0.0001);
    } else {
        panic!("Expected Value::Float");
    }
}

#[test]
fn test_roundtrip_marshal_unmarshal_arrays() {
    // Test roundtrip marshaling of arrays
    let original = Value::array(vec![Value::int(1), Value::int(2), Value::int(3)]);

    let marshaled = Marshal::elle_to_c(&original, &CType::Array(Box::new(CType::Int), 3)).unwrap();

    let unmarshaled =
        Marshal::c_to_elle(&marshaled, &CType::Array(Box::new(CType::Int), 3)).unwrap();

    if let Some(vec_ref) = unmarshaled.as_array() {
        let vec = vec_ref.borrow();
        assert_eq!(vec.len(), 3);
        assert_eq!(vec[0], Value::int(1));
        assert_eq!(vec[1], Value::int(2));
        assert_eq!(vec[2], Value::int(3));
    } else {
        panic!("Expected Value::Array");
    }
}

#[test]
fn test_error_on_type_mismatch() {
    // Test that type mismatches produce errors
    let test_cases = vec![
        (Value::string("hello"), CType::Int),
        (
            Value::symbol(elle::SymbolTable::new().intern("sym").0),
            CType::Float,
        ),
        (Value::bool(true), CType::Pointer(Box::new(CType::Int))),
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
    let positive = Value::int(100);
    assert!(Marshal::elle_to_c(&positive, &CType::Int).is_ok());
    assert!(Marshal::elle_to_c(&positive, &CType::UInt).is_ok());

    // Negative values should only work for signed
    let negative = Value::int(-50);
    assert!(Marshal::elle_to_c(&negative, &CType::Int).is_ok());
    assert!(Marshal::elle_to_c(&negative, &CType::UInt).is_err());

    // Unsigned chars should reject negatives
    assert!(Marshal::elle_to_c(&negative, &CType::UChar).is_err());
}
