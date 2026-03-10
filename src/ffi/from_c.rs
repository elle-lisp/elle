//! C → Value unmarshalling: read_value_from_buffer.
//!
//! Converts C-typed data back into Elle Values.

use crate::error::{LError, LResult};
use crate::ffi::types::TypeDesc;
use crate::value::Value;
use std::ffi::c_void;

/// Read a C value from a buffer at the given pointer, returning an Elle Value.
///
/// For struct/array types, returns an Elle array of field/element values.
///
/// # Safety
/// `ptr` must point to a readable region of at least `desc.size()` bytes
/// with appropriate alignment and valid data for the described type.
pub(crate) fn read_value_from_buffer(ptr: *const u8, desc: &TypeDesc) -> LResult<Value> {
    match desc {
        TypeDesc::Void => Ok(Value::NIL),

        TypeDesc::Bool => {
            let v = unsafe { *(ptr as *const std::ffi::c_int) };
            Ok(Value::bool(v != 0))
        }

        TypeDesc::I8 | TypeDesc::Char => {
            let v = unsafe { *(ptr as *const i8) };
            Ok(Value::int(v as i64))
        }
        TypeDesc::U8 | TypeDesc::UChar => {
            let v = unsafe { *ptr };
            Ok(Value::int(v as i64))
        }
        TypeDesc::I16 | TypeDesc::Short => {
            let v = unsafe { *(ptr as *const i16) };
            Ok(Value::int(v as i64))
        }
        TypeDesc::U16 | TypeDesc::UShort => {
            let v = unsafe { *(ptr as *const u16) };
            Ok(Value::int(v as i64))
        }
        TypeDesc::I32 | TypeDesc::Int => {
            let v = unsafe { *(ptr as *const i32) };
            Ok(Value::int(v as i64))
        }
        TypeDesc::U32 | TypeDesc::UInt => {
            let v = unsafe { *(ptr as *const u32) };
            Ok(Value::int(v as i64))
        }
        TypeDesc::I64 | TypeDesc::Long | TypeDesc::SSize => {
            let v = unsafe { *(ptr as *const i64) };
            Ok(Value::int(v))
        }
        TypeDesc::U64 | TypeDesc::ULong | TypeDesc::Size => {
            let v = unsafe { *(ptr as *const u64) };
            Ok(Value::int(v as i64))
        }

        TypeDesc::Float => {
            let v = unsafe { *(ptr as *const f32) };
            Ok(Value::float(v as f64))
        }
        TypeDesc::Double => {
            let v = unsafe { *(ptr as *const f64) };
            Ok(Value::float(v))
        }

        TypeDesc::Ptr | TypeDesc::Str => {
            let v = unsafe { *(ptr as *const *const c_void) };
            Ok(Value::pointer(v as usize))
        }

        TypeDesc::Struct(sd) => {
            let (offsets, _) = sd.field_offsets().ok_or_else(|| {
                LError::ffi_error("marshal", "cannot compute struct layout for read")
            })?;
            let mut values = Vec::with_capacity(sd.fields.len());
            for (field_desc, &field_offset) in sd.fields.iter().zip(offsets.iter()) {
                let field_val =
                    read_value_from_buffer(unsafe { ptr.add(field_offset) }, field_desc)?;
                values.push(field_val);
            }
            Ok(Value::array_mut(values))
        }

        TypeDesc::Array(elem_desc, count) => {
            let elem_size = elem_desc.size().ok_or_else(|| {
                LError::ffi_error("marshal", "cannot compute array element size for read")
            })?;
            let mut values = Vec::with_capacity(*count);
            for i in 0..*count {
                let elem_val =
                    read_value_from_buffer(unsafe { ptr.add(i * elem_size) }, elem_desc)?;
                values.push(elem_val);
            }
            Ok(Value::array_mut(values))
        }
    }
}
