// audited: 2026-10-13
//! `write_value_to_buffer`: write a value into caller-provided C memory of a described type.
//!
//! docs/ffi.md
//!
//! This is the counterpart of `MarshalledArg::new` for memory the caller owns:
//! a struct field, an array element, `ffi/write`, or a callback's struct result.

use super::{ArgStorage, MarshalledArg};
use crate::error::{LError, LResult};
use crate::ffi::scalar::write_scalar;
use crate::ffi::types::TypeDesc;
use crate::value::Value;
use std::ffi::CString;

/// Write a single Elle Value into a C buffer at the given pointer.
///
/// Returns owned data (MarshalledArgs) that must outlive the buffer —
/// this is needed for CString fields whose pointers are written into
/// the buffer.
///
/// # Safety
/// `ptr` must point to a writable region of at least `desc.size()` bytes
/// with appropriate alignment.
pub fn write_value_to_buffer(
    ptr: *mut u8,
    value: &Value,
    desc: &TypeDesc,
) -> LResult<Vec<MarshalledArg>> {
    match desc {
        TypeDesc::Void => Err(LError::ffi_error("marshal", "cannot write void to buffer")),

        TypeDesc::Str => {
            // nil → NULL pointer
            if value.is_nil() {
                unsafe { *(ptr as *mut *const std::ffi::c_char) = std::ptr::null() };
                return Ok(Vec::new());
            }
            // Create a CString, write its pointer into the buffer, and
            // return a MarshalledArg that owns the CString.
            let s = value.with_string(|s| s.to_string()).ok_or_else(|| {
                LError::ffi_type_error(
                    "string",
                    format!("expected string or nil, got {}", value.type_name()),
                )
            })?;
            let cstring = CString::new(s.as_str())
                .map_err(|_| LError::ffi_type_error("string", "contains interior null byte"))?;
            let cstr_ptr = cstring.as_ptr();
            unsafe { *(ptr as *mut *const std::ffi::c_char) = cstr_ptr };
            // The CString must outlive the buffer. Wrap it in a MarshalledArg.
            let owned = MarshalledArg {
                storage: ArgStorage::Str(cstring, cstr_ptr),
            };
            Ok(vec![owned])
        }

        TypeDesc::Struct(sd) => {
            let (offsets, _) = sd.field_offsets().ok_or_else(|| {
                LError::ffi_error("marshal", "cannot compute struct layout (contains void?)")
            })?;

            // Accept both mutable @[...] and immutable [...] arrays.
            let write_fields = |elems: &[Value]| -> LResult<Vec<MarshalledArg>> {
                if elems.len() != sd.fields.len() {
                    return Err(LError::ffi_type_error(
                        "struct",
                        format!(
                            "struct has {} fields, got {} values",
                            sd.fields.len(),
                            elems.len()
                        ),
                    ));
                }
                let mut owned = Vec::new();
                for (i, (field_desc, &field_offset)) in
                    sd.fields.iter().zip(offsets.iter()).enumerate()
                {
                    let field_owned = write_value_to_buffer(
                        unsafe { ptr.add(field_offset) },
                        &elems[i],
                        field_desc,
                    )?;
                    owned.extend(field_owned);
                }
                Ok(owned)
            };

            if let Some(arr) = value.as_array_mut() {
                let elems = arr.borrow();
                write_fields(&elems)
            } else if let Some(elems) = value.as_array() {
                write_fields(elems)
            } else {
                Err(LError::ffi_type_error(
                    "struct",
                    format!("expected array, got {}", value.type_name()),
                ))
            }
        }

        TypeDesc::Array(elem_desc, count) => {
            // Accept mutable @[...], immutable [...] arrays, and bytes/\@bytes
            // (for u8/i8 element types, bytes are written directly as a fast path).
            let elem_size = elem_desc
                .size()
                .ok_or_else(|| LError::ffi_error("marshal", "cannot compute array element size"))?;

            // Fast path: bytes value for u8/i8 array — copy directly.
            if matches!(
                **elem_desc,
                TypeDesc::U8 | TypeDesc::UChar | TypeDesc::I8 | TypeDesc::Char
            ) {
                if let Some(data) = value.as_bytes() {
                    if data.len() != *count {
                        return Err(LError::ffi_type_error(
                            "array",
                            format!("array has {} elements, got {} bytes", count, data.len()),
                        ));
                    }
                    unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len()) };
                    return Ok(Vec::new());
                }
                if let Some(cell) = value.as_bytes_mut() {
                    let data = cell.borrow();
                    if data.len() != *count {
                        return Err(LError::ffi_type_error(
                            "array",
                            format!("array has {} elements, got {} bytes", count, data.len()),
                        ));
                    }
                    unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len()) };
                    return Ok(Vec::new());
                }
            }

            // General path: array values.
            let write_elems = |elems: &[Value]| -> LResult<Vec<MarshalledArg>> {
                if elems.len() != *count {
                    return Err(LError::ffi_type_error(
                        "array",
                        format!("array has {} elements, got {} values", count, elems.len()),
                    ));
                }
                let mut owned = Vec::new();
                for (i, elem_val) in elems.iter().enumerate() {
                    let elem_owned = write_value_to_buffer(
                        unsafe { ptr.add(i * elem_size) },
                        elem_val,
                        elem_desc,
                    )?;
                    owned.extend(elem_owned);
                }
                Ok(owned)
            };

            if let Some(arr) = value.as_array_mut() {
                let elems = arr.borrow();
                write_elems(&elems)
            } else if let Some(elems) = value.as_array() {
                write_elems(elems)
            } else {
                Err(LError::ffi_type_error(
                    "array",
                    format!("expected array or bytes, got {}", value.type_name()),
                ))
            }
        }

        // Every other type is a scalar, and owns nothing beyond the write.
        _ => write_scalar(ptr, value, desc).map(|()| Vec::new()),
    }
}
