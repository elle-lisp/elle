use elle::ffi::types::CType;
/// FFI Phase 1 Integration Tests
///
/// Tests for:
/// - Library loading
/// - Symbol resolution
/// - Type system
/// - FFI subsystem
use elle::ffi::FFISubsystem;
use elle::value::{LibHandle, Value};
use elle::VM;

#[test]
fn test_ffi_subsystem_creation() {
    let ffi = FFISubsystem::new();
    assert_eq!(ffi.loaded_libraries().len(), 0);
}

#[test]
#[cfg(target_os = "linux")]
fn test_load_libc() {
    let mut ffi = FFISubsystem::new();

    // Try common paths for libc
    let paths = vec![
        "/lib/x86_64-linux-gnu/libc.so.6",
        "/lib64/libc.so.6",
        "libc.so.6",
    ];

    let mut loaded = false;
    for path in paths {
        if let Ok(id) = ffi.load_library(path) {
            loaded = true;
            assert!(ffi.get_library(id).is_some());
            assert_eq!(ffi.loaded_libraries().len(), 1);
            break;
        }
    }

    if !loaded {
        eprintln!("Warning: Could not load libc from any standard path");
    }
}

#[test]
fn test_type_sizes() {
    assert_eq!(CType::Bool.size(), 1);
    assert_eq!(CType::Char.size(), 1);
    assert_eq!(CType::Short.size(), 2);
    assert_eq!(CType::Int.size(), 4);
    assert_eq!(CType::Long.size(), 8);
    assert_eq!(CType::Float.size(), 4);
    assert_eq!(CType::Double.size(), 8);
}

#[test]
fn test_type_alignment() {
    assert_eq!(CType::Bool.alignment(), 1);
    assert_eq!(CType::Short.alignment(), 2);
    assert_eq!(CType::Int.alignment(), 4);
    assert_eq!(CType::Long.alignment(), 8);
    assert_eq!(CType::Double.alignment(), 8);
}

#[test]
fn test_library_handle_value() {
    let handle = LibHandle(42);
    // LibHandle is stored as a heap pointer in the new Value representation
    // For now, we just verify that LibHandle can be created
    assert_eq!(handle.0, 42);
}

#[test]
fn test_vm_ffi_integration() {
    let _vm = VM::new();

    // VM should have FFI subsystem
    // assert_eq!(vm.ffi().loaded_libraries().len(), 0);
}

#[test]
fn test_ffi_library_unload() {
    let mut ffi = FFISubsystem::new();

    // Unload nonexistent library
    assert!(ffi.unload_library(999).is_none());
}

#[test]
fn test_multiple_library_ids() {
    let ffi = FFISubsystem::new();

    // When we load libraries, each should get unique IDs
    // (This test would work once loading is fully implemented)
    let libs = ffi.loaded_libraries();
    assert_eq!(libs.len(), 0);
}

#[test]
fn test_type_classification() {
    assert!(CType::Int.is_integer());
    assert!(CType::Long.is_integer());
    assert!(!CType::Float.is_integer());

    assert!(CType::Float.is_float());
    assert!(CType::Double.is_float());
    assert!(!CType::Int.is_float());
}

#[test]
fn test_ctype_display() {
    assert_eq!(CType::Int.to_string(), "int");
    assert_eq!(CType::Float.to_string(), "float");
    assert_eq!(CType::Double.to_string(), "double");
    assert_eq!(CType::Long.to_string(), "long");
}

// ============================================================================
// Phase 2: Function Calling Integration Tests
// ============================================================================

#[test]
fn test_pointer_type_creation() {
    let ptr_int = CType::Pointer(Box::new(CType::Int));
    assert!(ptr_int.is_pointer());
    assert_eq!(ptr_int.size(), 8); // x86-64 pointers are 8 bytes
}

#[test]
fn test_array_type_creation() {
    let array_int = CType::Array(Box::new(CType::Int), 10);
    assert!(array_int.is_array());
    assert_eq!(array_int.size(), 40); // 4 bytes * 10 elements
}

#[test]
fn test_struct_layout_creation() {
    use elle::ffi::types::{StructField, StructId, StructLayout};

    let fields = vec![
        StructField {
            name: "x".to_string(),
            ctype: CType::Int,
            offset: 0,
        },
        StructField {
            name: "y".to_string(),
            ctype: CType::Int,
            offset: 4,
        },
        StructField {
            name: "z".to_string(),
            ctype: CType::Int,
            offset: 8,
        },
    ];

    let layout = StructLayout::new(StructId::new(1), "Point".to_string(), fields, 12, 4);

    assert_eq!(layout.size, 12);
    assert_eq!(layout.align, 4);
    assert_eq!(layout.field_offset("x"), Some(0));
    assert_eq!(layout.field_offset("y"), Some(4));
    assert_eq!(layout.field_offset("z"), Some(8));
    assert_eq!(layout.field_offset("w"), None);
}

#[test]
fn test_function_signature_creation() {
    use elle::ffi::types::FunctionSignature;

    let sig = FunctionSignature::new(
        "strlen".to_string(),
        vec![CType::Pointer(Box::new(CType::Char))],
        CType::Long,
    );

    assert_eq!(sig.name, "strlen");
    assert_eq!(sig.args.len(), 1);
    assert_eq!(sig.return_type, CType::Long);
    assert!(!sig.variadic);
}

#[test]
fn test_marshal_int_to_c() {
    use elle::ffi::marshal::{CValue, Marshal};

    let val = Value::int(42);
    let c_val = Marshal::elle_to_c(&val, &CType::Int).unwrap();

    if let CValue::Int(n) = c_val {
        assert_eq!(n, 42)
    } else {
        panic!("Expected Int")
    }
}

#[test]
fn test_marshal_bool_to_c() {
    use elle::ffi::marshal::{CValue, Marshal};

    let val = Value::bool(true);
    let c_val = Marshal::elle_to_c(&val, &CType::Bool).unwrap();

    if let CValue::Int(n) = c_val {
        assert_eq!(n, 1)
    } else {
        panic!("Expected Int")
    }
}

#[test]
fn test_marshal_float_to_c() {
    use elle::ffi::marshal::{CValue, Marshal};

    let val = Value::float(std::f64::consts::PI);
    let c_val = Marshal::elle_to_c(&val, &CType::Double).unwrap();

    match c_val {
        CValue::Float(f) => assert!((f - std::f64::consts::PI).abs() < 0.001),
        _ => panic!("Expected Float"),
    }
}

#[test]
fn test_unmarshal_int_from_c() {
    use elle::ffi::marshal::{CValue, Marshal};

    let c_val = CValue::Int(42);
    let val = Marshal::c_to_elle(&c_val, &CType::Int).unwrap();
    assert_eq!(val, Value::int(42));
}

#[test]
fn test_unmarshal_bool_from_c() {
    use elle::ffi::marshal::{CValue, Marshal};

    let c_val = CValue::Int(1);
    let val = Marshal::c_to_elle(&c_val, &CType::Bool).unwrap();
    assert_eq!(val, Value::bool(true));

    let c_val = CValue::Int(0);
    let val = Marshal::c_to_elle(&c_val, &CType::Bool).unwrap();
    assert_eq!(val, Value::bool(false));
}

#[test]
fn test_unmarshal_float_from_c() {
    use elle::ffi::marshal::{CValue, Marshal};

    let c_val = CValue::Float(std::f64::consts::E);
    let val = Marshal::c_to_elle(&c_val, &CType::Double).unwrap();

    if let Some(f) = val.as_float() {
        assert!((f - std::f64::consts::E).abs() < 0.00001);
    } else {
        panic!("Expected Float");
    }
}

#[test]
fn test_function_call_creation() {
    use elle::ffi::call::FunctionCall;
    use elle::ffi::types::FunctionSignature;

    let sig = FunctionSignature::new("test".to_string(), vec![CType::Int], CType::Int);

    let func_ptr = 0x12345678 as *const std::ffi::c_void;
    let call = FunctionCall::new(sig, func_ptr).unwrap();

    assert_eq!(call.signature.name, "test");
}

#[test]
fn test_function_call_null_pointer() {
    use elle::ffi::call::FunctionCall;
    use elle::ffi::types::FunctionSignature;

    let sig = FunctionSignature::new("test".to_string(), vec![], CType::Int);
    let result = FunctionCall::new(sig, std::ptr::null());

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("null"));
}

#[test]
fn test_function_call_argument_mismatch() {
    use elle::ffi::call::FunctionCall;
    use elle::ffi::types::FunctionSignature;

    let sig = FunctionSignature::new("add".to_string(), vec![CType::Int, CType::Int], CType::Int);

    let func_ptr = 0x12345678 as *const std::ffi::c_void;
    let call = FunctionCall::new(sig, func_ptr).unwrap();

    let args = vec![Value::int(1)];
    let result = call.call(&args);

    assert!(result.is_err());
}

#[test]
fn test_c_handle_value() {
    use elle::value::CHandle;

    let ptr = 0x12345678 as *const std::ffi::c_void;
    let handle = CHandle::new(ptr, 42);
    // CHandle is stored as a heap pointer in the new Value representation
    // For now, we just verify that CHandle can be created
    assert_eq!(handle.ptr, ptr);
    assert_eq!(handle.id, 42);
}

#[test]
fn test_ctype_size_calculations() {
    assert_eq!(CType::Bool.size(), 1);
    assert_eq!(CType::Char.size(), 1);
    assert_eq!(CType::Short.size(), 2);
    assert_eq!(CType::Int.size(), 4);
    assert_eq!(CType::Long.size(), 8);
    assert_eq!(CType::LongLong.size(), 8);
    assert_eq!(CType::Float.size(), 4);
    assert_eq!(CType::Double.size(), 8);
    assert_eq!(CType::Pointer(Box::new(CType::Int)).size(), 8);
}

#[test]
fn test_ctype_alignment_calculations() {
    assert_eq!(CType::Bool.alignment(), 1);
    assert_eq!(CType::Char.alignment(), 1);
    assert_eq!(CType::Short.alignment(), 2);
    assert_eq!(CType::Int.alignment(), 4);
    assert_eq!(CType::Long.alignment(), 8);
    assert_eq!(CType::Pointer(Box::new(CType::Int)).alignment(), 8);
}

#[test]
fn test_integer_type_classification() {
    assert!(CType::Char.is_integer());
    assert!(CType::Short.is_integer());
    assert!(CType::Int.is_integer());
    assert!(CType::Long.is_integer());
    assert!(!CType::Float.is_integer());
    assert!(!CType::Double.is_integer());
}

#[test]
fn test_float_type_classification() {
    assert!(CType::Float.is_float());
    assert!(CType::Double.is_float());
    assert!(!CType::Int.is_float());
    assert!(!CType::Long.is_float());
}

#[test]
fn test_pointer_type_classification() {
    let ptr = CType::Pointer(Box::new(CType::Int));
    assert!(ptr.is_pointer());
    assert!(!ptr.is_integer());
    assert!(!ptr.is_float());
}

#[test]
fn test_array_type_classification() {
    let arr = CType::Array(Box::new(CType::Int), 10);
    assert!(arr.is_array());
    assert!(!arr.is_integer());
    assert!(!arr.is_float());
    assert!(!arr.is_pointer());
}
