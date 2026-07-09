//! `MarshalledArg::new` and its struct/array marshalling helpers.
//!
//! Split from the module root so the large per-`TypeDesc` conversion match and
//! its compound (struct/array) helpers live apart from the storage type
//! definitions and the `as_arg` accessor.

use super::{ArgStorage, MarshalledArg};
use crate::error::{LError, LResult};
use crate::ffi::marshal::{desc_name, extract_int, range_check, AlignedBuffer};
use crate::ffi::to_c::write_value_to_buffer;
use crate::ffi::types::{StructDesc, TypeDesc};
use crate::value::Value;
use std::ffi::{c_void, CString};

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
                // nil → NULL pointer
                if value.is_nil() {
                    ArgStorage::Ptr(std::ptr::null_mut())
                } else {
                    let s = value.with_string(|s| s.to_string()).ok_or_else(|| {
                        LError::ffi_type_error(
                            "string",
                            format!("expected string or nil, got {}", value.type_name()),
                        )
                    })?;
                    let cstring = CString::new(s.as_str()).map_err(|_| {
                        LError::ffi_type_error("string", "contains interior null byte")
                    })?;
                    let ptr = cstring.as_ptr();
                    ArgStorage::Str(cstring, ptr)
                }
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
}

fn marshal_struct(value: &Value, sd: &StructDesc, desc: &TypeDesc) -> LResult<MarshalledArg> {
    let (offsets, total_size) = sd.field_offsets().ok_or_else(|| {
        LError::ffi_error("marshal", "cannot compute struct layout (contains void?)")
    })?;
    let align = desc.align().unwrap_or(1);

    // Accept both mutable @[...] and immutable [...] arrays.
    let write_fields = |elems: &[Value]| -> LResult<MarshalledArg> {
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

fn marshal_array(value: &Value, elem_desc: &TypeDesc, count: usize) -> LResult<MarshalledArg> {
    let elem_size = elem_desc
        .size()
        .ok_or_else(|| LError::ffi_error("marshal", "cannot compute array element size"))?;
    let total_size = elem_size * count;
    let align = elem_desc.align().unwrap_or(1);

    // Accept both mutable @[...] and immutable [...] arrays.
    let write_elems = |elems: &[Value]| -> LResult<MarshalledArg> {
        if elems.len() != count {
            return Err(LError::ffi_type_error(
                "array",
                format!("array has {} elements, got {} values", count, elems.len()),
            ));
        }
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
    };

    if let Some(arr) = value.as_array_mut() {
        let elems = arr.borrow();
        write_elems(&elems)
    } else if let Some(elems) = value.as_array() {
        write_elems(elems)
    } else {
        Err(LError::ffi_type_error(
            "array",
            format!("expected array, got {}", value.type_name()),
        ))
    }
}
