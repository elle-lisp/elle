//! Value → C marshalling: MarshalledArg and write_value_to_buffer.
//!
//! Converts Elle Values into C-typed storage for FFI arguments.

use crate::error::{LError, LResult};
use crate::ffi::marshal::{desc_name, desc_name_full, extract_int, range_check, AlignedBuffer};
use crate::ffi::types::{StructDesc, TypeDesc};
use crate::value::Value;
use std::ffi::{c_void, CString};

/// Holds C-typed data for an FFI argument.
///
/// Must live as long as the `libffi::middle::Arg` references it.
/// Created from an Elle `Value` and a `TypeDesc`, then passed to
/// `ffi_call` via `as_arg()`.
pub struct MarshalledArg {
    storage: ArgStorage,
}

#[allow(dead_code)]
enum ArgStorage {
    I8(i8),
    U8(u8),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
    Ptr(*const c_void),
    /// Owned CString for `:string` type. The `*const c_char` is the
    /// pointer that libffi reads through (it's a `char*` argument).
    /// The CString must outlive the Arg.
    Str(CString, *const std::ffi::c_char),
    /// Struct/array data in an aligned buffer. The `Vec<MarshalledArg>`
    /// keeps CStrings and nested buffers alive for the buffer's lifetime.
    Struct(AlignedBuffer, Vec<MarshalledArg>),
}

impl MarshalledArg {
    /// Create from an Elle Value and a type descriptor.
    pub fn new(value: &Value, desc: &TypeDesc) -> LResult<Self> {
        let storage = match desc {
            TypeDesc::Void => {
                return Err(LError::ffi_type_error(
                    "void",
                    "void is not valid for arguments",
                ));
            }

            TypeDesc::Bool => ArgStorage::I32(if value.is_truthy() { 1 } else { 0 }),

            TypeDesc::I8 => {
                let n = extract_int(value, "i8")?;
                range_check(n, i8::MIN as i64, i8::MAX as i64, "i8")?;
                ArgStorage::I8(n as i8)
            }
            TypeDesc::U8 | TypeDesc::UChar => {
                let n = extract_int(value, desc_name(desc))?;
                range_check(n, u8::MIN as i64, u8::MAX as i64, desc_name(desc))?;
                ArgStorage::U8(n as u8)
            }
            TypeDesc::I16 => {
                let n = extract_int(value, "i16")?;
                range_check(n, i16::MIN as i64, i16::MAX as i64, "i16")?;
                ArgStorage::I16(n as i16)
            }
            TypeDesc::U16 => {
                let n = extract_int(value, "u16")?;
                range_check(n, u16::MIN as i64, u16::MAX as i64, "u16")?;
                ArgStorage::U16(n as u16)
            }
            TypeDesc::I32 => {
                let n = extract_int(value, "i32")?;
                range_check(n, i32::MIN as i64, i32::MAX as i64, "i32")?;
                ArgStorage::I32(n as i32)
            }
            TypeDesc::U32 => {
                let n = extract_int(value, "u32")?;
                range_check(n, u32::MIN as i64, u32::MAX as i64, "u32")?;
                ArgStorage::U32(n as u32)
            }
            TypeDesc::I64 => {
                let n = extract_int(value, "i64")?;
                ArgStorage::I64(n)
            }
            TypeDesc::U64 => {
                let n = extract_int(value, "u64")?;
                // Bit-reinterpret back to u64: completes the lossless round-trip
                // with from_c.rs. See from_c.rs module-level doc for convention.
                ArgStorage::U64(n as u64)
            }
            TypeDesc::Int => {
                let n = extract_int(value, "int")?;
                range_check(
                    n,
                    std::ffi::c_int::MIN as i64,
                    std::ffi::c_int::MAX as i64,
                    "int",
                )?;
                ArgStorage::I32(n as i32)
            }
            TypeDesc::UInt => {
                let n = extract_int(value, "uint")?;
                range_check(n, 0, std::ffi::c_uint::MAX as i64, "uint")?;
                ArgStorage::U32(n as u32)
            }
            TypeDesc::Long => {
                let n = extract_int(value, "long")?;
                ArgStorage::I64(n as std::ffi::c_long as i64)
            }
            TypeDesc::ULong => {
                let n = extract_int(value, "ulong")?;
                ArgStorage::U64(n as std::ffi::c_ulong as u64)
            }
            TypeDesc::Char => {
                let n = extract_int(value, "char")?;
                range_check(n, i8::MIN as i64, i8::MAX as i64, "char")?;
                ArgStorage::I8(n as i8)
            }
            TypeDesc::Short => {
                let n = extract_int(value, "short")?;
                range_check(
                    n,
                    std::ffi::c_short::MIN as i64,
                    std::ffi::c_short::MAX as i64,
                    "short",
                )?;
                ArgStorage::I16(n as i16)
            }
            TypeDesc::UShort => {
                let n = extract_int(value, "ushort")?;
                range_check(n, 0, std::ffi::c_ushort::MAX as i64, "ushort")?;
                ArgStorage::U16(n as u16)
            }
            TypeDesc::Size => {
                let n = extract_int(value, "size")?;
                ArgStorage::U64(n as usize as u64)
            }
            TypeDesc::SSize => {
                let n = extract_int(value, "ssize")?;
                ArgStorage::I64(n as isize as i64)
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
                ArgStorage::F32(f as f32)
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
                ArgStorage::F64(f)
            }

            TypeDesc::Ptr => {
                if value.is_nil() {
                    ArgStorage::Ptr(std::ptr::null())
                } else if let Some(addr) = value.as_pointer() {
                    ArgStorage::Ptr(addr as *const c_void)
                } else if let Some(cell) = value.as_managed_pointer() {
                    match cell.get() {
                        Some(addr) => ArgStorage::Ptr(addr as *const c_void),
                        None => {
                            return Err(LError::ffi_type_error("ptr", "pointer has been freed"));
                        }
                    }
                } else {
                    return Err(LError::ffi_type_error(
                        "ptr",
                        format!("expected pointer or nil, got {}", value.type_name()),
                    ));
                }
            }

            TypeDesc::Str => {
                let s = value.with_string(|s| s.to_string()).ok_or_else(|| {
                    LError::ffi_type_error(
                        "string",
                        format!("expected string, got {}", value.type_name()),
                    )
                })?;
                let cstring = CString::new(s.as_str())
                    .map_err(|_| LError::ffi_type_error("string", "contains interior null byte"))?;
                let ptr = cstring.as_ptr();
                ArgStorage::Str(cstring, ptr)
            }

            TypeDesc::Struct(sd) => {
                return marshal_struct(value, sd, desc);
            }
            TypeDesc::Array(elem_desc, count) => {
                return marshal_array(value, elem_desc, *count);
            }
        };
        Ok(MarshalledArg { storage })
    }

    /// Get a libffi Arg referencing this storage.
    pub fn as_arg(&self) -> libffi::middle::Arg<'_> {
        match &self.storage {
            ArgStorage::I8(v) => libffi::middle::arg(v),
            ArgStorage::U8(v) => libffi::middle::arg(v),
            ArgStorage::I16(v) => libffi::middle::arg(v),
            ArgStorage::U16(v) => libffi::middle::arg(v),
            ArgStorage::I32(v) => libffi::middle::arg(v),
            ArgStorage::U32(v) => libffi::middle::arg(v),
            ArgStorage::I64(v) => libffi::middle::arg(v),
            ArgStorage::U64(v) => libffi::middle::arg(v),
            ArgStorage::F32(v) => libffi::middle::arg(v),
            ArgStorage::F64(v) => libffi::middle::arg(v),
            ArgStorage::Ptr(v) => libffi::middle::arg(v),
            ArgStorage::Str(_, ptr) => libffi::middle::arg(ptr),
            ArgStorage::Struct(buf, _) => {
                // Safety: buf.as_mut_ptr() points to valid, aligned struct data that
                // outlives this Arg (the AlignedBuffer lives in ArgStorage).
                // Arg::new stores the pointer as *mut c_void; libffi reads the
                // struct data starting at that address.
                unsafe { libffi::middle::arg(&*buf.as_mut_ptr()) }
            }
        }
    }
}

fn marshal_struct(value: &Value, sd: &StructDesc, desc: &TypeDesc) -> LResult<MarshalledArg> {
    let arr = value.as_array_mut().ok_or_else(|| {
        LError::ffi_type_error(
            "struct",
            format!("expected array, got {}", value.type_name()),
        )
    })?;
    let elems = arr.borrow();
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
    let (offsets, total_size) = sd.field_offsets().ok_or_else(|| {
        LError::ffi_error("marshal", "cannot compute struct layout (contains void?)")
    })?;
    let align = desc.align().unwrap_or(1);
    let buf = AlignedBuffer::new(total_size, align);
    let mut owned = Vec::new();
    for (i, (field_desc, &field_offset)) in sd.fields.iter().zip(offsets.iter()).enumerate() {
        let field_owned = write_value_to_buffer(
            unsafe { buf.as_mut_ptr().add(field_offset) },
            &elems[i],
            field_desc,
        )?;
        owned.extend(field_owned);
    }
    Ok(MarshalledArg {
        storage: ArgStorage::Struct(buf, owned),
    })
}

fn marshal_array(value: &Value, elem_desc: &TypeDesc, count: usize) -> LResult<MarshalledArg> {
    let arr = value.as_array_mut().ok_or_else(|| {
        LError::ffi_type_error(
            "array",
            format!("expected array, got {}", value.type_name()),
        )
    })?;
    let elems = arr.borrow();
    if elems.len() != count {
        return Err(LError::ffi_type_error(
            "array",
            format!("array has {} elements, got {} values", count, elems.len()),
        ));
    }
    let elem_size = elem_desc
        .size()
        .ok_or_else(|| LError::ffi_error("marshal", "cannot compute array element size"))?;
    let total_size = elem_size * count;
    let align = elem_desc.align().unwrap_or(1);
    let buf = AlignedBuffer::new(total_size, align);
    let mut owned = Vec::new();
    for (i, elem_val) in elems.iter().enumerate() {
        let elem_owned = write_value_to_buffer(
            unsafe { buf.as_mut_ptr().add(i * elem_size) },
            elem_val,
            elem_desc,
        )?;
        owned.extend(elem_owned);
    }
    Ok(MarshalledArg {
        storage: ArgStorage::Struct(buf, owned),
    })
}

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
            // Create a CString, write its pointer into the buffer, and
            // return a MarshalledArg that owns the CString.
            let s = value.with_string(|s| s.to_string()).ok_or_else(|| {
                LError::ffi_type_error(
                    "string",
                    format!("expected string, got {}", value.type_name()),
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
            let arr = value.as_array_mut().ok_or_else(|| {
                LError::ffi_type_error(
                    "struct",
                    format!("expected array, got {}", value.type_name()),
                )
            })?;
            let elems = arr.borrow();
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
            let (offsets, _) = sd.field_offsets().ok_or_else(|| {
                LError::ffi_error("marshal", "cannot compute struct layout (contains void?)")
            })?;
            let mut owned = Vec::new();
            for (i, (field_desc, &field_offset)) in sd.fields.iter().zip(offsets.iter()).enumerate()
            {
                let field_owned =
                    write_value_to_buffer(unsafe { ptr.add(field_offset) }, &elems[i], field_desc)?;
                owned.extend(field_owned);
            }
            Ok(owned)
        }

        TypeDesc::Array(elem_desc, count) => {
            let arr = value.as_array_mut().ok_or_else(|| {
                LError::ffi_type_error(
                    "array",
                    format!("expected array, got {}", value.type_name()),
                )
            })?;
            let elems = arr.borrow();
            if elems.len() != *count {
                return Err(LError::ffi_type_error(
                    "array",
                    format!("array has {} elements, got {} values", count, elems.len()),
                ));
            }
            let elem_size = elem_desc
                .size()
                .ok_or_else(|| LError::ffi_error("marshal", "cannot compute array element size"))?;
            let mut owned = Vec::new();
            for (i, elem_val) in elems.iter().enumerate() {
                let elem_owned =
                    write_value_to_buffer(unsafe { ptr.add(i * elem_size) }, elem_val, elem_desc)?;
                owned.extend(elem_owned);
            }
            Ok(owned)
        }
    }
}
