// audited: 2026-10-13
//! `MarshalledArg::new`: convert one argument value to the C storage its type names, structs and arrays included.
//!
//! docs/ffi.md

use super::{ArgStorage, MarshalledArg};
use crate::error::{LError, LResult};
use crate::ffi::marshal::AlignedBuffer;
use crate::ffi::scalar::{desc_name_full, extract_int, int_in, number, pointer};
use crate::ffi::to_c::write_value_to_buffer;
use crate::ffi::types::{to_c_bool, StructDesc, TypeDesc};
use crate::value::Value;
use std::ffi::CString;

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

            TypeDesc::Bool => ArgStorage::U8(to_c_bool(value)),
            TypeDesc::Bool32 => ArgStorage::I32(to_c_bool(value)),

            TypeDesc::I8 | TypeDesc::Char => {
                ArgStorage::I8(int_in(value, desc, i8::MIN as i64, i8::MAX as i64)? as i8)
            }
            TypeDesc::U8 | TypeDesc::UChar => {
                ArgStorage::U8(int_in(value, desc, u8::MIN as i64, u8::MAX as i64)? as u8)
            }
            TypeDesc::I16 | TypeDesc::Short => {
                ArgStorage::I16(int_in(value, desc, i16::MIN as i64, i16::MAX as i64)? as i16)
            }
            TypeDesc::U16 | TypeDesc::UShort => {
                ArgStorage::U16(int_in(value, desc, u16::MIN as i64, u16::MAX as i64)? as u16)
            }
            TypeDesc::I32 | TypeDesc::Int => ArgStorage::I32(int_in(
                value,
                desc,
                std::ffi::c_int::MIN as i64,
                std::ffi::c_int::MAX as i64,
            )? as i32),
            TypeDesc::U32 | TypeDesc::UInt => {
                ArgStorage::U32(int_in(value, desc, 0, std::ffi::c_uint::MAX as i64)? as u32)
            }
            TypeDesc::I64 | TypeDesc::Long | TypeDesc::SSize => {
                ArgStorage::I64(extract_int(value, desc_name_full(desc))?)
            }
            TypeDesc::U64 | TypeDesc::ULong | TypeDesc::Size => {
                // Bit-reinterpret back to u64: completes the lossless round-trip
                // with from_c.rs. See from_c.rs module-level doc for convention.
                ArgStorage::U64(extract_int(value, desc_name_full(desc))? as u64)
            }

            TypeDesc::Float => ArgStorage::F32(number(value, desc)? as f32),
            TypeDesc::Double => ArgStorage::F64(number(value, desc)?),

            TypeDesc::Ptr => ArgStorage::Ptr(pointer(value)?),

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
