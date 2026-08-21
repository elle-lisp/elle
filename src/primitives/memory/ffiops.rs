use super::*;

pub fn prim_ffi_read(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let addr = match extract_pointer_addr(&args[0], "ffi/read", ctx) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let desc = match resolve_type_desc(&args[1], "ffi/read", ctx) {
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
                        Ok(s) => ctx.string(s),
                        Err(_) => {
                            return (
                                SIG_ERROR,
                                ctx.error("ffi-error", "ffi/read: string is not valid UTF-8"),
                            )
                        }
                    }
                }
            }
            TypeDesc::Void => {
                return (
                    SIG_ERROR,
                    ctx.error("ffi-error", "ffi/read: cannot read void"),
                )
            }
            #[cfg(feature = "ffi")]
            TypeDesc::Struct(_) | TypeDesc::Array(_, _) => {
                match crate::ffi::marshal::read_value_from_buffer(ptr, &desc, ctx) {
                    Ok(val) => val,
                    Err(e) => {
                        return (
                            SIG_ERROR,
                            ctx.error("ffi-error", format!("ffi/read: {}", e)),
                        )
                    }
                }
            }
            #[cfg(not(feature = "ffi"))]
            TypeDesc::Struct(_) | TypeDesc::Array(_, _) => {
                return (
                    SIG_ERROR,
                    ctx.error("ffi-error", "ffi/read: struct/array requires `ffi` feature"),
                )
            }
        };
        (SIG_OK, val)
    }
}
pub fn prim_ffi_write(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let addr = match extract_pointer_addr(&args[0], "ffi/write", ctx) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let desc = match resolve_type_desc(&args[1], "ffi/write", ctx) {
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
                            ctx.error("type-error", "ffi/write: expected integer"),
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
                            ctx.error("type-error", "ffi/write: expected integer"),
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
                            ctx.error("type-error", "ffi/write: expected integer"),
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
                            ctx.error("type-error", "ffi/write: expected integer"),
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
                            ctx.error("type-error", "ffi/write: expected integer"),
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
                            ctx.error("type-error", "ffi/write: expected integer"),
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
                            ctx.error("type-error", "ffi/write: expected integer"),
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
                            ctx.error("type-error", "ffi/write: expected integer"),
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
                            ctx.error("type-error", "ffi/write: expected number"),
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
                            ctx.error("type-error", "ffi/write: expected number"),
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
                                ctx.error(
                                    "use-after-free",
                                    "ffi/write: source pointer has been freed",
                                ),
                            )
                        }
                    }
                } else {
                    return (
                        SIG_ERROR,
                        ctx.error("type-error", "ffi/write: expected pointer"),
                    );
                };
                *(ptr as *mut usize) = p;
            }
            TypeDesc::Void => {
                return (
                    SIG_ERROR,
                    ctx.error("ffi-error", "ffi/write: cannot write void"),
                )
            }
            TypeDesc::Str => {
                return (
                    SIG_ERROR,
                    ctx.error("ffi-error", "ffi/write: use ptr type for writing pointers"),
                )
            }
            #[cfg(feature = "ffi")]
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
                            ctx.error("ffi-error", format!("ffi/write: {}", e)),
                        )
                    }
                }
            }
            #[cfg(not(feature = "ffi"))]
            TypeDesc::Struct(_) | TypeDesc::Array(_, _) => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "ffi-error",
                        "ffi/write: struct/array requires `ffi` feature",
                    ),
                )
            }
        }
    }
    (SIG_OK, Value::NIL)
}
pub(crate) fn prim_ffi_string(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args[0].is_nil() {
        return (SIG_OK, Value::NIL);
    }
    let addr = match extract_pointer_addr(&args[0], "ffi/string", ctx) {
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
                        ctx.error(
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
                Ok(s) => (SIG_OK, ctx.string(s)),
                Err(_) => (
                    SIG_ERROR,
                    ctx.error("ffi-error", "ffi/string: not valid UTF-8"),
                ),
            }
        } else {
            // Read null-terminated string
            let cstr = std::ffi::CStr::from_ptr(ptr);
            match cstr.to_str() {
                Ok(s) => (SIG_OK, ctx.string(s)),
                Err(_) => (
                    SIG_ERROR,
                    ctx.error("ffi-error", "ffi/string: not valid UTF-8"),
                ),
            }
        }
    }
}
