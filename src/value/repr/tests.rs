//! Tests for NaN-boxed Value representation.

use super::*;

#[test]
fn test_size() {
    assert_eq!(std::mem::size_of::<Value>(), 8);
}

#[test]
fn test_nil() {
    let v = Value::NIL;
    assert!(v.is_nil());
    assert!(!v.is_bool());
    assert!(!v.is_int());
    assert!(!v.is_float());
    assert!(!v.is_truthy()); // nil is falsy
}

#[test]
fn test_undefined() {
    let v = Value::UNDEFINED;
    assert!(v.is_undefined());
    assert!(!v.is_nil());
    assert!(!v.is_bool());
    assert!(!v.is_int());
    assert!(!v.is_float());
    assert!(!v.is_empty_list());
    // Note: is_truthy() now debug_asserts that UNDEFINED never reaches it.
    // UNDEFINED should never appear in user-visible evaluation.

    // Verify UNDEFINED is distinct from all other special constants
    assert_ne!(Value::UNDEFINED.to_bits(), Value::NIL.to_bits());
    assert_ne!(Value::UNDEFINED.to_bits(), Value::TRUE.to_bits());
    assert_ne!(Value::UNDEFINED.to_bits(), Value::FALSE.to_bits());
    assert_ne!(Value::UNDEFINED.to_bits(), Value::EMPTY_LIST.to_bits());
}

#[test]
fn test_bool() {
    assert!(Value::TRUE.is_bool());
    assert!(Value::FALSE.is_bool());
    assert_eq!(Value::TRUE.as_bool(), Some(true));
    assert_eq!(Value::FALSE.as_bool(), Some(false));
    assert!(Value::TRUE.is_truthy());
    assert!(!Value::FALSE.is_truthy());
}

#[test]
fn test_int_roundtrip() {
    for &n in &[0i64, 1, -1, 100, -100, INT_MAX, INT_MIN] {
        let v = Value::int(n);
        assert!(v.is_int());
        assert!(!v.is_float());
        assert_eq!(v.as_int(), Some(n), "Failed for {}", n);
    }
}

#[test]
fn test_float_roundtrip() {
    for &f in &[
        0.0f64,
        1.0,
        -1.0,
        std::f64::consts::PI,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        let v = Value::float(f);
        assert!(v.is_float());
        assert!(!v.is_int());
        assert_eq!(v.as_float(), Some(f));
    }
}

#[test]
fn test_symbol() {
    let v = Value::symbol(42);
    assert!(v.is_symbol());
    assert_eq!(v.as_symbol(), Some(42));
}

#[test]
fn test_keyword() {
    let v = Value::keyword("test");
    assert!(v.is_keyword());
    assert_eq!(v.as_keyword_name(), Some("test"));
}

#[test]
fn test_bool_constructor() {
    assert_eq!(Value::bool(true), Value::TRUE);
    assert_eq!(Value::bool(false), Value::FALSE);
}

#[test]
fn test_string_constructor() {
    let v = Value::string("hello");
    assert!(v.is_string());
    assert_eq!(v.with_string(|s| s.to_string()), Some("hello".to_string()));
}

#[test]
fn test_sso_short_string() {
    let v = Value::string("hi");
    assert!(v.is_string());
    assert!(!v.is_heap()); // SSO, not heap
    assert_eq!(v.with_string(|s| s.to_string()), Some("hi".to_string()));
}

#[test]
fn test_sso_empty_string() {
    let v = Value::string("");
    assert!(v.is_string());
    assert!(!v.is_heap());
    assert_eq!(v.with_string(|s| s.to_string()), Some(String::new()));
}

#[test]
fn test_sso_six_byte_string() {
    let v = Value::string("abcdef");
    assert!(v.is_string());
    assert!(!v.is_heap());
    assert_eq!(v.with_string(|s| s.to_string()), Some("abcdef".to_string()));
}

#[test]
fn test_heap_seven_byte_string() {
    let v = Value::string("abcdefg");
    assert!(v.is_string());
    assert!(v.is_heap()); // Too long for SSO
    assert_eq!(
        v.with_string(|s| s.to_string()),
        Some("abcdefg".to_string())
    );
}

#[test]
fn test_sso_equality() {
    let a = Value::string("hi");
    let b = Value::string("hi");
    assert_eq!(a, b);
    assert_eq!(a.to_bits(), b.to_bits()); // Same bit pattern
}

#[test]
fn test_sso_nul_byte_falls_back_to_heap() {
    let v = Value::string("a\0b");
    assert!(v.is_string());
    assert!(v.is_heap()); // Contains NUL, falls back to heap
}

#[test]
fn test_cons_constructor() {
    let car = Value::int(1);
    let cdr = Value::int(2);
    let v = Value::cons(car, cdr);
    assert!(v.is_cons());
    if let Some(cons) = v.as_cons() {
        assert_eq!(cons.first, car);
        assert_eq!(cons.rest, cdr);
    } else {
        panic!("Expected cons cell");
    }
}

#[test]
fn test_array_constructor() {
    let elements = vec![Value::int(1), Value::int(2), Value::int(3)];
    let v = Value::array(elements.clone());
    assert!(v.is_array());
    if let Some(vec_ref) = v.as_array() {
        let borrowed = vec_ref.borrow();
        assert_eq!(borrowed.len(), 3);
        assert_eq!(borrowed[0], Value::int(1));
        assert_eq!(borrowed[1], Value::int(2));
        assert_eq!(borrowed[2], Value::int(3));
    } else {
        panic!("Expected array");
    }
}

#[test]
fn test_table_constructor() {
    let v = Value::table();
    assert!(v.is_table());
    if let Some(table_ref) = v.as_table() {
        let borrowed = table_ref.borrow();
        assert_eq!(borrowed.len(), 0);
    } else {
        panic!("Expected table");
    }
}

#[test]
fn test_cell_constructor() {
    let inner = Value::int(42);
    let v = Value::cell(inner);
    assert!(v.is_cell());
    if let Some(cell_ref) = v.as_cell() {
        let borrowed = cell_ref.borrow();
        assert_eq!(*borrowed, Value::int(42));
    } else {
        panic!("Expected cell");
    }
}

#[test]
fn test_list_function() {
    let values = vec![Value::int(1), Value::int(2), Value::int(3)];
    let list_val = list(values);
    assert!(list_val.is_list());

    // Convert back to vec
    let result = list_val.list_to_vec().unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result[0], Value::int(1));
    assert_eq!(result[1], Value::int(2));
    assert_eq!(result[2], Value::int(3));
}

#[test]
fn test_is_list() {
    // Proper list
    let proper_list = Value::cons(Value::int(1), Value::cons(Value::int(2), Value::NIL));
    assert!(proper_list.is_list());

    // Not a list (improper list)
    let improper_list = Value::cons(Value::int(1), Value::int(2));
    assert!(!improper_list.is_list());

    // Nil is a list
    assert!(Value::NIL.is_list());
}

#[test]
fn test_type_name() {
    assert_eq!(Value::NIL.type_name(), "nil");
    assert_eq!(Value::TRUE.type_name(), "boolean");
    assert_eq!(Value::int(42).type_name(), "integer");
    assert_eq!(Value::float(std::f64::consts::PI).type_name(), "float");
    assert_eq!(Value::symbol(1).type_name(), "symbol");
    assert_eq!(Value::keyword("test").type_name(), "keyword");
    assert_eq!(Value::string("test").type_name(), "string");
    assert_eq!(
        Value::cons(Value::NIL, Value::EMPTY_LIST).type_name(),
        "list"
    );
    assert_eq!(Value::array(vec![]).type_name(), "array");
    assert_eq!(Value::table().type_name(), "table");
    assert_eq!(Value::cell(Value::NIL).type_name(), "cell");
}

#[test]
fn test_truthiness_semantics() {
    // Only nil and false are falsy
    assert!(!Value::NIL.is_truthy(), "nil is falsy");
    assert!(!Value::FALSE.is_truthy(), "false is falsy");

    // true is truthy
    assert!(Value::TRUE.is_truthy(), "true is truthy");

    // Zero is truthy (not falsy like in C)
    assert!(Value::int(0).is_truthy(), "0 is truthy");
    assert!(Value::float(0.0).is_truthy(), "0.0 is truthy");

    // Empty string is truthy
    assert!(Value::string("").is_truthy(), "empty string is truthy");

    // Empty list is truthy (it's nil, but we test the list form)
    assert!(Value::EMPTY_LIST.is_truthy(), "empty list is truthy");

    // Empty array is truthy
    assert!(Value::array(vec![]).is_truthy(), "empty array is truthy");

    // Regular values are truthy
    assert!(Value::int(1).is_truthy(), "1 is truthy");
    assert!(Value::int(-1).is_truthy(), "-1 is truthy");
    assert!(
        Value::float(std::f64::consts::PI).is_truthy(),
        "PI is truthy"
    );
    assert!(
        Value::string("hello").is_truthy(),
        "non-empty string is truthy"
    );
    assert!(Value::symbol(1).is_truthy(), "symbol is truthy");
    assert!(Value::keyword("test").is_truthy(), "keyword is truthy");

    // Non-empty list is truthy
    let non_empty_list = Value::cons(Value::int(1), Value::NIL);
    assert!(non_empty_list.is_truthy(), "non-empty list is truthy");

    // Non-empty array is truthy
    let non_empty_vec = Value::array(vec![Value::int(1)]);
    assert!(non_empty_vec.is_truthy(), "non-empty array is truthy");

    // Table is truthy
    assert!(Value::table().is_truthy(), "table is truthy");

    // Cell is truthy
    assert!(Value::cell(Value::int(42)).is_truthy(), "cell is truthy");
}

#[test]
fn test_pointer() {
    // NULL pointer becomes nil
    let null = Value::pointer(0);
    assert!(null.is_nil());
    assert!(!null.is_pointer());
    assert_eq!(null.as_pointer(), None);

    // Non-null pointer
    let ptr = Value::pointer(0x7F4A_2B3C_0000);
    assert!(ptr.is_pointer());
    assert!(!ptr.is_nil());
    assert!(!ptr.is_heap());
    assert!(!ptr.is_int());
    assert_eq!(ptr.as_pointer(), Some(0x7F4A_2B3C_0000));
    assert_eq!(ptr.type_name(), "pointer");

    // Pointer equality
    let ptr2 = Value::pointer(0x7F4A_2B3C_0000);
    assert_eq!(ptr, ptr2);

    // Different pointers are not equal
    let ptr3 = Value::pointer(0x1234_5678_0000);
    assert_ne!(ptr, ptr3);

    // Pointers are truthy
    assert!(ptr.is_truthy());

    // Display format
    assert_eq!(format!("{}", ptr), "<pointer 0x7f4a2b3c0000>");
}

#[test]
fn test_ffi_signature_roundtrip() {
    use crate::ffi::types::{CallingConvention, Signature, TypeDesc};
    let sig = Signature {
        convention: CallingConvention::Default,
        ret: TypeDesc::I32,
        args: vec![TypeDesc::Ptr, TypeDesc::U64],
        fixed_args: None,
    };
    let v = Value::ffi_signature(sig.clone());
    assert!(v.is_heap());
    assert_eq!(v.as_ffi_signature(), Some(&sig));
    assert_eq!(v.type_name(), "ffi-signature");
    assert_eq!(format!("{}", v), "<ffi-signature>");
}

#[test]
fn test_lib_handle_roundtrip() {
    let v = Value::lib_handle(42);
    assert!(v.is_heap());
    assert_eq!(v.as_lib_handle(), Some(42));
    assert_eq!(v.type_name(), "library-handle");
    assert_eq!(format!("{}", v), "<lib-handle:42>");
}

#[test]
fn test_ffi_signature_equality() {
    use crate::ffi::types::{CallingConvention, Signature, TypeDesc};
    let sig1 = Signature {
        convention: CallingConvention::Default,
        ret: TypeDesc::Void,
        args: vec![],
        fixed_args: None,
    };
    let sig2 = Signature {
        convention: CallingConvention::Default,
        ret: TypeDesc::Void,
        args: vec![],
        fixed_args: None,
    };
    let sig3 = Signature {
        convention: CallingConvention::Default,
        ret: TypeDesc::I32,
        args: vec![],
        fixed_args: None,
    };
    assert_eq!(
        Value::ffi_signature(sig1.clone()),
        Value::ffi_signature(sig2)
    );
    assert_ne!(Value::ffi_signature(sig1), Value::ffi_signature(sig3));
}

#[test]
fn test_lib_handle_equality() {
    assert_eq!(Value::lib_handle(1), Value::lib_handle(1));
    assert_ne!(Value::lib_handle(1), Value::lib_handle(2));
}

#[test]
fn test_ffi_signature_not_on_non_signature() {
    assert_eq!(Value::int(42).as_ffi_signature(), None);
    assert_eq!(Value::NIL.as_ffi_signature(), None);
    assert_eq!(Value::string("hello").as_ffi_signature(), None);
}

#[test]
fn test_lib_handle_not_on_non_handle() {
    assert_eq!(Value::int(42).as_lib_handle(), None);
    assert_eq!(Value::NIL.as_lib_handle(), None);
    assert_eq!(Value::string("hello").as_lib_handle(), None);
}

#[test]
fn test_ffi_type_roundtrip() {
    use crate::ffi::types::{StructDesc, TypeDesc};
    let desc = TypeDesc::Struct(StructDesc {
        fields: vec![TypeDesc::I32, TypeDesc::Double, TypeDesc::Ptr],
    });
    let v = Value::ffi_type(desc.clone());
    assert_eq!(v.type_name(), "ffi-type");
    assert_eq!(v.as_ffi_type(), Some(&desc));
}

#[test]
fn test_ffi_type_equality() {
    use crate::ffi::types::{StructDesc, TypeDesc};
    let desc1 = TypeDesc::Struct(StructDesc {
        fields: vec![TypeDesc::I32, TypeDesc::Double],
    });
    let desc2 = TypeDesc::Struct(StructDesc {
        fields: vec![TypeDesc::I32, TypeDesc::Double],
    });
    let desc3 = TypeDesc::Struct(StructDesc {
        fields: vec![TypeDesc::I32, TypeDesc::I32],
    });
    assert_eq!(Value::ffi_type(desc1.clone()), Value::ffi_type(desc2));
    assert_ne!(Value::ffi_type(desc1), Value::ffi_type(desc3));
}

#[test]
fn test_ffi_type_not_on_non_type() {
    assert_eq!(Value::int(42).as_ffi_type(), None);
    assert_eq!(Value::NIL.as_ffi_type(), None);
    assert_eq!(Value::string("hello").as_ffi_type(), None);
}

#[test]
fn test_ffi_type_array() {
    use crate::ffi::types::TypeDesc;
    let desc = TypeDesc::Array(Box::new(TypeDesc::I32), 10);
    let v = Value::ffi_type(desc.clone());
    assert_eq!(v.as_ffi_type(), Some(&desc));
    assert_eq!(v.type_name(), "ffi-type");
}
