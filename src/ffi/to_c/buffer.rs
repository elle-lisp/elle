//! `write_value_to_buffer` — writing an Elle Value into raw C storage.
//!
//! Split from the module root: this is the low-level counterpart to
//! `MarshalledArg::new`, writing directly into a caller-provided buffer rather
//! than allocating owned `ArgStorage`.

use super::{ArgStorage, MarshalledArg};
use crate::error::{LError, LResult};
use crate::ffi::marshal::{desc_name_full, extract_int, range_check};
use crate::ffi::types::TypeDesc;
use crate::value::Value;
use std::ffi::{c_void, CString};

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

        TypeDesc::Bool => {
            let v: std::ffi::c_int = if value.is_truthy() { 1 } else { 0 };
            unsafe { *(ptr as *mut std::ffi::c_int) = v };
            Ok(Vec::new())
        }

        TypeDesc::I8 | TypeDesc::Char => {
            let n = extract_int(value, desc_name_full(desc))?;
            range_check(n, i8::MIN as i64, i8::MAX as i64, desc_name_full(desc))?;
            unsafe { *(ptr as *mut i8) = n as i8 };
            Ok(Vec::new())
        }
        TypeDesc::U8 | TypeDesc::UChar => {
            let n = extract_int(value, desc_name_full(desc))?;
            range_check(n, u8::MIN as i64, u8::MAX as i64, desc_name_full(desc))?;
            unsafe { *ptr = n as u8 };
            Ok(Vec::new())
        }
        TypeDesc::I16 | TypeDesc::Short => {
            let n = extract_int(value, desc_name_full(desc))?;
            range_check(n, i16::MIN as i64, i16::MAX as i64, desc_name_full(desc))?;
            unsafe { *(ptr as *mut i16) = n as i16 };
            Ok(Vec::new())
        }
        TypeDesc::U16 | TypeDesc::UShort => {
            let n = extract_int(value, desc_name_full(desc))?;
            range_check(n, u16::MIN as i64, u16::MAX as i64, desc_name_full(desc))?;
            unsafe { *(ptr as *mut u16) = n as u16 };
            Ok(Vec::new())
        }
        TypeDesc::I32 | TypeDesc::Int => {
            let n = extract_int(value, desc_name_full(desc))?;
            range_check(
                n,
                std::ffi::c_int::MIN as i64,
                std::ffi::c_int::MAX as i64,
                desc_name_full(desc),
            )?;
            unsafe { *(ptr as *mut i32) = n as i32 };
            Ok(Vec::new())
        }
        TypeDesc::U32 | TypeDesc::UInt => {
            let n = extract_int(value, desc_name_full(desc))?;
            range_check(n, 0, std::ffi::c_uint::MAX as i64, desc_name_full(desc))?;
            unsafe { *(ptr as *mut u32) = n as u32 };
            Ok(Vec::new())
        }
        TypeDesc::I64 | TypeDesc::Long | TypeDesc::SSize => {
            let n = extract_int(value, desc_name_full(desc))?;
            unsafe { *(ptr as *mut i64) = n };
            Ok(Vec::new())
        }
        TypeDesc::U64 | TypeDesc::ULong | TypeDesc::Size => {
            let n = extract_int(value, desc_name_full(desc))?;
            // Bit-reinterpret back to u64: completes the lossless round-trip
            // with from_c.rs. See from_c.rs module-level doc for convention.
            unsafe { *(ptr as *mut u64) = n as u64 };
            Ok(Vec::new())
        }

        TypeDesc::Float => {
            let f = value
                .as_float()
                .or_else(|| value.as_int().map(|i| i as f64))
                .ok_or_else(|| {
                    LError::ffi_type_error(
                        "float",
                        format!("expected number, got {}", value.type_name()),
                    )
                })?;
            unsafe { *(ptr as *mut f32) = f as f32 };
            Ok(Vec::new())
        }
        TypeDesc::Double => {
            let f = value
                .as_float()
                .or_else(|| value.as_int().map(|i| i as f64))
                .ok_or_else(|| {
                    LError::ffi_type_error(
                        "double",
                        format!("expected number, got {}", value.type_name()),
                    )
                })?;
            unsafe { *(ptr as *mut f64) = f };
            Ok(Vec::new())
        }

        TypeDesc::Ptr => {
            let p = if value.is_nil() {
                std::ptr::null::<c_void>()
            } else if let Some(addr) = value.as_pointer() {
                addr as *const c_void
            } else if let Some(cell) = value.as_managed_pointer() {
                match cell.get() {
                    Some(addr) => addr as *const c_void,
                    None => {
                        return Err(LError::ffi_type_error("ptr", "pointer has been freed"));
                    }
                }
            } else {
                return Err(LError::ffi_type_error(
                    "ptr",
                    format!("expected pointer or nil, got {}", value.type_name()),
                ));
            };
            unsafe { *(ptr as *mut *const c_void) = p };
            Ok(Vec::new())
        }

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
    }
}
