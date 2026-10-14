// audited: 2026-10-13
//! The scalar writer: a number, boolean or pointer, checked against its C type and written into C memory.
//!
//! docs/ffi.md
//!
//! It needs no libffi, so it builds without the `ffi` feature, and `ffi/write`
//! checks a scalar the same way in every build. `MarshalledArg::new` uses the
//! same checks for an argument.

use crate::error::{LError, LResult};
use crate::ffi::types::{to_c_bool, CBool, CBool32, TypeDesc};
use crate::value::Value;
use std::ffi::c_void;

/// Write the scalar `value` at `ptr` as the C type `desc`. A value the type
/// cannot hold is an error, and then nothing is written. `Void`, `Str`,
/// `Struct` and `Array` are not scalars, and are an error too.
///
/// # Safety
/// `ptr` must point to a writable region of at least `desc.size()` bytes
/// with appropriate alignment.
pub(crate) fn write_scalar(ptr: *mut u8, value: &Value, desc: &TypeDesc) -> LResult<()> {
    match desc {
        TypeDesc::Bool => unsafe { *(ptr as *mut CBool) = to_c_bool(value) },
        TypeDesc::Bool32 => unsafe { *(ptr as *mut CBool32) = to_c_bool(value) },
        TypeDesc::I8 | TypeDesc::Char => {
            let n = int_in(value, desc, i8::MIN as i64, i8::MAX as i64)?;
            unsafe { *(ptr as *mut i8) = n as i8 };
        }
        TypeDesc::U8 | TypeDesc::UChar => {
            let n = int_in(value, desc, u8::MIN as i64, u8::MAX as i64)?;
            unsafe { *ptr = n as u8 };
        }
        TypeDesc::I16 | TypeDesc::Short => {
            let n = int_in(value, desc, i16::MIN as i64, i16::MAX as i64)?;
            unsafe { *(ptr as *mut i16) = n as i16 };
        }
        TypeDesc::U16 | TypeDesc::UShort => {
            let n = int_in(value, desc, u16::MIN as i64, u16::MAX as i64)?;
            unsafe { *(ptr as *mut u16) = n as u16 };
        }
        TypeDesc::I32 | TypeDesc::Int => {
            let n = int_in(
                value,
                desc,
                std::ffi::c_int::MIN as i64,
                std::ffi::c_int::MAX as i64,
            )?;
            unsafe { *(ptr as *mut i32) = n as i32 };
        }
        TypeDesc::U32 | TypeDesc::UInt => {
            let n = int_in(value, desc, 0, std::ffi::c_uint::MAX as i64)?;
            unsafe { *(ptr as *mut u32) = n as u32 };
        }
        TypeDesc::I64 | TypeDesc::Long | TypeDesc::SSize => {
            let n = extract_int(value, desc_name_full(desc))?;
            unsafe { *(ptr as *mut i64) = n };
        }
        TypeDesc::U64 | TypeDesc::ULong | TypeDesc::Size => {
            let n = extract_int(value, desc_name_full(desc))?;
            // Bit-reinterpret back to u64: completes the lossless round-trip
            // with from_c.rs. See from_c.rs module-level doc for convention.
            unsafe { *(ptr as *mut u64) = n as u64 };
        }
        TypeDesc::Float => {
            let f = number(value, desc)?;
            unsafe { *(ptr as *mut f32) = f as f32 };
        }
        TypeDesc::Double => {
            let f = number(value, desc)?;
            unsafe { *(ptr as *mut f64) = f };
        }
        TypeDesc::Ptr => {
            let p = pointer(value)?;
            unsafe { *(ptr as *mut *const c_void) = p };
        }
        TypeDesc::Void | TypeDesc::Str | TypeDesc::Struct(_) | TypeDesc::Array(_, _) => {
            return Err(LError::ffi_error(
                "marshal",
                format!("{} is not a scalar type", desc.short_name()),
            ));
        }
    }
    Ok(())
}

/// The integer `value`, which must lie in `[min, max]` to be a `desc`.
pub(crate) fn int_in(value: &Value, desc: &TypeDesc, min: i64, max: i64) -> LResult<i64> {
    let n = extract_int(value, desc_name_full(desc))?;
    range_check(n, min, max, desc_name_full(desc))?;
    Ok(n)
}

/// The number `value` as a float: a `:float` or `:double` accepts an integer.
pub(crate) fn number(value: &Value, desc: &TypeDesc) -> LResult<f64> {
    value
        .as_float()
        .or_else(|| value.as_int().map(|i| i as f64))
        .ok_or_else(|| {
            LError::ffi_type_error(
                desc_name_full(desc),
                format!("expected number, got {}", value.type_name()),
            )
        })
}

/// The address `value` names: nil is NULL, and a freed pointer is an error.
pub(crate) fn pointer(value: &Value) -> LResult<*const c_void> {
    if value.is_nil() {
        Ok(std::ptr::null())
    } else if let Some(addr) = value.as_pointer() {
        Ok(addr as *const c_void)
    } else if let Some(cell) = value.as_managed_pointer() {
        cell.get()
            .map(|addr| addr as *const c_void)
            .ok_or_else(|| LError::ffi_type_error("ptr", "pointer has been freed"))
    } else {
        Err(LError::ffi_type_error(
            "ptr",
            format!("expected pointer or nil, got {}", value.type_name()),
        ))
    }
}

pub(crate) fn extract_int(value: &Value, type_name: &str) -> LResult<i64> {
    value.as_int().ok_or_else(|| {
        LError::ffi_type_error(
            type_name,
            format!("expected integer, got {}", value.type_name()),
        )
    })
}

pub(crate) fn range_check(n: i64, min: i64, max: i64, type_name: &str) -> LResult<()> {
    if n < min || n > max {
        Err(LError::ffi_type_error(
            type_name,
            format!("value {} out of range [{}, {}]", n, min, max),
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn desc_name_full(desc: &TypeDesc) -> &'static str {
    match desc {
        TypeDesc::I8 => "i8",
        TypeDesc::U8 => "u8",
        TypeDesc::I16 => "i16",
        TypeDesc::U16 => "u16",
        TypeDesc::I32 => "i32",
        TypeDesc::U32 => "u32",
        TypeDesc::I64 => "i64",
        TypeDesc::U64 => "u64",
        TypeDesc::Int => "int",
        TypeDesc::UInt => "uint",
        TypeDesc::Long => "long",
        TypeDesc::ULong => "ulong",
        TypeDesc::Char => "char",
        TypeDesc::UChar => "uchar",
        TypeDesc::Short => "short",
        TypeDesc::UShort => "ushort",
        TypeDesc::Size => "size",
        TypeDesc::SSize => "ssize",
        TypeDesc::Float => "float",
        TypeDesc::Double => "double",
        TypeDesc::Ptr => "ptr",
        TypeDesc::Str => "string",
        TypeDesc::Bool => "bool",
        TypeDesc::Bool32 => "bool32",
        TypeDesc::Void => "void",
        TypeDesc::Struct(_) => "struct",
        TypeDesc::Array(_, _) => "array",
    }
}
