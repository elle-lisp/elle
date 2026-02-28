//! Marshalling between Elle Values and C-typed data for libffi calls.
//!
//! Two concerns:
//! - **Argument marshalling**: Elle `Value` -> C-typed storage -> `libffi::middle::Arg`
//! - **Return conversion**: C return value -> Elle `Value`

use crate::error::{LError, LResult};
use crate::ffi::types::{StructDesc, TypeDesc};
use crate::value::Value;
use libffi::middle::Type;
use std::ffi::{c_void, CString};

/// Convert a `TypeDesc` to the corresponding `libffi::middle::Type`.
pub fn to_libffi_type(desc: &TypeDesc) -> Type {
    match desc {
        TypeDesc::Void => Type::void(),
        TypeDesc::Bool => Type::c_int(),
        TypeDesc::I8 => Type::i8(),
        TypeDesc::U8 => Type::u8(),
        TypeDesc::I16 => Type::i16(),
        TypeDesc::U16 => Type::u16(),
        TypeDesc::I32 => Type::i32(),
        TypeDesc::U32 => Type::u32(),
        TypeDesc::I64 => Type::i64(),
        TypeDesc::U64 => Type::u64(),
        TypeDesc::Float => Type::f32(),
        TypeDesc::Double => Type::f64(),
        TypeDesc::Int => Type::c_int(),
        TypeDesc::UInt => Type::c_uint(),
        TypeDesc::Long => Type::c_long(),
        TypeDesc::ULong => Type::c_ulong(),
        TypeDesc::Char => Type::i8(),
        TypeDesc::UChar => Type::u8(),
        TypeDesc::Short => Type::c_short(),
        TypeDesc::UShort => Type::c_ushort(),
        TypeDesc::Size => Type::usize(),
        TypeDesc::SSize => Type::isize(),
        TypeDesc::Ptr | TypeDesc::Str => Type::pointer(),
        TypeDesc::Struct(desc) => {
            let fields: Vec<Type> = desc.fields.iter().map(to_libffi_type).collect();
            Type::structure(fields)
        }
        TypeDesc::Array(elem, count) => {
            let elem_type = to_libffi_type(elem);
            let fields: Vec<Type> = (0..*count).map(|_| elem_type.clone()).collect();
            Type::structure(fields)
        }
    }
}

// ── Aligned buffer ──────────────────────────────────────────────────

/// Heap-allocated buffer with guaranteed alignment for FFI struct data.
///
/// Used to hold C struct/array data that libffi reads from (arguments)
/// or writes into (return values). The buffer is zero-initialized.
pub(crate) struct AlignedBuffer {
    ptr: *mut u8,
    layout: std::alloc::Layout,
}

impl AlignedBuffer {
    /// Allocate a zero-initialized buffer of `size` bytes with `align` alignment.
    ///
    /// Panics if align is 0 or not a power of two, or if size overflows
    /// the layout constraints.
    pub(crate) fn new(size: usize, align: usize) -> Self {
        // std::alloc::alloc with size 0 is UB; use at least 1 byte.
        let effective_size = size.max(1);
        let layout = std::alloc::Layout::from_size_align(effective_size, align)
            .expect("invalid layout for AlignedBuffer");
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        AlignedBuffer { ptr, layout }
    }

    /// Raw pointer to the buffer data.
    pub(crate) fn as_mut_ptr(&self) -> *mut u8 {
        self.ptr
    }
}

impl Drop for AlignedBuffer {
    fn drop(&mut self) {
        unsafe {
            std::alloc::dealloc(self.ptr, self.layout);
        }
    }
}

// ── Argument marshalling ────────────────────────────────────────────

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
                // Safety: buf.ptr points to valid, aligned struct data that
                // outlives this Arg (the AlignedBuffer lives in ArgStorage).
                // Arg::new stores buf.ptr as *mut c_void; libffi reads the
                // struct data starting at that address.
                unsafe { libffi::middle::arg(&*buf.ptr) }
            }
        }
    }
}

// ── Struct/array marshalling ────────────────────────────────────────

fn marshal_struct(value: &Value, sd: &StructDesc, desc: &TypeDesc) -> LResult<MarshalledArg> {
    let arr = value.as_array().ok_or_else(|| {
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
        let field_owned =
            write_value_to_buffer(unsafe { buf.ptr.add(field_offset) }, &elems[i], field_desc)?;
        owned.extend(field_owned);
    }
    Ok(MarshalledArg {
        storage: ArgStorage::Struct(buf, owned),
    })
}

fn marshal_array(value: &Value, elem_desc: &TypeDesc, count: usize) -> LResult<MarshalledArg> {
    let arr = value.as_array().ok_or_else(|| {
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
        let elem_owned =
            write_value_to_buffer(unsafe { buf.ptr.add(i * elem_size) }, elem_val, elem_desc)?;
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
pub(crate) fn write_value_to_buffer(
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
            let arr = value.as_array().ok_or_else(|| {
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
            let arr = value.as_array().ok_or_else(|| {
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
            Ok(Value::array(values))
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
            Ok(Value::array(values))
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

fn extract_int(value: &Value, type_name: &str) -> LResult<i64> {
    value.as_int().ok_or_else(|| {
        LError::ffi_type_error(
            type_name,
            format!("expected integer, got {}", value.type_name()),
        )
    })
}

fn range_check(n: i64, min: i64, max: i64, type_name: &str) -> LResult<()> {
    if n < min || n > max {
        Err(LError::ffi_type_error(
            type_name,
            format!("value {} out of range [{}, {}]", n, min, max),
        ))
    } else {
        Ok(())
    }
}

fn desc_name(desc: &TypeDesc) -> &'static str {
    match desc {
        TypeDesc::UChar => "uchar",
        TypeDesc::U8 => "u8",
        _ => "unknown",
    }
}

fn desc_name_full(desc: &TypeDesc) -> &'static str {
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
        TypeDesc::Void => "void",
        TypeDesc::Struct(_) => "struct",
        TypeDesc::Array(_, _) => "array",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::types::StructDesc;

    #[test]
    fn test_to_libffi_type_primitives() {
        // Smoke test: these should not panic
        to_libffi_type(&TypeDesc::Void);
        to_libffi_type(&TypeDesc::Bool);
        to_libffi_type(&TypeDesc::I8);
        to_libffi_type(&TypeDesc::U8);
        to_libffi_type(&TypeDesc::I16);
        to_libffi_type(&TypeDesc::U16);
        to_libffi_type(&TypeDesc::I32);
        to_libffi_type(&TypeDesc::U32);
        to_libffi_type(&TypeDesc::I64);
        to_libffi_type(&TypeDesc::U64);
        to_libffi_type(&TypeDesc::Float);
        to_libffi_type(&TypeDesc::Double);
        to_libffi_type(&TypeDesc::Int);
        to_libffi_type(&TypeDesc::UInt);
        to_libffi_type(&TypeDesc::Long);
        to_libffi_type(&TypeDesc::ULong);
        to_libffi_type(&TypeDesc::Char);
        to_libffi_type(&TypeDesc::UChar);
        to_libffi_type(&TypeDesc::Short);
        to_libffi_type(&TypeDesc::UShort);
        to_libffi_type(&TypeDesc::Size);
        to_libffi_type(&TypeDesc::SSize);
        to_libffi_type(&TypeDesc::Ptr);
        to_libffi_type(&TypeDesc::Str);
    }

    #[test]
    fn test_to_libffi_type_struct() {
        let desc = TypeDesc::Struct(StructDesc {
            fields: vec![TypeDesc::I32, TypeDesc::Double],
        });
        to_libffi_type(&desc);
    }

    #[test]
    fn test_to_libffi_type_array() {
        let desc = TypeDesc::Array(Box::new(TypeDesc::I32), 4);
        to_libffi_type(&desc);
    }

    #[test]
    fn test_marshal_int_types() {
        let val = Value::int(42);
        assert!(MarshalledArg::new(&val, &TypeDesc::I8).is_ok());
        assert!(MarshalledArg::new(&val, &TypeDesc::U8).is_ok());
        assert!(MarshalledArg::new(&val, &TypeDesc::I16).is_ok());
        assert!(MarshalledArg::new(&val, &TypeDesc::U16).is_ok());
        assert!(MarshalledArg::new(&val, &TypeDesc::I32).is_ok());
        assert!(MarshalledArg::new(&val, &TypeDesc::U32).is_ok());
        assert!(MarshalledArg::new(&val, &TypeDesc::I64).is_ok());
        assert!(MarshalledArg::new(&val, &TypeDesc::U64).is_ok());
        assert!(MarshalledArg::new(&val, &TypeDesc::Int).is_ok());
        assert!(MarshalledArg::new(&val, &TypeDesc::UInt).is_ok());
    }

    #[test]
    fn test_marshal_int_range_error() {
        let val = Value::int(256);
        assert!(MarshalledArg::new(&val, &TypeDesc::I8).is_err());
        assert!(MarshalledArg::new(&val, &TypeDesc::U8).is_err());

        let neg = Value::int(-1);
        assert!(MarshalledArg::new(&neg, &TypeDesc::U8).is_err());
        assert!(MarshalledArg::new(&neg, &TypeDesc::U16).is_err());
        assert!(MarshalledArg::new(&neg, &TypeDesc::U32).is_err());
    }

    #[test]
    fn test_marshal_float() {
        let val = Value::float(2.5);
        assert!(MarshalledArg::new(&val, &TypeDesc::Float).is_ok());
        assert!(MarshalledArg::new(&val, &TypeDesc::Double).is_ok());
    }

    #[test]
    fn test_marshal_int_as_float() {
        let val = Value::int(42);
        assert!(MarshalledArg::new(&val, &TypeDesc::Float).is_ok());
        assert!(MarshalledArg::new(&val, &TypeDesc::Double).is_ok());
    }

    #[test]
    fn test_marshal_bool() {
        let t = Value::bool(true);
        let f = Value::bool(false);
        assert!(MarshalledArg::new(&t, &TypeDesc::Bool).is_ok());
        assert!(MarshalledArg::new(&f, &TypeDesc::Bool).is_ok());
    }

    #[test]
    fn test_marshal_ptr_nil() {
        let nil = Value::NIL;
        assert!(MarshalledArg::new(&nil, &TypeDesc::Ptr).is_ok());
    }

    #[test]
    fn test_marshal_ptr_value() {
        let ptr = Value::pointer(0x1234);
        assert!(MarshalledArg::new(&ptr, &TypeDesc::Ptr).is_ok());
    }

    #[test]
    fn test_marshal_ptr_type_error() {
        let val = Value::int(42);
        assert!(MarshalledArg::new(&val, &TypeDesc::Ptr).is_err());
    }

    #[test]
    fn test_marshal_string() {
        let val = Value::string("hello");
        assert!(MarshalledArg::new(&val, &TypeDesc::Str).is_ok());
    }

    #[test]
    fn test_marshal_string_interior_null() {
        let val = Value::string("hel\0lo");
        assert!(MarshalledArg::new(&val, &TypeDesc::Str).is_err());
    }

    #[test]
    fn test_marshal_void_error() {
        let val = Value::NIL;
        assert!(MarshalledArg::new(&val, &TypeDesc::Void).is_err());
    }

    #[test]
    fn test_marshal_struct() {
        let desc = TypeDesc::Struct(StructDesc {
            fields: vec![TypeDesc::I32, TypeDesc::Double],
        });
        let val = Value::array(vec![Value::int(42), Value::float(1.5)]);
        let m = MarshalledArg::new(&val, &desc).unwrap();
        let _ = m.as_arg(); // Should not panic
    }

    #[test]
    fn test_marshal_struct_wrong_count() {
        let desc = TypeDesc::Struct(StructDesc {
            fields: vec![TypeDesc::I32, TypeDesc::Double],
        });
        let val = Value::array(vec![Value::int(42)]); // Only 1 value for 2 fields
        assert!(MarshalledArg::new(&val, &desc).is_err());
    }

    #[test]
    fn test_marshal_struct_wrong_type() {
        let desc = TypeDesc::Struct(StructDesc {
            fields: vec![TypeDesc::I32],
        });
        let val = Value::int(42); // Not an array
        assert!(MarshalledArg::new(&val, &desc).is_err());
    }

    #[test]
    fn test_marshal_array() {
        let desc = TypeDesc::Array(Box::new(TypeDesc::I32), 3);
        let val = Value::array(vec![Value::int(1), Value::int(2), Value::int(3)]);
        let m = MarshalledArg::new(&val, &desc).unwrap();
        let _ = m.as_arg();
    }

    #[test]
    fn test_marshal_array_wrong_count() {
        let desc = TypeDesc::Array(Box::new(TypeDesc::I32), 3);
        let val = Value::array(vec![Value::int(1), Value::int(2)]);
        assert!(MarshalledArg::new(&val, &desc).is_err());
    }

    #[test]
    fn test_read_write_struct_roundtrip() {
        let sd = StructDesc {
            fields: vec![TypeDesc::I32, TypeDesc::Double, TypeDesc::I64],
        };
        let desc = TypeDesc::Struct(sd.clone());
        let values = Value::array(vec![Value::int(42), Value::float(1.5), Value::int(-100)]);

        let (offsets, total_size) = sd.field_offsets().unwrap();
        let align = desc.align().unwrap();
        let buf = AlignedBuffer::new(total_size, align);

        // Write each field
        let arr = values.as_array().unwrap();
        let elems = arr.borrow();
        for (i, (field_desc, &offset)) in sd.fields.iter().zip(offsets.iter()).enumerate() {
            let _ = write_value_to_buffer(unsafe { buf.ptr.add(offset) }, &elems[i], field_desc)
                .unwrap();
        }

        // Read back
        let result = read_value_from_buffer(buf.ptr, &desc).unwrap();
        let result_arr = result.as_array().unwrap();
        let result_elems = result_arr.borrow();
        assert_eq!(result_elems[0].as_int(), Some(42));
        assert!((result_elems[1].as_float().unwrap() - 1.5).abs() < 1e-10);
        assert_eq!(result_elems[2].as_int(), Some(-100));
    }

    #[test]
    fn test_read_write_array_roundtrip() {
        let desc = TypeDesc::Array(Box::new(TypeDesc::I32), 4);
        let values = Value::array(vec![
            Value::int(10),
            Value::int(20),
            Value::int(30),
            Value::int(40),
        ]);

        let elem_size = TypeDesc::I32.size().unwrap();
        let total_size = elem_size * 4;
        let align = TypeDesc::I32.align().unwrap();
        let buf = AlignedBuffer::new(total_size, align);

        let arr = values.as_array().unwrap();
        let elems = arr.borrow();
        for (i, elem_val) in elems.iter().enumerate() {
            let _ = write_value_to_buffer(
                unsafe { buf.ptr.add(i * elem_size) },
                elem_val,
                &TypeDesc::I32,
            )
            .unwrap();
        }

        let result = read_value_from_buffer(buf.ptr, &desc).unwrap();
        let result_arr = result.as_array().unwrap();
        let result_elems = result_arr.borrow();
        assert_eq!(result_elems.len(), 4);
        assert_eq!(result_elems[0].as_int(), Some(10));
        assert_eq!(result_elems[1].as_int(), Some(20));
        assert_eq!(result_elems[2].as_int(), Some(30));
        assert_eq!(result_elems[3].as_int(), Some(40));
    }

    #[test]
    fn test_read_write_nested_struct_roundtrip() {
        let inner_sd = StructDesc {
            fields: vec![TypeDesc::I8, TypeDesc::I32],
        };
        let outer_sd = StructDesc {
            fields: vec![TypeDesc::I64, TypeDesc::Struct(inner_sd)],
        };
        let desc = TypeDesc::Struct(outer_sd.clone());

        let inner_val = Value::array(vec![Value::int(7), Value::int(999)]);
        let outer_val = Value::array(vec![Value::int(123456), inner_val]);

        // Marshal via MarshalledArg
        let m = MarshalledArg::new(&outer_val, &desc).unwrap();
        let _ = m.as_arg();

        // Also test roundtrip through write/read
        let (offsets, total_size) = outer_sd.field_offsets().unwrap();
        let align = desc.align().unwrap();
        let buf = AlignedBuffer::new(total_size, align);

        let arr = outer_val.as_array().unwrap();
        let elems = arr.borrow();
        for (i, (field_desc, &offset)) in outer_sd.fields.iter().zip(offsets.iter()).enumerate() {
            let _ = write_value_to_buffer(unsafe { buf.ptr.add(offset) }, &elems[i], field_desc)
                .unwrap();
        }

        let result = read_value_from_buffer(buf.ptr, &desc).unwrap();
        let result_arr = result.as_array().unwrap();
        let result_elems = result_arr.borrow();
        assert_eq!(result_elems[0].as_int(), Some(123456));

        let inner_result = result_elems[1].as_array().unwrap();
        let inner_elems = inner_result.borrow();
        assert_eq!(inner_elems[0].as_int(), Some(7));
        assert_eq!(inner_elems[1].as_int(), Some(999));
    }

    #[test]
    fn test_as_arg_does_not_panic() {
        let val = Value::int(42);
        let m = MarshalledArg::new(&val, &TypeDesc::I32).unwrap();
        let _ = m.as_arg();

        let fval = Value::float(1.5);
        let m2 = MarshalledArg::new(&fval, &TypeDesc::Double).unwrap();
        let _ = m2.as_arg();

        let sval = Value::string("test");
        let m3 = MarshalledArg::new(&sval, &TypeDesc::Str).unwrap();
        let _ = m3.as_arg();
    }
}
