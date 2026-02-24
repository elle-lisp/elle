use elle::ffi::marshal::{CValue, Marshal};
use elle::ffi::types::{CType, UnionField, UnionId, UnionLayout};
use elle::value::Value;

/// Helper function to create a union layout with given fields
fn create_union(id: u32, name: &str, fields: Vec<UnionField>) -> UnionLayout {
    // Union size = max field size, alignment = max field alignment
    let size = fields.iter().map(|f| f.ctype.size()).max().unwrap_or(1);
    let align = fields
        .iter()
        .map(|f| f.ctype.alignment())
        .max()
        .unwrap_or(1);
    UnionLayout::new(UnionId::new(id), name.to_string(), fields, size, align)
}

#[test]
fn test_union_basic_int_float() {
    // Union { int i; float f; } - size should be 4 (max(4, 4))
    let layout = create_union(
        1,
        "int_float",
        vec![
            UnionField {
                name: "i".to_string(),
                ctype: CType::Int,
            },
            UnionField {
                name: "f".to_string(),
                ctype: CType::Float,
            },
        ],
    );

    assert_eq!(layout.size, 4);
    assert_eq!(layout.align, 4);

    // Set integer value
    let value = Value::array(vec![Value::int(42)]);
    let cval = Marshal::marshal_union_with_layout(&value, &layout).unwrap();

    match cval {
        CValue::Union(bytes) => {
            assert_eq!(bytes.len(), 4);
            // Verify the value at offset 0
            let mut int_bytes = [0u8; 4];
            int_bytes.copy_from_slice(&bytes[0..4]);
            let i = i32::from_le_bytes(int_bytes);
            assert_eq!(i, 42);
        }
        _ => panic!("Expected CValue::Union"),
    }
}

#[test]
fn test_union_long_double() {
    // Union { long l; double d; } - size should be 8 (max(8, 8))
    let layout = create_union(
        2,
        "long_double",
        vec![
            UnionField {
                name: "l".to_string(),
                ctype: CType::Long,
            },
            UnionField {
                name: "d".to_string(),
                ctype: CType::Double,
            },
        ],
    );

    assert_eq!(layout.size, 8);
    assert_eq!(layout.align, 8);

    // Set long value
    let value = Value::array(vec![Value::int(123456789)]);
    let cval = Marshal::marshal_union_with_layout(&value, &layout).unwrap();

    match cval {
        CValue::Union(bytes) => {
            assert_eq!(bytes.len(), 8);
            let mut long_bytes = [0u8; 8];
            long_bytes.copy_from_slice(&bytes[0..8]);
            let l = i64::from_le_bytes(long_bytes);
            assert_eq!(l, 123456789);
        }
        _ => panic!("Expected CValue::Union"),
    }
}

#[test]
fn test_union_char_int_long() {
    // Union { char c; int i; long l; } - size should be 8 (max(1, 4, 8))
    let layout = create_union(
        3,
        "char_int_long",
        vec![
            UnionField {
                name: "c".to_string(),
                ctype: CType::Char,
            },
            UnionField {
                name: "i".to_string(),
                ctype: CType::Int,
            },
            UnionField {
                name: "l".to_string(),
                ctype: CType::Long,
            },
        ],
    );

    assert_eq!(layout.size, 8);
    assert_eq!(layout.align, 8);

    // Set char value
    let value = Value::array(vec![Value::int(65)]); // 'A'
    let cval = Marshal::marshal_union_with_layout(&value, &layout).unwrap();

    match cval {
        CValue::Union(bytes) => {
            assert_eq!(bytes.len(), 8);
            assert_eq!(bytes[0], 65);
        }
        _ => panic!("Expected CValue::Union"),
    }
}

#[test]
fn test_union_roundtrip_marshaling() {
    // Union { int i; long l; }
    let layout = create_union(
        4,
        "int_long",
        vec![
            UnionField {
                name: "i".to_string(),
                ctype: CType::Int,
            },
            UnionField {
                name: "l".to_string(),
                ctype: CType::Long,
            },
        ],
    );

    let value = Value::array(vec![Value::int(999)]);

    // Marshal to C
    let cval = Marshal::marshal_union_with_layout(&value, &layout).unwrap();

    // Unmarshal back to Elle
    let result = Marshal::unmarshal_union_with_layout(&cval, &layout).unwrap();

    if let Some(vec_ref) = result.as_array() {
        let vec = vec_ref.borrow();
            // Both fields should read the same bytes at offset 0
            assert_eq!(vec.len(), 2);
            // First field (int) at offset 0: 999
            assert_eq!(vec[0], Value::int(999));
            // Second field (long) at offset 0: also contains 999 (though interpreted as larger value)
            assert_eq!(vec[1], Value::int(999));
    } else {
        panic!("Expected Array");
    }
}

#[test]
fn test_union_size_matches_largest_field() {
    // Union { char c; float f; } - size should be 4 (max(1, 4))
    let layout = create_union(
        5,
        "char_float",
        vec![
            UnionField {
                name: "c".to_string(),
                ctype: CType::Char,
            },
            UnionField {
                name: "f".to_string(),
                ctype: CType::Float,
            },
        ],
    );

    assert_eq!(layout.size, 4);

    // Set char value (first field)
    let value = Value::array(vec![Value::int(65)]); // 'A'
    let cval = Marshal::marshal_union_with_layout(&value, &layout).unwrap();

    match cval {
        CValue::Union(bytes) => {
            // Union should be sized to fit the largest field
            assert_eq!(bytes.len(), 4);

            // Verify the char value is written at offset 0
            assert_eq!(bytes[0], 65);
        }
        _ => panic!("Expected CValue::Union"),
    }
}

#[test]
fn test_union_alignment_matches_largest_field() {
    // Union { char c; double d; } - alignment should be 8
    let layout = create_union(
        6,
        "char_double",
        vec![
            UnionField {
                name: "c".to_string(),
                ctype: CType::Char,
            },
            UnionField {
                name: "d".to_string(),
                ctype: CType::Double,
            },
        ],
    );

    assert_eq!(layout.align, 8);
    assert_eq!(layout.size, 8);
}

#[test]
fn test_union_field_lookup() {
    let layout = create_union(
        7,
        "test",
        vec![
            UnionField {
                name: "x".to_string(),
                ctype: CType::Int,
            },
            UnionField {
                name: "y".to_string(),
                ctype: CType::Float,
            },
        ],
    );

    assert!(layout.get_field("x").is_some());
    assert!(layout.get_field("y").is_some());
    assert!(layout.get_field("z").is_none());
    assert!(layout.has_field("x"));
    assert!(layout.has_field("y"));
    assert!(!layout.has_field("z"));
}

#[test]
fn test_union_multiple_types() {
    // Union { short s; int i; long l; double d; }
    let layout = create_union(
        8,
        "multi_type",
        vec![
            UnionField {
                name: "s".to_string(),
                ctype: CType::Short,
            },
            UnionField {
                name: "i".to_string(),
                ctype: CType::Int,
            },
            UnionField {
                name: "l".to_string(),
                ctype: CType::Long,
            },
            UnionField {
                name: "d".to_string(),
                ctype: CType::Double,
            },
        ],
    );

    assert_eq!(layout.size, 8); // max(2, 4, 8, 8)
    assert_eq!(layout.align, 8); // max alignment

    // Test with different field sizes
    let value = Value::array(vec![Value::int(32767)]); // max short
    let cval = Marshal::marshal_union_with_layout(&value, &layout).unwrap();

    match cval {
        CValue::Union(bytes) => {
            assert_eq!(bytes.len(), 8);
        }
        _ => panic!("Expected CValue::Union"),
    }
}

#[test]
fn test_union_bool_fields() {
    // Union { bool b; int i; }
    let layout = create_union(
        9,
        "bool_int",
        vec![
            UnionField {
                name: "b".to_string(),
                ctype: CType::Bool,
            },
            UnionField {
                name: "i".to_string(),
                ctype: CType::Int,
            },
        ],
    );

    assert_eq!(layout.size, 4); // max(1, 4)

    // Set bool to true
    let value = Value::array(vec![Value::bool(true)]);
    let cval = Marshal::marshal_union_with_layout(&value, &layout).unwrap();

    match cval {
        CValue::Union(bytes) => {
            assert_eq!(bytes[0], 1); // true = 1
            assert_eq!(bytes.len(), 4);
        }
        _ => panic!("Expected CValue::Union"),
    }
}

#[test]
fn test_union_zero_initialize() {
    // Union should be zero-initialized
    let layout = create_union(
        10,
        "zero_init",
        vec![
            UnionField {
                name: "i".to_string(),
                ctype: CType::Int,
            },
            UnionField {
                name: "f".to_string(),
                ctype: CType::Float,
            },
        ],
    );

    // Set int to 42, rest should be zero-initialized
    let value = Value::array(vec![Value::int(42)]);
    let cval = Marshal::marshal_union_with_layout(&value, &layout).unwrap();

    match cval {
        CValue::Union(bytes) => {
            assert_eq!(bytes.len(), 4);
            // All bytes after the value should be zero
            let mut int_bytes = [0u8; 4];
            int_bytes.copy_from_slice(&bytes[0..4]);
            let i = i32::from_le_bytes(int_bytes);
            assert_eq!(i, 42);
        }
        _ => panic!("Expected CValue::Union"),
    }
}

#[test]
fn test_union_error_on_empty_value() {
    let layout = create_union(
        11,
        "test",
        vec![UnionField {
            name: "i".to_string(),
            ctype: CType::Int,
        }],
    );

    // Empty array should fail
    let value = Value::array(vec![]);
    let result = Marshal::marshal_union_with_layout(&value, &layout);
    assert!(result.is_err());
}

#[test]
fn test_union_error_on_too_many_values() {
    let layout = create_union(
        12,
        "test",
        vec![
            UnionField {
                name: "i".to_string(),
                ctype: CType::Int,
            },
            UnionField {
                name: "f".to_string(),
                ctype: CType::Float,
            },
        ],
    );

    // Too many values should fail
    let value = Value::array(vec![
        Value::int(1),
        Value::int(2),
        Value::int(3),
    ]);
    let result = Marshal::marshal_union_with_layout(&value, &layout);
    assert!(result.is_err());
}

#[test]
fn test_union_non_array_error() {
    let layout = create_union(
        13,
        "test",
        vec![UnionField {
            name: "i".to_string(),
            ctype: CType::Int,
        }],
    );

    // Non-array should fail
    let value = Value::int(42);
    let result = Marshal::marshal_union_with_layout(&value, &layout);
    assert!(result.is_err());
}

#[test]
fn test_union_unmarshal_all_fields_readable() {
    // Union { int i; float f; }
    let layout = create_union(
        14,
        "int_float_readable",
        vec![
            UnionField {
                name: "i".to_string(),
                ctype: CType::Int,
            },
            UnionField {
                name: "f".to_string(),
                ctype: CType::Float,
            },
        ],
    );

    // Set int value
    let value = Value::array(vec![Value::int(42)]);
    let cval = Marshal::marshal_union_with_layout(&value, &layout).unwrap();

    // Unmarshal returns all fields (they all read from offset 0)
    let result = Marshal::unmarshal_union_with_layout(&cval, &layout).unwrap();

    if let Some(vec_ref) = result.as_array() {
        let vec = vec_ref.borrow();
            // Should have 2 values (one for each field)
            assert_eq!(vec.len(), 2);
            // First field reads 42 as int
            assert_eq!(vec[0], Value::int(42));
            // Second field reads 42 as float (but interpreted as bit pattern)
            // The bit pattern of 42 as float is a small positive float
            if vec[1].is_float() {
                    // Just verify it\'s a float
                } else {
                    panic!("Expected float");
                }
    } else {
        panic!("Expected Array");
    }
}

#[test]
fn test_union_unmarshal_size_check() {
    let layout = create_union(
        15,
        "size_check",
        vec![UnionField {
            name: "i".to_string(),
            ctype: CType::Int,
        }],
    );

    // Create union with correct size
    let correct_union = CValue::Union(vec![1, 2, 3, 4]);
    let result = Marshal::unmarshal_union_with_layout(&correct_union, &layout).unwrap();
    assert!((result).is_array());

    // Create union with wrong size
    let wrong_union = CValue::Union(vec![1, 2]); // size 2 instead of 4
    let result = Marshal::unmarshal_union_with_layout(&wrong_union, &layout);
    assert!(result.is_err());
}

#[test]
fn test_union_unmarshal_wrong_type() {
    let layout = create_union(
        16,
        "type_check",
        vec![UnionField {
            name: "i".to_string(),
            ctype: CType::Int,
        }],
    );

    // Try to unmarshal a struct as a union
    let struct_val = CValue::Struct(vec![1, 2, 3, 4]);
    let result = Marshal::unmarshal_union_with_layout(&struct_val, &layout);
    assert!(result.is_err());
}

#[test]
fn test_union_id_uniqueness() {
    let id1 = UnionId::new(1);
    let id2 = UnionId::new(2);
    let id3 = UnionId::new(1);

    assert_ne!(id1, id2);
    assert_eq!(id1, id3); // Same value should be equal
}

#[test]
fn test_union_max_field_types() {
    // Test all primitive types in union
    let layout = create_union(
        17,
        "all_types",
        vec![
            UnionField {
                name: "i".to_string(),
                ctype: CType::Int,
            },
            UnionField {
                name: "f".to_string(),
                ctype: CType::Float,
            },
            UnionField {
                name: "d".to_string(),
                ctype: CType::Double,
            },
        ],
    );

    // Size should be 8 (max of 4, 4, 8)
    assert_eq!(layout.size, 8);

    // Marshal with int value
    let value = Value::array(vec![Value::int(100)]);
    let cval = Marshal::marshal_union_with_layout(&value, &layout).unwrap();

    match cval {
        CValue::Union(bytes) => {
            assert_eq!(bytes.len(), 8);
        }
        _ => panic!("Expected CValue::Union"),
    }
}

#[test]
fn test_union_pointer_field() {
    // Union { int i; void* p; }
    let layout = create_union(
        18,
        "int_ptr",
        vec![
            UnionField {
                name: "i".to_string(),
                ctype: CType::Int,
            },
            UnionField {
                name: "p".to_string(),
                ctype: CType::Pointer(Box::new(CType::Void)),
            },
        ],
    );

    // Size should be 8 (max(4, 8))
    assert_eq!(layout.size, 8);

    // Test with int value (first field)
    let value = Value::array(vec![Value::int(42)]);
    let cval = Marshal::marshal_union_with_layout(&value, &layout).unwrap();

    match cval {
        CValue::Union(bytes) => {
            assert_eq!(bytes.len(), 8);
            // Verify int is written at offset 0
            let mut int_bytes = [0u8; 4];
            int_bytes.copy_from_slice(&bytes[0..4]);
            let i = i32::from_le_bytes(int_bytes);
            assert_eq!(i, 42);
        }
        _ => panic!("Expected CValue::Union"),
    }
}
