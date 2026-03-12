//! FFI memory management, typed access, and type construction primitives

use crate::ffi::types::TypeDesc;
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

use super::ffi::{extract_pointer_addr, resolve_type_desc};

// ── Struct/array type creation ──────────────────────────────────────

pub(crate) fn prim_ffi_struct(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val("arity-error", "ffi/struct: expected 1 argument"),
        );
    }
    // Accept array or list of type descriptors
    let field_vals = if let Some(arr) = args[0].as_array_mut() {
        arr.borrow().clone()
    } else {
        match args[0].list_to_vec() {
            Ok(v) => v,
            Err(_) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "ffi/struct: expected array or list of types, got {}",
                            args[0].type_name()
                        ),
                    ),
                )
            }
        }
    };

    if field_vals.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "argument-error",
                "ffi/struct: struct must have at least one field",
            ),
        );
    }

    let mut fields = Vec::with_capacity(field_vals.len());
    for val in &field_vals {
        match resolve_type_desc(val, "ffi/struct") {
            Ok(desc) => {
                if matches!(desc, TypeDesc::Void) {
                    return (
                        SIG_ERROR,
                        error_val(
                            "argument-error",
                            "ffi/struct: void is not valid as a field type",
                        ),
                    );
                }
                fields.push(desc);
            }
            Err(e) => return e,
        }
    }

    let desc = TypeDesc::Struct(crate::ffi::types::StructDesc { fields });
    (SIG_OK, Value::ffi_type(desc))
}

pub(crate) fn prim_ffi_array(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val("arity-error", "ffi/array: expected 2 arguments"),
        );
    }
    let elem_desc = match resolve_type_desc(&args[0], "ffi/array") {
        Ok(desc) => {
            if matches!(desc, TypeDesc::Void) {
                return (
                    SIG_ERROR,
                    error_val(
                        "argument-error",
                        "ffi/array: void is not valid as element type",
                    ),
                );
            }
            desc
        }
        Err(e) => return e,
    };
    let count = match args[1].as_int() {
        Some(n) if n > 0 => n as usize,
        Some(0) => {
            return (
                SIG_ERROR,
                error_val("argument-error", "ffi/array: count must be positive"),
            )
        }
        Some(n) => {
            return (
                SIG_ERROR,
                error_val(
                    "argument-error",
                    format!("ffi/array: count must be positive, got {}", n),
                ),
            )
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "ffi/array: expected integer for count, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };
    let desc = TypeDesc::Array(Box::new(elem_desc), count);
    (SIG_OK, Value::ffi_type(desc))
}

// ── Type introspection ──────────────────────────────────────────────

pub fn prim_ffi_size(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val("arity-error", "ffi/size: expected 1 argument"),
        );
    }
    let desc = match resolve_type_desc(&args[0], "ffi/size") {
        Ok(t) => t,
        Err(e) => return e,
    };
    match desc.size() {
        Some(s) => (SIG_OK, Value::int(s as i64)),
        None => (SIG_OK, Value::NIL),
    }
}

pub fn prim_ffi_align(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val("arity-error", "ffi/align: expected 1 argument"),
        );
    }
    let desc = match resolve_type_desc(&args[0], "ffi/align") {
        Ok(t) => t,
        Err(e) => return e,
    };
    match desc.align() {
        Some(a) => (SIG_OK, Value::int(a as i64)),
        None => (SIG_OK, Value::NIL),
    }
}

// ── Memory management ───────────────────────────────────────────────

pub fn prim_ffi_malloc(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val("arity-error", "ffi/malloc: expected 1 argument"),
        );
    }
    let size = match args[0].as_int() {
        Some(n) if n > 0 => n as usize,
        Some(_) => {
            return (
                SIG_ERROR,
                error_val("argument-error", "ffi/malloc: size must be positive"),
            )
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("ffi/malloc: expected integer, got {}", args[0].type_name()),
                ),
            )
        }
    };
    let ptr = unsafe { libc::malloc(size) };
    if ptr.is_null() {
        (
            SIG_ERROR,
            error_val("ffi-error", "ffi/malloc: allocation failed"),
        )
    } else {
        (SIG_OK, Value::managed_pointer(ptr as usize))
    }
}

pub fn prim_ffi_free(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val("arity-error", "ffi/free: expected 1 argument"),
        );
    }
    if args[0].is_nil() {
        return (SIG_OK, Value::NIL); // free(NULL) is a no-op
    }
    // Managed pointer: check not already freed, then invalidate
    if let Some(cell) = args[0].as_managed_pointer() {
        return match cell.get() {
            Some(addr) => {
                cell.set(None);
                unsafe { libc::free(addr as *mut libc::c_void) };
                (SIG_OK, Value::NIL)
            }
            None => (
                SIG_ERROR,
                error_val("double-free", "ffi/free: pointer has already been freed"),
            ),
        };
    }
    // Raw CPointer: free without lifecycle tracking (backwards compat)
    let addr = match args[0].as_pointer() {
        Some(a) => a,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("ffi/free: expected pointer, got {}", args[0].type_name()),
                ),
            )
        }
    };
    unsafe { libc::free(addr as *mut libc::c_void) };
    (SIG_OK, Value::NIL)
}

// ── Typed memory access ─────────────────────────────────────────────

pub fn prim_ffi_read(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val("arity-error", "ffi/read: expected 2 arguments"),
        );
    }
    let addr = match extract_pointer_addr(&args[0], "ffi/read") {
        Ok(a) => a,
        Err(e) => return e,
    };
    let desc = match resolve_type_desc(&args[1], "ffi/read") {
        Ok(t) => t,
        Err(e) => return e,
    };
    let ptr = addr as *const u8;
    unsafe {
        let val = match desc {
            TypeDesc::I8 | TypeDesc::Char => Value::int(*(ptr as *const i8) as i64),
            TypeDesc::U8 | TypeDesc::UChar => Value::int(*ptr as i64),
            TypeDesc::I16 | TypeDesc::Short => Value::int(*(ptr as *const i16) as i64),
            TypeDesc::U16 | TypeDesc::UShort => Value::int(*(ptr as *const u16) as i64),
            TypeDesc::I32 | TypeDesc::Int => Value::int(*(ptr as *const i32) as i64),
            TypeDesc::U32 | TypeDesc::UInt => Value::int(*(ptr as *const u32) as i64),
            TypeDesc::I64 | TypeDesc::Long | TypeDesc::SSize => Value::int(*(ptr as *const i64)),
            TypeDesc::U64 | TypeDesc::ULong | TypeDesc::Size => {
                Value::int(*(ptr as *const u64) as i64)
            }
            TypeDesc::Float => Value::float(*(ptr as *const f32) as f64),
            TypeDesc::Double => Value::float(*(ptr as *const f64)),
            TypeDesc::Bool => Value::bool(*(ptr as *const std::ffi::c_int) != 0),
            TypeDesc::Ptr => Value::pointer(*(ptr as *const usize)),
            TypeDesc::Str => {
                let cptr = *(ptr as *const *const std::ffi::c_char);
                if cptr.is_null() {
                    Value::NIL
                } else {
                    let cstr = std::ffi::CStr::from_ptr(cptr);
                    match cstr.to_str() {
                        Ok(s) => Value::string(s),
                        Err(_) => {
                            return (
                                SIG_ERROR,
                                error_val("ffi-error", "ffi/read: string is not valid UTF-8"),
                            )
                        }
                    }
                }
            }
            TypeDesc::Void => {
                return (
                    SIG_ERROR,
                    error_val("ffi-error", "ffi/read: cannot read void"),
                )
            }
            TypeDesc::Struct(_) | TypeDesc::Array(_, _) => {
                match crate::ffi::marshal::read_value_from_buffer(ptr, &desc) {
                    Ok(val) => val,
                    Err(e) => {
                        return (
                            SIG_ERROR,
                            error_val("ffi-error", format!("ffi/read: {}", e)),
                        )
                    }
                }
            }
        };
        (SIG_OK, val)
    }
}

pub fn prim_ffi_write(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 3 {
        return (
            SIG_ERROR,
            error_val("arity-error", "ffi/write: expected 3 arguments"),
        );
    }
    let addr = match extract_pointer_addr(&args[0], "ffi/write") {
        Ok(a) => a,
        Err(e) => return e,
    };
    let desc = match resolve_type_desc(&args[1], "ffi/write") {
        Ok(t) => t,
        Err(e) => return e,
    };

    let ptr = addr as *mut u8;
    let value = &args[2];

    unsafe {
        match desc {
            TypeDesc::I8 | TypeDesc::Char => {
                let n = match value.as_int() {
                    Some(n) => n as i8,
                    None => {
                        return (
                            SIG_ERROR,
                            error_val("type-error", "ffi/write: expected integer"),
                        )
                    }
                };
                *(ptr as *mut i8) = n;
            }
            TypeDesc::U8 | TypeDesc::UChar => {
                let n = match value.as_int() {
                    Some(n) => n as u8,
                    None => {
                        return (
                            SIG_ERROR,
                            error_val("type-error", "ffi/write: expected integer"),
                        )
                    }
                };
                *ptr = n;
            }
            TypeDesc::I16 | TypeDesc::Short => {
                let n = match value.as_int() {
                    Some(n) => n as i16,
                    None => {
                        return (
                            SIG_ERROR,
                            error_val("type-error", "ffi/write: expected integer"),
                        )
                    }
                };
                *(ptr as *mut i16) = n;
            }
            TypeDesc::U16 | TypeDesc::UShort => {
                let n = match value.as_int() {
                    Some(n) => n as u16,
                    None => {
                        return (
                            SIG_ERROR,
                            error_val("type-error", "ffi/write: expected integer"),
                        )
                    }
                };
                *(ptr as *mut u16) = n;
            }
            TypeDesc::I32 | TypeDesc::Int => {
                let n = match value.as_int() {
                    Some(n) => n as i32,
                    None => {
                        return (
                            SIG_ERROR,
                            error_val("type-error", "ffi/write: expected integer"),
                        )
                    }
                };
                *(ptr as *mut i32) = n;
            }
            TypeDesc::U32 | TypeDesc::UInt => {
                let n = match value.as_int() {
                    Some(n) => n as u32,
                    None => {
                        return (
                            SIG_ERROR,
                            error_val("type-error", "ffi/write: expected integer"),
                        )
                    }
                };
                *(ptr as *mut u32) = n;
            }
            TypeDesc::I64 | TypeDesc::Long | TypeDesc::SSize => {
                let n = match value.as_int() {
                    Some(n) => n,
                    None => {
                        return (
                            SIG_ERROR,
                            error_val("type-error", "ffi/write: expected integer"),
                        )
                    }
                };
                *(ptr as *mut i64) = n;
            }
            TypeDesc::U64 | TypeDesc::ULong | TypeDesc::Size => {
                let n = match value.as_int() {
                    Some(n) => n as u64,
                    None => {
                        return (
                            SIG_ERROR,
                            error_val("type-error", "ffi/write: expected integer"),
                        )
                    }
                };
                *(ptr as *mut u64) = n;
            }
            TypeDesc::Float => {
                let f = match value
                    .as_float()
                    .or_else(|| value.as_int().map(|i| i as f64))
                {
                    Some(f) => f,
                    None => {
                        return (
                            SIG_ERROR,
                            error_val("type-error", "ffi/write: expected number"),
                        )
                    }
                };
                *(ptr as *mut f32) = f as f32;
            }
            TypeDesc::Double => {
                let f = match value
                    .as_float()
                    .or_else(|| value.as_int().map(|i| i as f64))
                {
                    Some(f) => f,
                    None => {
                        return (
                            SIG_ERROR,
                            error_val("type-error", "ffi/write: expected number"),
                        )
                    }
                };
                *(ptr as *mut f64) = f;
            }
            TypeDesc::Bool => {
                *(ptr as *mut std::ffi::c_int) = if value.is_truthy() { 1 } else { 0 };
            }
            TypeDesc::Ptr => {
                let p = if value.is_nil() {
                    0usize
                } else if let Some(a) = value.as_pointer() {
                    a
                } else if let Some(cell) = value.as_managed_pointer() {
                    match cell.get() {
                        Some(a) => a,
                        None => {
                            return (
                                SIG_ERROR,
                                error_val(
                                    "use-after-free",
                                    "ffi/write: source pointer has been freed",
                                ),
                            )
                        }
                    }
                } else {
                    return (
                        SIG_ERROR,
                        error_val("type-error", "ffi/write: expected pointer"),
                    );
                };
                *(ptr as *mut usize) = p;
            }
            TypeDesc::Void => {
                return (
                    SIG_ERROR,
                    error_val("ffi-error", "ffi/write: cannot write void"),
                )
            }
            TypeDesc::Str => {
                return (
                    SIG_ERROR,
                    error_val("ffi-error", "ffi/write: use ptr type for writing pointers"),
                )
            }
            TypeDesc::Struct(_) | TypeDesc::Array(_, _) => {
                match crate::ffi::marshal::write_value_to_buffer(ptr, value, &desc) {
                    Ok(_owned) => {
                        // Note: owned data (CStrings for string fields) is dropped here.
                        // This is fine for ffi/write since the data has already been written
                        // to the buffer at this point.
                    }
                    Err(e) => {
                        return (
                            SIG_ERROR,
                            error_val("ffi-error", format!("ffi/write: {}", e)),
                        )
                    }
                }
            }
        }
    }
    (SIG_OK, Value::NIL)
}

// ── String from pointer ─────────────────────────────────────────────

pub(crate) fn prim_ffi_string(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            error_val("arity-error", "ffi/string: expected 1 or 2 arguments"),
        );
    }
    if args[0].is_nil() {
        return (SIG_OK, Value::NIL);
    }
    let addr = match extract_pointer_addr(&args[0], "ffi/string") {
        Ok(a) => a,
        Err(e) => return e,
    };

    let ptr = addr as *const std::ffi::c_char;
    unsafe {
        if args.len() == 2 {
            // Read up to N bytes
            let max_len = match args[1].as_int() {
                Some(n) if n >= 0 => n as usize,
                _ => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            "ffi/string: expected non-negative integer for length",
                        ),
                    )
                }
            };
            let slice = std::slice::from_raw_parts(ptr as *const u8, max_len);
            // Find null terminator within the slice
            let len = slice.iter().position(|&b| b == 0).unwrap_or(max_len);
            match std::str::from_utf8(&slice[..len]) {
                Ok(s) => (SIG_OK, Value::string(s)),
                Err(_) => (
                    SIG_ERROR,
                    error_val("ffi-error", "ffi/string: not valid UTF-8"),
                ),
            }
        } else {
            // Read null-terminated string
            let cstr = std::ffi::CStr::from_ptr(ptr);
            match cstr.to_str() {
                Ok(s) => (SIG_OK, Value::string(s)),
                Err(_) => (
                    SIG_ERROR,
                    error_val("ffi-error", "ffi/string: not valid UTF-8"),
                ),
            }
        }
    }
}

/// Declarative primitive definitions for FFI memory operations.
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "ffi/size",
        func: prim_ffi_size,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the size of a C type in bytes.",
        params: &["type"],
        category: "ffi",
        example: "(ffi/size :i32) #=> 4",
        aliases: &[],
    },
    PrimitiveDef {
        name: "ffi/align",
        func: prim_ffi_align,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the alignment of a C type in bytes.",
        params: &["type"],
        category: "ffi",
        example: "(ffi/align :double) #=> 8",
        aliases: &[],
    },
    PrimitiveDef {
        name: "ffi/malloc",
        func: prim_ffi_malloc,
        signal: Signal::ffi_errors(),
        arity: Arity::Exact(1),
        doc: "Allocate C memory.",
        params: &["size"],
        category: "ffi",
        example: "(ffi/malloc 100)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "ffi/free",
        func: prim_ffi_free,
        signal: Signal::ffi_errors(),
        arity: Arity::Exact(1),
        doc: "Free C memory.",
        params: &["ptr"],
        category: "ffi",
        example: "(ffi/free ptr)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "ffi/read",
        func: prim_ffi_read,
        signal: Signal::ffi_errors(),
        arity: Arity::Exact(2),
        doc: "Read a typed value from C memory.",
        params: &["ptr", "type"],
        category: "ffi",
        example: "(ffi/read ptr :i32)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "ffi/write",
        func: prim_ffi_write,
        signal: Signal::ffi_errors(),
        arity: Arity::Exact(3),
        doc: "Write a typed value to C memory.",
        params: &["ptr", "type", "value"],
        category: "ffi",
        example: "(ffi/write ptr :i32 42)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "ffi/string",
        func: prim_ffi_string,
        signal: Signal::ffi_errors(),
        arity: Arity::Range(1, 2),
        doc: "Read a null-terminated C string from a pointer.",
        params: &["ptr", "max-len"],
        category: "ffi",
        example: "(ffi/string ptr)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "ffi/struct",
        func: prim_ffi_struct,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Create a struct type descriptor from field types.",
        params: &["fields"],
        category: "ffi",
        example: "(ffi/struct [:i32 :double :ptr])",
        aliases: &[],
    },
    PrimitiveDef {
        name: "ffi/array",
        func: prim_ffi_array,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Create an array type descriptor from element type and count.",
        params: &["elem-type", "count"],
        category: "ffi",
        example: "(ffi/array :i32 10)",
        aliases: &[],
    },
];
