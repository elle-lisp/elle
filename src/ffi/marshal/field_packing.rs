use super::super::types::CType;
use crate::value::Value;

/// Pack a field value into struct bytes at the given offset.
pub fn pack_field(
    bytes: &mut [u8],
    value: &Value,
    offset: usize,
    ctype: &CType,
) -> Result<(), String> {
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
pub fn unpack_field(bytes: &[u8], offset: usize, ctype: &CType) -> Result<Value, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
