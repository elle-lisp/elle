use super::super::types::CType;
use super::array_marshal;
use super::cvalue::CValue;
use super::struct_marshal;
use crate::value::{CHandle, Value};
use std::sync::atomic::{AtomicU32, Ordering};

/// Convert an Elle value to a C representation.
pub fn elle_to_c(value: &Value, ctype: &CType) -> Result<CValue, String> {
    match ctype {
        CType::Bool => match value {
            Value::Bool(b) => Ok(CValue::Int(if *b { 1 } else { 0 })),
            Value::Int(n) => Ok(CValue::Int(if *n != 0 { 1 } else { 0 })),
            Value::Nil => Ok(CValue::Int(0)),
            _ => Err(format!("Cannot convert {:?} to bool", value)),
        },
        CType::Char | CType::SChar | CType::UChar => match value {
            Value::Int(n) => {
                if matches!(ctype, CType::UChar) && *n < 0 {
                    Err(format!(
                        "Cannot convert negative value {} to unsigned char",
                        n
                    ))
                } else {
                    Ok(CValue::Int(*n as i8 as i64))
                }
            }
            _ => Err(format!("Cannot convert {:?} to char", value)),
        },
        CType::Short | CType::UShort => match value {
            Value::Int(n) => {
                if matches!(ctype, CType::UShort) && *n < 0 {
                    Err(format!(
                        "Cannot convert negative value {} to unsigned short",
                        n
                    ))
                } else {
                    Ok(CValue::Int(*n as i16 as i64))
                }
            }
            _ => Err(format!("Cannot convert {:?} to short", value)),
        },
        CType::Int => match value {
            Value::Int(n) => Ok(CValue::Int(*n as i32 as i64)),
            _ => Err(format!("Cannot convert {:?} to int", value)),
        },
        CType::UInt => match value {
            Value::Int(n) => {
                if *n < 0 {
                    Err(format!(
                        "Cannot convert negative value {} to unsigned int",
                        n
                    ))
                } else {
                    Ok(CValue::UInt(*n as u64))
                }
            }
            _ => Err(format!("Cannot convert {:?} to unsigned int", value)),
        },
        CType::Long => match value {
            Value::Int(n) => Ok(CValue::Int(*n)),
            _ => Err(format!("Cannot convert {:?} to long", value)),
        },
        CType::ULong | CType::LongLong | CType::ULongLong => match value {
            Value::Int(n) => {
                if matches!(ctype, CType::ULong | CType::ULongLong) && *n < 0 {
                    Err(format!(
                        "Cannot convert negative value {} to unsigned type",
                        n
                    ))
                } else if matches!(ctype, CType::ULong | CType::ULongLong) {
                    Ok(CValue::UInt(*n as u64))
                } else {
                    Ok(CValue::Int(*n))
                }
            }
            _ => Err(format!("Cannot convert {:?} to long", value)),
        },
        CType::Float => match value {
            Value::Float(f) => Ok(CValue::Float(*f)),
            Value::Int(n) => Ok(CValue::Float(*n as f64)),
            _ => Err(format!("Cannot convert {:?} to float", value)),
        },
        CType::Double => match value {
            Value::Float(f) => Ok(CValue::Float(*f)),
            Value::Int(n) => Ok(CValue::Float(*n as f64)),
            _ => Err(format!("Cannot convert {:?} to double", value)),
        },
        CType::Pointer(_) => match value {
            Value::CHandle(handle) => Ok(CValue::Pointer(handle.ptr)),
            Value::Nil => Ok(CValue::Pointer(std::ptr::null())),
            Value::String(s) => {
                // Allow marshaling strings as char* pointers
                let mut bytes = s.as_ref().as_bytes().to_vec();
                bytes.push(0); // null-terminate
                Ok(CValue::String(bytes))
            }
            _ => Err(format!("Cannot convert {:?} to pointer", value)),
        },
        CType::Enum(_) => match value {
            Value::Int(n) => Ok(CValue::Int(*n)),
            _ => Err(format!("Cannot convert {:?} to enum", value)),
        },
        CType::Void => Err("Cannot marshal void as argument".to_string()),
        CType::Struct(_) => struct_marshal::marshal_struct(value),
        CType::Union(_) => {
            Err("Union marshaling requires layout info - use marshal_union_with_layout".to_string())
        }
        CType::Array(elem_type, count) => array_marshal::marshal_array(value, elem_type, *count),
    }
}

/// Convert a C value back to an Elle value.
pub fn c_to_elle(cvalue: &CValue, ctype: &CType) -> Result<Value, String> {
    match ctype {
        CType::Void => Ok(Value::Nil),
        CType::Bool => match cvalue {
            CValue::Int(n) => Ok(Value::Bool(*n != 0)),
            _ => Err("Type mismatch in unmarshal: expected bool".to_string()),
        },
        CType::Char | CType::SChar | CType::UChar => match cvalue {
            CValue::Int(n) => Ok(Value::Int(*n as i8 as i64)),
            CValue::UInt(n) => Ok(Value::Int(*n as u8 as i64)),
            _ => Err("Type mismatch in unmarshal: expected char".to_string()),
        },
        CType::Short | CType::UShort => match cvalue {
            CValue::Int(n) => Ok(Value::Int(*n as i16 as i64)),
            CValue::UInt(n) => Ok(Value::Int(*n as u16 as i64)),
            _ => Err("Type mismatch in unmarshal: expected short".to_string()),
        },
        CType::Int => match cvalue {
            CValue::Int(n) => Ok(Value::Int(*n as i32 as i64)),
            _ => Err("Type mismatch in unmarshal: expected int".to_string()),
        },
        CType::UInt => match cvalue {
            CValue::UInt(n) => Ok(Value::Int(*n as u32 as i64)),
            CValue::Int(n) => Ok(Value::Int(*n as u32 as i64)),
            _ => Err("Type mismatch in unmarshal: expected unsigned int".to_string()),
        },
        CType::Long => match cvalue {
            CValue::Int(n) => Ok(Value::Int(*n)),
            _ => Err("Type mismatch in unmarshal: expected long".to_string()),
        },
        CType::ULong | CType::LongLong | CType::ULongLong => match cvalue {
            CValue::Int(n) => Ok(Value::Int(*n)),
            CValue::UInt(n) => Ok(Value::Int(*n as i64)),
            _ => Err("Type mismatch in unmarshal: expected long".to_string()),
        },
        CType::Float => match cvalue {
            CValue::Float(f) => Ok(Value::Float(*f as f32 as f64)),
            _ => Err("Type mismatch in unmarshal: expected float".to_string()),
        },
        CType::Double => match cvalue {
            CValue::Float(f) => Ok(Value::Float(*f)),
            _ => Err("Type mismatch in unmarshal: expected double".to_string()),
        },
        CType::Pointer(_) => match cvalue {
            CValue::Pointer(p) => {
                if p.is_null() {
                    Ok(Value::Nil)
                } else {
                    // Generate a unique ID for this handle
                    static HANDLE_ID: AtomicU32 = AtomicU32::new(0);
                    let id = HANDLE_ID.fetch_add(1, Ordering::SeqCst);
                    Ok(Value::CHandle(CHandle::new(*p, id)))
                }
            }
            CValue::String(_) => {
                // String pointer - convert to Elle string
                // Note: In reality, we'd need to dereference and read until null
                // For now, return nil as placeholder
                Ok(Value::Nil)
            }
            _ => Err("Type mismatch in unmarshal: expected pointer".to_string()),
        },
        CType::Enum(_) => match cvalue {
            CValue::Int(n) => Ok(Value::Int(*n)),
            _ => Err("Type mismatch in unmarshal: expected enum".to_string()),
        },
        CType::Struct(_) => struct_marshal::unmarshal_struct(cvalue),
        CType::Union(_) => Err(
            "Union unmarshaling requires layout info - use unmarshal_union_with_layout".to_string(),
        ),
        CType::Array(elem_type, _) => array_marshal::unmarshal_array(cvalue, elem_type),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marshal_int() {
        let val = Value::Int(42);
        let cval = elle_to_c(&val, &CType::Int).unwrap();
        match cval {
            CValue::Int(n) => assert_eq!(n, 42),
            _ => panic!("Wrong type"),
        }
    }

    #[test]
    fn test_marshal_bool_true() {
        let val = Value::Bool(true);
        let cval = elle_to_c(&val, &CType::Bool).unwrap();
        match cval {
            CValue::Int(n) => assert_eq!(n, 1),
            _ => panic!("Wrong type"),
        }
    }

    #[test]
    fn test_marshal_bool_false() {
        let val = Value::Bool(false);
        let cval = elle_to_c(&val, &CType::Bool).unwrap();
        match cval {
            CValue::Int(n) => assert_eq!(n, 0),
            _ => panic!("Wrong type"),
        }
    }

    #[test]
    fn test_marshal_float() {
        let val = Value::Float(std::f64::consts::PI);
        let cval = elle_to_c(&val, &CType::Float).unwrap();
        match cval {
            CValue::Float(f) => assert!((f - std::f64::consts::PI).abs() < 0.01),
            _ => panic!("Wrong type"),
        }
    }

    #[test]
    fn test_unmarshal_int() {
        let cval = CValue::Int(42);
        let val = c_to_elle(&cval, &CType::Int).unwrap();
        assert_eq!(val, Value::Int(42));
    }

    #[test]
    fn test_unmarshal_bool() {
        let cval = CValue::Int(1);
        let val = c_to_elle(&cval, &CType::Bool).unwrap();
        assert_eq!(val, Value::Bool(true));

        let cval = CValue::Int(0);
        let val = c_to_elle(&cval, &CType::Bool).unwrap();
        assert_eq!(val, Value::Bool(false));
    }

    #[test]
    fn test_unmarshal_float() {
        let cval = CValue::Float(std::f64::consts::E);
        let val = c_to_elle(&cval, &CType::Double).unwrap();
        match val {
            Value::Float(f) => assert!((f - std::f64::consts::E).abs() < 0.0001),
            _ => panic!("Wrong type"),
        }
    }

    #[test]
    fn test_marshal_unsigned_int() {
        let val = Value::Int(42);
        let cval = elle_to_c(&val, &CType::UInt).unwrap();
        match cval {
            CValue::UInt(n) => assert_eq!(n, 42),
            _ => panic!("Expected UInt"),
        }
    }

    #[test]
    fn test_marshal_unsigned_int_rejects_negative() {
        let val = Value::Int(-1);
        let result = elle_to_c(&val, &CType::UInt);
        assert!(result.is_err());
    }

    #[test]
    fn test_unmarshal_unsigned_int() {
        let cval = CValue::UInt(100);
        let val = c_to_elle(&cval, &CType::UInt).unwrap();
        assert_eq!(val, Value::Int(100));
    }

    #[test]
    fn test_marshal_unsigned_char() {
        let val = Value::Int(65); // 'A'
        let cval = elle_to_c(&val, &CType::UChar).unwrap();
        match cval {
            CValue::Int(n) => assert_eq!(n as u8, 65),
            _ => panic!("Expected Int"),
        }
    }

    #[test]
    fn test_marshal_unsigned_char_rejects_negative() {
        let val = Value::Int(-5);
        let result = elle_to_c(&val, &CType::UChar);
        assert!(result.is_err());
    }

    #[test]
    fn test_marshal_string_as_pointer() {
        let val = Value::String("hello".into());
        let cval = elle_to_c(&val, &CType::Pointer(Box::new(CType::Char))).unwrap();
        match cval {
            CValue::String(bytes) => {
                assert_eq!(bytes.len(), 6); // "hello\0"
                assert_eq!(bytes[5], 0); // null terminator
            }
            _ => panic!("Expected String"),
        }
    }

    #[test]
    fn test_marshal_float_to_double() {
        let val = Value::Float(2.5);
        let cval = elle_to_c(&val, &CType::Double).unwrap();
        match cval {
            CValue::Float(f) => assert!((f - 2.5).abs() < 0.01),
            _ => panic!("Expected Float"),
        }
    }

    #[test]
    fn test_unmarshal_signed_vs_unsigned() {
        // Test that unsigned types are properly distinguished
        let cval_uint = CValue::UInt(200);
        let val = c_to_elle(&cval_uint, &CType::UInt).unwrap();
        assert_eq!(val, Value::Int(200));
    }

    #[test]
    fn test_error_message_unsigned_char() {
        let val = Value::Int(-1);
        let result = elle_to_c(&val, &CType::UChar);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("negative"));
    }

    #[test]
    fn test_nil_as_null_pointer() {
        let val = Value::Nil;
        let cval = elle_to_c(&val, &CType::Pointer(Box::new(CType::Int))).unwrap();
        match cval {
            CValue::Pointer(p) => assert!(p.is_null()),
            _ => panic!("Expected Pointer"),
        }
    }

    #[test]
    fn test_roundtrip_marshal_unmarshal_floats() {
        // Test roundtrip marshaling of floats
        let original = Value::Float(1.618);
        let marshaled = elle_to_c(&original, &CType::Double).unwrap();
        let unmarshaled = c_to_elle(&marshaled, &CType::Double).unwrap();

        if let Value::Float(f) = unmarshaled {
            assert!((f - 1.618).abs() < 0.0001);
        } else {
            panic!("Expected Value::Float");
        }
    }
}
