use super::super::types::StructLayout;
use super::cvalue::CValue;
use super::field_packing::{pack_field, unpack_field};
use crate::value::Value;

/// Marshal a struct value to C representation with layout information.
pub fn marshal_struct_with_layout(value: &Value, layout: &StructLayout) -> Result<CValue, String> {
    match value {
        Value::Vector(vec) => {
            // Vector representation: fields in order
            if vec.len() != layout.fields.len() {
                return Err(format!(
                    "Struct {} expects {} fields, got {}",
                    layout.name,
                    layout.fields.len(),
                    vec.len()
                ));
            }

            let mut bytes = vec![0u8; layout.size];

            for (i, field) in layout.fields.iter().enumerate() {
                let field_value = &vec[i];
                pack_field(&mut bytes, field_value, field.offset, &field.ctype)?;
            }

            Ok(CValue::Struct(bytes))
        }
        Value::Cons(_) => {
            // List representation: convert to vector first
            let vec_vals = value.list_to_vec()?;
            let vec_value = Value::Vector(std::rc::Rc::new(vec_vals));
            marshal_struct_with_layout(&vec_value, layout)
        }
        _ => Err(format!(
            "Cannot marshal {:?} as struct {}",
            value, layout.name
        )),
    }
}

/// Marshal a struct value to C representation.
pub fn marshal_struct(_value: &Value) -> Result<CValue, String> {
    // Without layout information, we can't properly marshal struct fields
    Err("Struct marshaling requires struct definition metadata".to_string())
}

/// Unmarshal a C struct to Elle value with layout information.
pub fn unmarshal_struct_with_layout(
    cvalue: &CValue,
    layout: &StructLayout,
) -> Result<Value, String> {
    match cvalue {
        CValue::Struct(bytes) => {
            if bytes.len() != layout.size {
                return Err(format!(
                    "Struct data size mismatch: expected {}, got {}",
                    layout.size,
                    bytes.len()
                ));
            }

            let mut field_values = Vec::new();

            for field in &layout.fields {
                let field_value = unpack_field(bytes, field.offset, &field.ctype)?;
                field_values.push(field_value);
            }

            Ok(Value::Vector(std::rc::Rc::new(field_values)))
        }
        _ => Err(format!(
            "Type mismatch: expected struct {}, got {:?}",
            layout.name, cvalue
        )),
    }
}

/// Unmarshal a C struct to Elle value.
pub fn unmarshal_struct(_cvalue: &CValue) -> Result<Value, String> {
    // Without layout information, we can't properly unmarshal struct fields
    Err("Struct unmarshaling requires struct definition metadata".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::types::{CType, StructField, StructId};

    #[test]
    fn test_struct_marshaling_simple() {
        // Create a simple struct: {x: int, y: int}
        let layout = StructLayout::new(
            StructId::new(1),
            "Point".to_string(),
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

        // Create Elle vector [10, 20]
        let value = Value::Vector(std::rc::Rc::new(vec![Value::Int(10), Value::Int(20)]));

        // Marshal to struct
        let cval = marshal_struct_with_layout(&value, &layout).unwrap();

        // Verify bytes
        match cval {
            CValue::Struct(bytes) => {
                assert_eq!(bytes.len(), 8);
                // First int (10) at offset 0
                let x = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                assert_eq!(x, 10);
                // Second int (20) at offset 4
                let y = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
                assert_eq!(y, 20);
            }
            _ => panic!("Expected CValue::Struct"),
        }
    }

    #[test]
    fn test_struct_unmarshaling_simple() {
        // Create the same struct layout
        let layout = StructLayout::new(
            StructId::new(1),
            "Point".to_string(),
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

        // Create struct bytes manually
        let mut bytes = [0u8; 8];
        bytes[0..4].copy_from_slice(&(10i32).to_le_bytes());
        bytes[4..8].copy_from_slice(&(20i32).to_le_bytes());

        let cval = CValue::Struct(bytes.to_vec());

        // Unmarshal to Elle value
        let val = unmarshal_struct_with_layout(&cval, &layout).unwrap();

        // Verify values
        match val {
            Value::Vector(vec) => {
                assert_eq!(vec.len(), 2);
                assert_eq!(vec[0], Value::Int(10));
                assert_eq!(vec[1], Value::Int(20));
            }
            _ => panic!("Expected Value::Vector"),
        }
    }

    #[test]
    fn test_struct_roundtrip() {
        let layout = StructLayout::new(
            StructId::new(2),
            "Data".to_string(),
            vec![
                StructField {
                    name: "a".to_string(),
                    ctype: CType::Short,
                    offset: 0,
                },
                StructField {
                    name: "b".to_string(),
                    ctype: CType::Int,
                    offset: 4,
                },
                StructField {
                    name: "c".to_string(),
                    ctype: CType::Float,
                    offset: 8,
                },
            ],
            12,
            4,
        );

        // Original values
        let original = Value::Vector(std::rc::Rc::new(vec![
            Value::Int(100),
            Value::Int(5000),
            Value::Float(std::f64::consts::PI),
        ]));

        // Marshal
        let marshaled = marshal_struct_with_layout(&original, &layout).unwrap();

        // Unmarshal
        let unmarshaled = unmarshal_struct_with_layout(&marshaled, &layout).unwrap();

        // Verify
        match unmarshaled {
            Value::Vector(vec) => {
                assert_eq!(vec.len(), 3);
                assert_eq!(vec[0], Value::Int(100));
                assert_eq!(vec[1], Value::Int(5000));
                match &vec[2] {
                    Value::Float(f) => assert!((f - std::f64::consts::PI).abs() < 0.01),
                    _ => panic!("Expected float"),
                }
            }
            _ => panic!("Expected Vector"),
        }
    }

    #[test]
    fn test_struct_with_mixed_types() {
        // Struct with bool, char, int, double
        let layout = StructLayout::new(
            StructId::new(3),
            "Mixed".to_string(),
            vec![
                StructField {
                    name: "flag".to_string(),
                    ctype: CType::Bool,
                    offset: 0,
                },
                StructField {
                    name: "ch".to_string(),
                    ctype: CType::Char,
                    offset: 1,
                },
                StructField {
                    name: "num".to_string(),
                    ctype: CType::Int,
                    offset: 4,
                },
                StructField {
                    name: "val".to_string(),
                    ctype: CType::Double,
                    offset: 8,
                },
            ],
            16,
            8,
        );

        let value = Value::Vector(std::rc::Rc::new(vec![
            Value::Bool(true),
            Value::Int(65), // 'A'
            Value::Int(42),
            Value::Float(std::f64::consts::E),
        ]));

        let cval = marshal_struct_with_layout(&value, &layout).unwrap();
        let result = unmarshal_struct_with_layout(&cval, &layout).unwrap();

        match result {
            Value::Vector(vec) => {
                assert_eq!(vec[0], Value::Bool(true));
                assert_eq!(vec[1], Value::Int(65));
                assert_eq!(vec[2], Value::Int(42));
                match &vec[3] {
                    Value::Float(f) => assert!((f - std::f64::consts::E).abs() < 0.01),
                    _ => panic!("Expected float"),
                }
            }
            _ => panic!("Expected Vector"),
        }
    }

    #[test]
    fn test_struct_field_error_count_mismatch() {
        let layout = StructLayout::new(
            StructId::new(4),
            "Point".to_string(),
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

        // Only provide 1 field instead of 2
        let value = Value::Vector(std::rc::Rc::new(vec![Value::Int(10)]));

        let result = marshal_struct_with_layout(&value, &layout);
        assert!(result.is_err());
    }
}
