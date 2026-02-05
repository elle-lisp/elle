//! Value marshaling between Elle and C types.
//!
//! This module handles conversion of Elle values to C representations
//! and vice versa, supporting all basic types, pointers, structs, and arrays.

use super::types::{CType, StructLayout};
use crate::value::{CHandle, Value};
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

/// A value in C representation - raw bytes that can be passed to C functions.
#[derive(Debug, Clone, PartialEq)]
pub enum CValue {
    /// 64-bit integer (covers all scalar integer types on x86-64)
    Int(i64),
    /// 64-bit unsigned integer
    UInt(u64),
    /// 64-bit float (stored as f64)
    Float(f64),
    /// Opaque pointer to C data
    Pointer(*const c_void),
    /// C string (null-terminated)
    String(Vec<u8>),
    /// Raw struct bytes
    Struct(Vec<u8>),
    /// Array of values
    Array(Vec<CValue>),
}

impl CValue {
    /// Get the raw bytes for this value (for libffi calling).
    pub fn as_raw(&self) -> Vec<u8> {
        match self {
            CValue::Int(n) => n.to_le_bytes().to_vec(),
            CValue::UInt(n) => n.to_le_bytes().to_vec(),
            CValue::Float(f) => f.to_le_bytes().to_vec(),
            CValue::Pointer(p) => (*p as u64).to_le_bytes().to_vec(),
            CValue::String(bytes) => {
                // For C string, return pointer to the data
                let ptr = bytes.as_ptr() as u64;
                ptr.to_le_bytes().to_vec()
            }
            CValue::Struct(bytes) => bytes.clone(),
            CValue::Array(_) => {
                // Arrays are typically passed by pointer
                vec![]
            }
        }
    }
}

/// Pack a field value into struct bytes at the given offset.
fn pack_field(bytes: &mut [u8], value: &Value, offset: usize, ctype: &CType) -> Result<(), String> {
    match ctype {
        CType::Bool => {
            let b = match value {
                Value::Bool(b) => *b as u8,
                Value::Int(n) => {
                    if *n != 0 {
                        1
                    } else {
                        0
                    }
                }
                Value::Nil => 0,
                _ => return Err(format!("Cannot convert {:?} to bool", value)),
            };
            if offset < bytes.len() {
                bytes[offset] = b;
                Ok(())
            } else {
                Err(format!("Field offset {} out of bounds", offset))
            }
        }
        CType::Char | CType::SChar | CType::UChar => {
            let n = match value {
                Value::Int(n) => *n as u8,
                _ => return Err(format!("Cannot convert {:?} to char", value)),
            };
            if offset < bytes.len() {
                bytes[offset] = n;
                Ok(())
            } else {
                Err(format!("Field offset {} out of bounds", offset))
            }
        }
        CType::Short | CType::UShort => {
            let n = match value {
                Value::Int(n) => *n as i16 as u16,
                _ => return Err(format!("Cannot convert {:?} to short", value)),
            };
            if offset + 2 <= bytes.len() {
                bytes[offset..offset + 2].copy_from_slice(&n.to_le_bytes());
                Ok(())
            } else {
                Err(format!("Field offset {} out of bounds", offset))
            }
        }
        CType::Int | CType::UInt => {
            let n = match value {
                Value::Int(n) => *n as i32 as u32,
                _ => return Err(format!("Cannot convert {:?} to int", value)),
            };
            if offset + 4 <= bytes.len() {
                bytes[offset..offset + 4].copy_from_slice(&n.to_le_bytes());
                Ok(())
            } else {
                Err(format!("Field offset {} out of bounds", offset))
            }
        }
        CType::Long | CType::ULong | CType::LongLong | CType::ULongLong => {
            let n = match value {
                Value::Int(n) => *n as u64,
                _ => return Err(format!("Cannot convert {:?} to long", value)),
            };
            if offset + 8 <= bytes.len() {
                bytes[offset..offset + 8].copy_from_slice(&n.to_le_bytes());
                Ok(())
            } else {
                Err(format!("Field offset {} out of bounds", offset))
            }
        }
        CType::Float => {
            let f = match value {
                Value::Float(f) => *f as f32,
                Value::Int(n) => *n as f32,
                _ => return Err(format!("Cannot convert {:?} to float", value)),
            };
            if offset + 4 <= bytes.len() {
                bytes[offset..offset + 4].copy_from_slice(&f.to_le_bytes());
                Ok(())
            } else {
                Err(format!("Field offset {} out of bounds", offset))
            }
        }
        CType::Double => {
            let f = match value {
                Value::Float(f) => *f,
                Value::Int(n) => *n as f64,
                _ => return Err(format!("Cannot convert {:?} to double", value)),
            };
            if offset + 8 <= bytes.len() {
                bytes[offset..offset + 8].copy_from_slice(&f.to_le_bytes());
                Ok(())
            } else {
                Err(format!("Field offset {} out of bounds", offset))
            }
        }
        _ => Err(format!("Unsupported field type in struct: {:?}", ctype)),
    }
}

/// Unpack a field value from struct bytes at the given offset.
fn unpack_field(bytes: &[u8], offset: usize, ctype: &CType) -> Result<Value, String> {
    match ctype {
        CType::Bool => {
            if offset < bytes.len() {
                Ok(Value::Bool(bytes[offset] != 0))
            } else {
                Err(format!("Field offset {} out of bounds", offset))
            }
        }
        CType::Char | CType::SChar | CType::UChar => {
            if offset < bytes.len() {
                Ok(Value::Int(bytes[offset] as i8 as i64))
            } else {
                Err(format!("Field offset {} out of bounds", offset))
            }
        }
        CType::Short | CType::UShort => {
            if offset + 2 <= bytes.len() {
                let mut arr = [0u8; 2];
                arr.copy_from_slice(&bytes[offset..offset + 2]);
                let n = i16::from_le_bytes(arr);
                Ok(Value::Int(n as i64))
            } else {
                Err(format!("Field offset {} out of bounds", offset))
            }
        }
        CType::Int | CType::UInt => {
            if offset + 4 <= bytes.len() {
                let mut arr = [0u8; 4];
                arr.copy_from_slice(&bytes[offset..offset + 4]);
                let n = i32::from_le_bytes(arr);
                Ok(Value::Int(n as i64))
            } else {
                Err(format!("Field offset {} out of bounds", offset))
            }
        }
        CType::Long | CType::ULong | CType::LongLong | CType::ULongLong => {
            if offset + 8 <= bytes.len() {
                let mut arr = [0u8; 8];
                arr.copy_from_slice(&bytes[offset..offset + 8]);
                let n = i64::from_le_bytes(arr);
                Ok(Value::Int(n))
            } else {
                Err(format!("Field offset {} out of bounds", offset))
            }
        }
        CType::Float => {
            if offset + 4 <= bytes.len() {
                let mut arr = [0u8; 4];
                arr.copy_from_slice(&bytes[offset..offset + 4]);
                let f = f32::from_le_bytes(arr);
                Ok(Value::Float(f as f64))
            } else {
                Err(format!("Field offset {} out of bounds", offset))
            }
        }
        CType::Double => {
            if offset + 8 <= bytes.len() {
                let mut arr = [0u8; 8];
                arr.copy_from_slice(&bytes[offset..offset + 8]);
                let f = f64::from_le_bytes(arr);
                Ok(Value::Float(f))
            } else {
                Err(format!("Field offset {} out of bounds", offset))
            }
        }
        _ => Err(format!("Unsupported field type in struct: {:?}", ctype)),
    }
}

/// Marshals Elle values to C representations.
pub struct Marshal;

impl Marshal {
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
            CType::Struct(_) => Self::marshal_struct(value),
            CType::Array(elem_type, count) => Self::marshal_array(value, elem_type, *count),
        }
    }

    /// Marshal a struct value to C representation with layout information.
    pub fn marshal_struct_with_layout(
        value: &Value,
        layout: &StructLayout,
    ) -> Result<CValue, String> {
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
                Self::marshal_struct_with_layout(&vec_value, layout)
            }
            _ => Err(format!(
                "Cannot marshal {:?} as struct {}",
                value, layout.name
            )),
        }
    }

    /// Marshal a struct value to C representation.
    fn marshal_struct(_value: &Value) -> Result<CValue, String> {
        // Without layout information, we can't properly marshal struct fields
        Err("Struct marshaling requires struct definition metadata".to_string())
    }

    /// Marshal an array value to C representation.
    fn marshal_array(value: &Value, elem_type: &CType, _count: usize) -> Result<CValue, String> {
        match value {
            Value::Vector(vec) => {
                let mut elements = Vec::new();
                for elem in vec.iter() {
                    elements.push(Self::elle_to_c(elem, elem_type)?);
                }
                Ok(CValue::Array(elements))
            }
            Value::Cons(cons) => {
                let mut elements = Vec::new();
                let mut current = Some(cons.clone());
                while let Some(cell) = current {
                    elements.push(Self::elle_to_c(&cell.first, elem_type)?);
                    current = match &cell.rest {
                        Value::Cons(c) => Some(c.clone()),
                        Value::Nil => None,
                        _ => None,
                    };
                }
                Ok(CValue::Array(elements))
            }
            _ => Err(format!("Cannot marshal {:?} as array", value)),
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
            CType::Struct(_) => Self::unmarshal_struct(cvalue),
            CType::Array(elem_type, _) => Self::unmarshal_array(cvalue, elem_type),
        }
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
    fn unmarshal_struct(_cvalue: &CValue) -> Result<Value, String> {
        // Without layout information, we can't properly unmarshal struct fields
        Err("Struct unmarshaling requires struct definition metadata".to_string())
    }

    /// Unmarshal a C array to Elle value.
    fn unmarshal_array(cvalue: &CValue, elem_type: &CType) -> Result<Value, String> {
        match cvalue {
            CValue::Array(elements) => {
                let mut result = vec![];
                for elem in elements {
                    result.push(Self::c_to_elle(elem, elem_type)?);
                }
                Ok(Value::Vector(std::rc::Rc::new(result)))
            }
            _ => Err("Type mismatch in unmarshal: expected array".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marshal_int() {
        let val = Value::Int(42);
        let cval = Marshal::elle_to_c(&val, &CType::Int).unwrap();
        match cval {
            CValue::Int(n) => assert_eq!(n, 42),
            _ => panic!("Wrong type"),
        }
    }

    #[test]
    fn test_marshal_bool_true() {
        let val = Value::Bool(true);
        let cval = Marshal::elle_to_c(&val, &CType::Bool).unwrap();
        match cval {
            CValue::Int(n) => assert_eq!(n, 1),
            _ => panic!("Wrong type"),
        }
    }

    #[test]
    fn test_marshal_bool_false() {
        let val = Value::Bool(false);
        let cval = Marshal::elle_to_c(&val, &CType::Bool).unwrap();
        match cval {
            CValue::Int(n) => assert_eq!(n, 0),
            _ => panic!("Wrong type"),
        }
    }

    #[test]
    fn test_marshal_float() {
        let val = Value::Float(std::f64::consts::PI);
        let cval = Marshal::elle_to_c(&val, &CType::Float).unwrap();
        match cval {
            CValue::Float(f) => assert!((f - std::f64::consts::PI).abs() < 0.01),
            _ => panic!("Wrong type"),
        }
    }

    #[test]
    fn test_unmarshal_int() {
        let cval = CValue::Int(42);
        let val = Marshal::c_to_elle(&cval, &CType::Int).unwrap();
        assert_eq!(val, Value::Int(42));
    }

    #[test]
    fn test_unmarshal_bool() {
        let cval = CValue::Int(1);
        let val = Marshal::c_to_elle(&cval, &CType::Bool).unwrap();
        assert_eq!(val, Value::Bool(true));

        let cval = CValue::Int(0);
        let val = Marshal::c_to_elle(&cval, &CType::Bool).unwrap();
        assert_eq!(val, Value::Bool(false));
    }

    #[test]
    fn test_unmarshal_float() {
        let cval = CValue::Float(std::f64::consts::E);
        let val = Marshal::c_to_elle(&cval, &CType::Double).unwrap();
        match val {
            Value::Float(f) => assert!((f - std::f64::consts::E).abs() < 0.0001),
            _ => panic!("Wrong type"),
        }
    }

    #[test]
    fn test_marshal_enum() {
        use super::super::types::EnumId;
        let val = Value::Int(5);
        let enum_type = CType::Enum(EnumId::new(1));
        let cval = Marshal::elle_to_c(&val, &enum_type).unwrap();
        match cval {
            CValue::Int(n) => assert_eq!(n, 5),
            _ => panic!("Wrong type"),
        }
    }

    #[test]
    fn test_unmarshal_enum() {
        use super::super::types::EnumId;
        let cval = CValue::Int(10);
        let enum_type = CType::Enum(EnumId::new(1));
        let val = Marshal::c_to_elle(&cval, &enum_type).unwrap();
        assert_eq!(val, Value::Int(10));
    }

    #[test]
    fn test_marshal_unsigned_int() {
        let val = Value::Int(42);
        let cval = Marshal::elle_to_c(&val, &CType::UInt).unwrap();
        match cval {
            CValue::UInt(n) => assert_eq!(n, 42),
            _ => panic!("Expected UInt"),
        }
    }

    #[test]
    fn test_marshal_unsigned_int_rejects_negative() {
        let val = Value::Int(-1);
        let result = Marshal::elle_to_c(&val, &CType::UInt);
        assert!(result.is_err());
    }

    #[test]
    fn test_unmarshal_unsigned_int() {
        let cval = CValue::UInt(100);
        let val = Marshal::c_to_elle(&cval, &CType::UInt).unwrap();
        assert_eq!(val, Value::Int(100));
    }

    #[test]
    fn test_marshal_unsigned_char() {
        let val = Value::Int(65); // 'A'
        let cval = Marshal::elle_to_c(&val, &CType::UChar).unwrap();
        match cval {
            CValue::Int(n) => assert_eq!(n as u8, 65),
            _ => panic!("Expected Int"),
        }
    }

    #[test]
    fn test_marshal_unsigned_char_rejects_negative() {
        let val = Value::Int(-5);
        let result = Marshal::elle_to_c(&val, &CType::UChar);
        assert!(result.is_err());
    }

    #[test]
    fn test_marshal_string_as_pointer() {
        let val = Value::String("hello".into());
        let cval = Marshal::elle_to_c(&val, &CType::Pointer(Box::new(CType::Char))).unwrap();
        match cval {
            CValue::String(bytes) => {
                assert_eq!(bytes.len(), 6); // "hello\0"
                assert_eq!(bytes[5], 0); // null terminator
            }
            _ => panic!("Expected String"),
        }
    }

    #[test]
    fn test_marshal_vector_as_array() {
        let val = Value::Vector(std::rc::Rc::new(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
        ]));
        let cval = Marshal::elle_to_c(&val, &CType::Array(Box::new(CType::Int), 3)).unwrap();
        match cval {
            CValue::Array(elems) => {
                assert_eq!(elems.len(), 3);
                match &elems[0] {
                    CValue::Int(n) => assert_eq!(*n, 1),
                    _ => panic!("Expected Int"),
                }
            }
            _ => panic!("Expected Array"),
        }
    }

    #[test]
    fn test_marshal_cons_as_array() {
        use crate::value::cons;
        let list = cons(
            Value::Int(10),
            cons(Value::Int(20), cons(Value::Int(30), Value::Nil)),
        );
        let cval = Marshal::elle_to_c(&list, &CType::Array(Box::new(CType::Int), 3)).unwrap();
        match cval {
            CValue::Array(elems) => {
                assert_eq!(elems.len(), 3);
            }
            _ => panic!("Expected Array"),
        }
    }

    #[test]
    fn test_unmarshal_array_to_vector() {
        let cval = CValue::Array(vec![CValue::Int(5), CValue::Int(10), CValue::Int(15)]);
        let val = Marshal::c_to_elle(&cval, &CType::Array(Box::new(CType::Int), 3)).unwrap();
        match val {
            Value::Vector(vec) => {
                assert_eq!(vec.len(), 3);
                assert_eq!(vec[0], Value::Int(5));
                assert_eq!(vec[1], Value::Int(10));
                assert_eq!(vec[2], Value::Int(15));
            }
            _ => panic!("Expected Vector"),
        }
    }

    #[test]
    fn test_marshal_float_to_double() {
        let val = Value::Float(2.5);
        let cval = Marshal::elle_to_c(&val, &CType::Double).unwrap();
        match cval {
            CValue::Float(f) => assert!((f - 2.5).abs() < 0.01),
            _ => panic!("Expected Float"),
        }
    }

    #[test]
    fn test_unmarshal_signed_vs_unsigned() {
        // Test that unsigned types are properly distinguished
        let cval_uint = CValue::UInt(200);
        let val = Marshal::c_to_elle(&cval_uint, &CType::UInt).unwrap();
        assert_eq!(val, Value::Int(200));
    }

    #[test]
    fn test_error_message_unsigned_char() {
        let val = Value::Int(-1);
        let result = Marshal::elle_to_c(&val, &CType::UChar);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("negative"));
    }

    #[test]
    fn test_nil_as_null_pointer() {
        let val = Value::Nil;
        let cval = Marshal::elle_to_c(&val, &CType::Pointer(Box::new(CType::Int))).unwrap();
        match cval {
            CValue::Pointer(p) => assert!(p.is_null()),
            _ => panic!("Expected Pointer"),
        }
    }

    #[test]
    fn test_roundtrip_marshal_unmarshal_floats() {
        // Test roundtrip marshaling of floats
        let original = Value::Float(1.618);
        let marshaled = Marshal::elle_to_c(&original, &CType::Double).unwrap();
        let unmarshaled = Marshal::c_to_elle(&marshaled, &CType::Double).unwrap();

        if let Value::Float(f) = unmarshaled {
            assert!((f - 1.618).abs() < 0.0001);
        } else {
            panic!("Expected Value::Float");
        }
    }

    #[test]
    fn test_struct_marshaling_simple() {
        use crate::ffi::types::{StructField, StructId};

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
        let cval = Marshal::marshal_struct_with_layout(&value, &layout).unwrap();

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
        use crate::ffi::types::{StructField, StructId};

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
        let val = Marshal::unmarshal_struct_with_layout(&cval, &layout).unwrap();

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
        use crate::ffi::types::{StructField, StructId};

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
        let marshaled = Marshal::marshal_struct_with_layout(&original, &layout).unwrap();

        // Unmarshal
        let unmarshaled = Marshal::unmarshal_struct_with_layout(&marshaled, &layout).unwrap();

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
        use crate::ffi::types::{StructField, StructId};

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

        let cval = Marshal::marshal_struct_with_layout(&value, &layout).unwrap();
        let result = Marshal::unmarshal_struct_with_layout(&cval, &layout).unwrap();

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
        use crate::ffi::types::{StructField, StructId};

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

        let result = Marshal::marshal_struct_with_layout(&value, &layout);
        assert!(result.is_err());
    }

    #[test]
    fn test_pack_field_bounds_check() {
        let mut bytes = vec![0u8; 4];
        // Trying to pack at offset 2 with 4-byte int should fail
        let result = pack_field(&mut bytes, &Value::Int(42), 2, &CType::Int);
        assert!(result.is_err());
    }

    #[test]
    fn test_unpack_field_bounds_check() {
        let bytes = vec![0u8; 4];
        // Trying to unpack at offset 2 with 4-byte int should fail
        let result = unpack_field(&bytes, 2, &CType::Int);
        assert!(result.is_err());
    }
}
