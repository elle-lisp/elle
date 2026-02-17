use super::super::types::CType;
use super::array_marshal;
use super::cvalue::CValue;
use super::struct_marshal;
use crate::error::{LError, LResult};
use crate::value::Value;
use std::sync::atomic::{AtomicU32, Ordering};

/// Convert an Elle value to a C representation.
pub fn elle_to_c(value: &Value, ctype: &CType) -> LResult<CValue> {
    match ctype {
        CType::Bool => {
            if let Some(b) = value.as_bool() {
                Ok(CValue::Int(if b { 1 } else { 0 }))
            } else if let Some(n) = value.as_int() {
                Ok(CValue::Int(if n != 0 { 1 } else { 0 }))
            } else if value.is_nil() || value.is_empty_list() {
                Ok(CValue::Int(0))
            } else {
                Err(LError::from(format!("Cannot convert {:?} to bool", value)))
            }
        }
        CType::Char | CType::SChar | CType::UChar => {
            if let Some(n) = value.as_int() {
                if matches!(ctype, CType::UChar) && n < 0 {
                    Err(LError::from(format!(
                        "Cannot convert negative value {} to unsigned char",
                        n
                    )))
                } else {
                    Ok(CValue::Int(n as i8 as i64))
                }
            } else {
                Err(LError::from(format!("Cannot convert {:?} to char", value)))
            }
        }
        CType::Short | CType::UShort => {
            if let Some(n) = value.as_int() {
                if matches!(ctype, CType::UShort) && n < 0 {
                    Err(LError::from(format!(
                        "Cannot convert negative value {} to unsigned short",
                        n
                    )))
                } else {
                    Ok(CValue::Int(n as i16 as i64))
                }
            } else {
                Err(LError::from(format!("Cannot convert {:?} to short", value)))
            }
        }
        CType::Int => {
            if let Some(n) = value.as_int() {
                Ok(CValue::Int(n as i32 as i64))
            } else {
                Err(LError::from(format!("Cannot convert {:?} to int", value)))
            }
        }
        CType::UInt => {
            if let Some(n) = value.as_int() {
                if n < 0 {
                    Err(LError::from(format!(
                        "Cannot convert negative value {} to unsigned int",
                        n
                    )))
                } else {
                    Ok(CValue::UInt(n as u64))
                }
            } else {
                Err(LError::from(format!(
                    "Cannot convert {:?} to unsigned int",
                    value
                )))
            }
        }
        CType::Long => {
            if let Some(n) = value.as_int() {
                Ok(CValue::Int(n))
            } else {
                Err(LError::from(format!("Cannot convert {:?} to long", value)))
            }
        }
        CType::ULong | CType::LongLong | CType::ULongLong => {
            if let Some(n) = value.as_int() {
                if matches!(ctype, CType::ULong | CType::ULongLong) && n < 0 {
                    Err(LError::from(format!(
                        "Cannot convert negative value {} to unsigned type",
                        n
                    )))
                } else if matches!(ctype, CType::ULong | CType::ULongLong) {
                    Ok(CValue::UInt(n as u64))
                } else {
                    Ok(CValue::Int(n))
                }
            } else {
                Err(LError::from(format!("Cannot convert {:?} to long", value)))
            }
        }
        CType::Float => {
            if let Some(f) = value.as_float() {
                Ok(CValue::Float(f))
            } else if let Some(n) = value.as_int() {
                Ok(CValue::Float(n as f64))
            } else {
                Err(LError::from(format!("Cannot convert {:?} to float", value)))
            }
        }
        CType::Double => {
            if let Some(f) = value.as_float() {
                Ok(CValue::Float(f))
            } else if let Some(n) = value.as_int() {
                Ok(CValue::Float(n as f64))
            } else {
                Err(LError::from(format!(
                    "Cannot convert {:?} to double",
                    value
                )))
            }
        }
        CType::Pointer(_) => {
            if let Some(s) = value.as_string() {
                // Allow marshaling strings as char* pointers
                let mut bytes = s.as_bytes().to_vec();
                bytes.push(0); // null-terminate
                Ok(CValue::String(bytes))
            } else if let Some(handle) = value.as_heap_ptr() {
                // CHandle is stored as a heap pointer
                Ok(CValue::Pointer(handle as *const std::ffi::c_void))
            } else if value.is_nil() || value.is_empty_list() {
                Ok(CValue::Pointer(std::ptr::null()))
            } else {
                Err(LError::from(format!(
                    "Cannot convert {:?} to pointer",
                    value
                )))
            }
        }
        CType::Enum(_) => {
            if let Some(n) = value.as_int() {
                Ok(CValue::Int(n))
            } else {
                Err(LError::from(format!("Cannot convert {:?} to enum", value)))
            }
        }
        CType::Void => Err(LError::from("Cannot marshal void as argument")),
        CType::Struct(_) => struct_marshal::marshal_struct(value).map_err(LError::from),
        CType::Union(_) => Err(LError::from(
            "Union marshaling requires layout info - use marshal_union_with_layout",
        )),
        CType::Array(elem_type, count) => {
            array_marshal::marshal_array(value, elem_type, *count).map_err(LError::from)
        }
    }
}

/// Convert a C value back to an Elle value.
pub fn c_to_elle(cvalue: &CValue, ctype: &CType) -> LResult<Value> {
    match ctype {
        CType::Void => Ok(Value::NIL),
        CType::Bool => match cvalue {
            CValue::Int(n) => Ok(Value::bool(*n != 0)),
            _ => Err(LError::from("Type mismatch in unmarshal: expected bool")),
        },
        CType::Char | CType::SChar | CType::UChar => match cvalue {
            CValue::Int(n) => Ok(Value::int(*n as i8 as i64)),
            CValue::UInt(n) => Ok(Value::int(*n as u8 as i64)),
            _ => Err(LError::from("Type mismatch in unmarshal: expected char")),
        },
        CType::Short | CType::UShort => match cvalue {
            CValue::Int(n) => Ok(Value::int(*n as i16 as i64)),
            CValue::UInt(n) => Ok(Value::int(*n as u16 as i64)),
            _ => Err(LError::from("Type mismatch in unmarshal: expected short")),
        },
        CType::Int => match cvalue {
            CValue::Int(n) => Ok(Value::int(*n as i32 as i64)),
            _ => Err(LError::from("Type mismatch in unmarshal: expected int")),
        },
        CType::UInt => match cvalue {
            CValue::UInt(n) => Ok(Value::int(*n as u32 as i64)),
            CValue::Int(n) => Ok(Value::int(*n as u32 as i64)),
            _ => Err(LError::from(
                "Type mismatch in unmarshal: expected unsigned int",
            )),
        },
        CType::Long => match cvalue {
            CValue::Int(n) => Ok(Value::int(*n)),
            _ => Err(LError::from("Type mismatch in unmarshal: expected long")),
        },
        CType::ULong | CType::LongLong | CType::ULongLong => match cvalue {
            CValue::Int(n) => Ok(Value::int(*n)),
            CValue::UInt(n) => Ok(Value::int(*n as i64)),
            _ => Err(LError::from("Type mismatch in unmarshal: expected long")),
        },
        CType::Float => match cvalue {
            CValue::Float(f) => Ok(Value::float(*f as f32 as f64)),
            _ => Err(LError::from("Type mismatch in unmarshal: expected float")),
        },
        CType::Double => match cvalue {
            CValue::Float(f) => Ok(Value::float(*f)),
            _ => Err(LError::from("Type mismatch in unmarshal: expected double")),
        },
        CType::Pointer(_) => match cvalue {
            CValue::Pointer(p) => {
                if p.is_null() {
                    Ok(Value::NIL)
                } else {
                    // Generate a unique ID for this handle
                    static HANDLE_ID: AtomicU32 = AtomicU32::new(0);
                    let _id = HANDLE_ID.fetch_add(1, Ordering::SeqCst);
                    Ok(Value::from_heap_ptr(*p as *const ()))
                }
            }
            CValue::String(_) => {
                // String pointer - convert to Elle string
                // Note: In reality, we'd need to dereference and read until null
                // For now, return nil as placeholder
                Ok(Value::NIL)
            }
            _ => Err(LError::from("Type mismatch in unmarshal: expected pointer")),
        },
        CType::Enum(_) => match cvalue {
            CValue::Int(n) => Ok(Value::int(*n)),
            _ => Err(LError::from("Type mismatch in unmarshal: expected enum")),
        },
        CType::Struct(_) => struct_marshal::unmarshal_struct(cvalue).map_err(LError::from),
        CType::Union(_) => Err(LError::from(
            "Union unmarshaling requires layout info - use unmarshal_union_with_layout",
        )),
        CType::Array(elem_type, _) => {
            array_marshal::unmarshal_array(cvalue, elem_type).map_err(LError::from)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marshal_int() {
        let val = Value::int(42);
        let cval = elle_to_c(&val, &CType::Int).unwrap();
        match cval {
            CValue::Int(n) => assert_eq!(n, 42),
            _ => panic!("Wrong type"),
        }
    }

    #[test]
    fn test_marshal_bool_true() {
        let val = Value::bool(true);
        let cval = elle_to_c(&val, &CType::Bool).unwrap();
        match cval {
            CValue::Int(n) => assert_eq!(n, 1),
            _ => panic!("Wrong type"),
        }
    }

    #[test]
    fn test_marshal_bool_false() {
        let val = Value::bool(false);
        let cval = elle_to_c(&val, &CType::Bool).unwrap();
        match cval {
            CValue::Int(n) => assert_eq!(n, 0),
            _ => panic!("Wrong type"),
        }
    }

    #[test]
    fn test_marshal_float() {
        let val = Value::float(std::f64::consts::PI);
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
        assert_eq!(val, Value::int(42));
    }

    #[test]
    fn test_unmarshal_bool() {
        let cval = CValue::Int(1);
        let val = c_to_elle(&cval, &CType::Bool).unwrap();
        assert_eq!(val, Value::bool(true));

        let cval = CValue::Int(0);
        let val = c_to_elle(&cval, &CType::Bool).unwrap();
        assert_eq!(val, Value::bool(false));
    }

    #[test]
    fn test_unmarshal_float() {
        let cval = CValue::Float(std::f64::consts::E);
        let val = c_to_elle(&cval, &CType::Double).unwrap();
        if let Some(f) = val.as_float() {
            assert!((f - std::f64::consts::E).abs() < 0.0001);
        } else {
            panic!("Wrong type");
        }
    }

    #[test]
    fn test_marshal_unsigned_int() {
        let val = Value::int(42);
        let cval = elle_to_c(&val, &CType::UInt).unwrap();
        match cval {
            CValue::UInt(n) => assert_eq!(n, 42),
            _ => panic!("Expected UInt"),
        }
    }

    #[test]
    fn test_marshal_unsigned_int_rejects_negative() {
        let val = Value::int(-1);
        let result = elle_to_c(&val, &CType::UInt);
        assert!(result.is_err());
    }

    #[test]
    fn test_unmarshal_unsigned_int() {
        let cval = CValue::UInt(100);
        let val = c_to_elle(&cval, &CType::UInt).unwrap();
        assert_eq!(val, Value::int(100));
    }

    #[test]
    fn test_marshal_unsigned_char() {
        let val = Value::int(65); // 'A'
        let cval = elle_to_c(&val, &CType::UChar).unwrap();
        match cval {
            CValue::Int(n) => assert_eq!(n as u8, 65),
            _ => panic!("Expected Int"),
        }
    }

    #[test]
    fn test_marshal_unsigned_char_rejects_negative() {
        let val = Value::int(-5);
        let result = elle_to_c(&val, &CType::UChar);
        assert!(result.is_err());
    }

    #[test]
    fn test_marshal_string_as_pointer() {
        let val = Value::string("hello");
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
        let val = Value::float(2.5);
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
        assert_eq!(val, Value::int(200));
    }

    #[test]
    fn test_error_message_unsigned_char() {
        let val = Value::int(-1);
        let result = elle_to_c(&val, &CType::UChar);
        assert!(result.is_err());
        assert!(result.unwrap_err().description().contains("negative"));
    }

    #[test]
    fn test_nil_as_null_pointer() {
        let val = Value::NIL;
        let cval = elle_to_c(&val, &CType::Pointer(Box::new(CType::Int))).unwrap();
        match cval {
            CValue::Pointer(p) => assert!(p.is_null()),
            _ => panic!("Expected Pointer"),
        }
    }

    #[test]
    fn test_roundtrip_marshal_unmarshal_floats() {
        // Test roundtrip marshaling of floats
        let original = Value::float(1.618);
        let marshaled = elle_to_c(&original, &CType::Double).unwrap();
        let unmarshaled = c_to_elle(&marshaled, &CType::Double).unwrap();

        if let Some(f) = unmarshaled.as_float() {
            assert!((f - 1.618).abs() < 0.0001);
        } else {
            panic!("Expected float value");
        }
    }
}
