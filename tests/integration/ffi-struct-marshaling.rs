use elle::ffi::marshal::{CValue, Marshal};
use elle::ffi::types::{CType, StructField, StructId, StructLayout};
use elle::value::Value;

#[test]
fn test_struct_marshaling_libc_timeval_like() {
    // Simulate struct timeval { long tv_sec; long tv_usec; }
    // On Linux x86-64: 8 bytes (long) + 8 bytes (long) = 16 bytes
    let layout = StructLayout::new(
        StructId::new(10),
        "timeval".to_string(),
        vec![
            StructField {
                name: "tv_sec".to_string(),
                ctype: CType::Long,
                offset: 0,
            },
            StructField {
                name: "tv_usec".to_string(),
                ctype: CType::Long,
                offset: 8,
            },
        ],
        16,
        8,
    );

    // Create Elle representation
    let value = Value::Vector(std::rc::Rc::new(vec![
        Value::Int(1609459200), // 2021-01-01 00:00:00
        Value::Int(500000),     // 500 milliseconds
    ]));

    // Marshal to C
    let cval = Marshal::marshal_struct_with_layout(&value, &layout).unwrap();

    // Verify it's a struct
    match cval {
        CValue::Struct(bytes) => {
            assert_eq!(bytes.len(), 16);

            // Extract values
            let mut sec_bytes = [0u8; 8];
            sec_bytes.copy_from_slice(&bytes[0..8]);
            let sec = i64::from_le_bytes(sec_bytes);

            let mut usec_bytes = [0u8; 8];
            usec_bytes.copy_from_slice(&bytes[8..16]);
            let usec = i64::from_le_bytes(usec_bytes);

            assert_eq!(sec, 1609459200);
            assert_eq!(usec, 500000);
        }
        _ => panic!("Expected CValue::Struct"),
    }
}

#[test]
fn test_struct_marshaling_file_stat_like() {
    // Simulate essential parts of struct stat
    // {dev: ulong, ino: ulong, mode: uint, nlink: uint}
    let layout = StructLayout::new(
        StructId::new(11),
        "stat_minimal".to_string(),
        vec![
            StructField {
                name: "st_dev".to_string(),
                ctype: CType::ULong,
                offset: 0,
            },
            StructField {
                name: "st_ino".to_string(),
                ctype: CType::ULong,
                offset: 8,
            },
            StructField {
                name: "st_mode".to_string(),
                ctype: CType::UInt,
                offset: 16,
            },
            StructField {
                name: "st_nlink".to_string(),
                ctype: CType::UInt,
                offset: 20,
            },
        ],
        24,
        8,
    );

    let value = Value::Vector(std::rc::Rc::new(vec![
        Value::Int(2049),     // st_dev
        Value::Int(12345678), // st_ino
        Value::Int(33188),    // st_mode (regular file, 0644)
        Value::Int(1),        // st_nlink
    ]));

    let cval = Marshal::marshal_struct_with_layout(&value, &layout).unwrap();
    let result = Marshal::unmarshal_struct_with_layout(&cval, &layout).unwrap();

    match result {
        Value::Vector(vec) => {
            assert_eq!(vec.len(), 4);
            assert_eq!(vec[0], Value::Int(2049));
            assert_eq!(vec[1], Value::Int(12345678));
            assert_eq!(vec[2], Value::Int(33188));
            assert_eq!(vec[3], Value::Int(1));
        }
        _ => panic!("Expected Vector"),
    }
}

#[test]
fn test_struct_with_padding() {
    // Test struct with natural padding:
    // struct { char a; int b; char c; }
    // Layout: a(1) + padding(3) + b(4) + c(1) + padding(3) = 12 bytes
    let layout = StructLayout::new(
        StructId::new(12),
        "padded".to_string(),
        vec![
            StructField {
                name: "a".to_string(),
                ctype: CType::Char,
                offset: 0,
            },
            StructField {
                name: "b".to_string(),
                ctype: CType::Int,
                offset: 4,
            },
            StructField {
                name: "c".to_string(),
                ctype: CType::Char,
                offset: 8,
            },
        ],
        12,
        4,
    );

    let value = Value::Vector(std::rc::Rc::new(vec![
        Value::Int(65), // 'A'
        Value::Int(1000),
        Value::Int(66), // 'B'
    ]));

    let cval = Marshal::marshal_struct_with_layout(&value, &layout).unwrap();
    let result = Marshal::unmarshal_struct_with_layout(&cval, &layout).unwrap();

    match result {
        Value::Vector(vec) => {
            assert_eq!(vec[0], Value::Int(65));
            assert_eq!(vec[1], Value::Int(1000));
            assert_eq!(vec[2], Value::Int(66));
        }
        _ => panic!("Expected Vector"),
    }
}

#[test]
fn test_struct_all_basic_types() {
    // Create a struct with all supported basic types
    let layout = StructLayout::new(
        StructId::new(13),
        "comprehensive".to_string(),
        vec![
            StructField {
                name: "b".to_string(),
                ctype: CType::Bool,
                offset: 0,
            },
            StructField {
                name: "c".to_string(),
                ctype: CType::Char,
                offset: 1,
            },
            StructField {
                name: "s".to_string(),
                ctype: CType::Short,
                offset: 2,
            },
            StructField {
                name: "i".to_string(),
                ctype: CType::Int,
                offset: 4,
            },
            StructField {
                name: "l".to_string(),
                ctype: CType::Long,
                offset: 8,
            },
            StructField {
                name: "f".to_string(),
                ctype: CType::Float,
                offset: 16,
            },
            StructField {
                name: "d".to_string(),
                ctype: CType::Double,
                offset: 20,
            },
        ],
        28,
        8,
    );

    let value = Value::Vector(std::rc::Rc::new(vec![
        Value::Bool(true),
        Value::Int(100),
        Value::Int(1000),
        Value::Int(100000),
        Value::Int(10000000),
        Value::Float(1.5),
        Value::Float(std::f64::consts::E),
    ]));

    let cval = Marshal::marshal_struct_with_layout(&value, &layout).unwrap();
    let result = Marshal::unmarshal_struct_with_layout(&cval, &layout).unwrap();

    match result {
        Value::Vector(vec) => {
            assert_eq!(vec.len(), 7);
            assert_eq!(vec[0], Value::Bool(true));
            assert_eq!(vec[1], Value::Int(100));
            assert_eq!(vec[2], Value::Int(1000));
            assert_eq!(vec[3], Value::Int(100000));
            assert_eq!(vec[4], Value::Int(10000000));
            match &vec[5] {
                Value::Float(f) => assert!((f - 1.5).abs() < 0.01),
                _ => panic!("Expected float"),
            }
            match &vec[6] {
                Value::Float(f) => assert!((f - std::f64::consts::E).abs() < 0.01),
                _ => panic!("Expected float"),
            }
        }
        _ => panic!("Expected Vector"),
    }
}

#[test]
fn test_struct_list_to_struct_conversion() {
    // Test that cons lists are properly converted to vector representation
    use elle::value::cons;

    let layout = StructLayout::new(
        StructId::new(14),
        "point".to_string(),
        vec![
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
        ],
        8,
        4,
    );

    // Create a cons list (x . (y . nil))
    let list_value = cons(Value::Int(5), cons(Value::Int(10), Value::Nil));

    // Marshal from cons list
    let cval = Marshal::marshal_struct_with_layout(&list_value, &layout).unwrap();

    // Verify it marshaled correctly
    match cval {
        CValue::Struct(bytes) => {
            assert_eq!(bytes.len(), 8);
            let x = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            let y = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
            assert_eq!(x, 5);
            assert_eq!(y, 10);
        }
        _ => panic!("Expected CValue::Struct"),
    }
}
