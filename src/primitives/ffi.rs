//! FFI primitive functions for Elle.
//!
//! Provides the Elle-facing API for calling C functions:
//! library loading, symbol lookup, signature creation,
//! function calls, memory management, and typed memory access.

use crate::effects::Effect;
use crate::ffi::types::{CallingConvention, Signature, TypeDesc};
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

// ── Type descriptor resolution ──────────────────────────────────────

/// Resolve a type descriptor from a keyword or FFIType value.
///
/// Used by ffi/read, ffi/write, ffi/size, ffi/align, ffi/signature.
/// Returns the TypeDesc or an error tuple.
fn resolve_type_desc(value: &Value, context: &str) -> Result<TypeDesc, (SignalBits, Value)> {
    // First try keyword
    if let Some(name) = value.as_keyword_name() {
        return TypeDesc::from_keyword(name).ok_or_else(|| {
            (
                SIG_ERROR,
                error_val("ffi-error", format!("{}: unknown type :{}", context, name)),
            )
        });
    }
    // Then try FFIType value
    if let Some(desc) = value.as_ffi_type() {
        return Ok(desc.clone());
    }
    Err((
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "{}: expected keyword or ffi-type, got {}",
                context,
                value.type_name()
            ),
        ),
    ))
}

// ── Pointer extraction helper ───────────────────────────────────────

/// Extract a raw pointer address from a Value that is either a raw CPointer
/// or a managed pointer. Returns an error for nil, freed, or wrong-type values.
fn extract_pointer_addr(value: &Value, context: &str) -> Result<usize, (SignalBits, Value)> {
    if value.is_nil() {
        return Err((
            SIG_ERROR,
            error_val(
                "argument-error",
                format!("{}: cannot use null pointer", context),
            ),
        ));
    }
    // Raw CPointer (unmanaged — from ffi/lookup, ffi/call returns, etc.)
    if let Some(addr) = value.as_pointer() {
        return Ok(addr);
    }
    // Managed pointer (from ffi/malloc)
    if let Some(cell) = value.as_managed_pointer() {
        return match cell.get() {
            Some(addr) => Ok(addr),
            None => Err((
                SIG_ERROR,
                error_val(
                    "use-after-free",
                    format!("{}: pointer has been freed", context),
                ),
            )),
        };
    }
    Err((
        SIG_ERROR,
        error_val(
            "type-error",
            format!("{}: expected pointer, got {}", context, value.type_name()),
        ),
    ))
}

// ── Library loading ─────────────────────────────────────────────────

pub fn prim_ffi_native(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val("arity-error", "ffi/native: expected 1 argument"),
        );
    }
    let vm_ptr = match crate::context::get_vm_context() {
        Some(ptr) => ptr,
        None => {
            return (
                SIG_ERROR,
                error_val("ffi-error", "ffi/native: no VM context"),
            )
        }
    };
    let vm = unsafe { &mut *vm_ptr };

    // nil → load self process (dlopen(NULL))
    if args[0].is_nil() {
        return match vm.ffi_mut().load_self() {
            Ok(id) => (SIG_OK, Value::lib_handle(id)),
            Err(e) => (
                SIG_ERROR,
                error_val("ffi-error", format!("ffi/native: {}", e)),
            ),
        };
    }

    let path = match args[0].as_string() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "ffi/native: expected string or nil, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };
    match vm.ffi_mut().load_library(path) {
        Ok(id) => (SIG_OK, Value::lib_handle(id)),
        Err(e) => (
            SIG_ERROR,
            error_val("ffi-error", format!("ffi/native: {}", e)),
        ),
    }
}

// ── Symbol lookup ───────────────────────────────────────────────────

pub fn prim_ffi_lookup(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val("arity-error", "ffi/lookup: expected 2 arguments"),
        );
    }
    let lib_id = match args[0].as_lib_handle() {
        Some(id) => id,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "ffi/lookup: expected library handle, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };
    let sym_name = match args[1].as_string() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("ffi/lookup: expected string, got {}", args[1].type_name()),
                ),
            )
        }
    };
    let vm_ptr = match crate::context::get_vm_context() {
        Some(ptr) => ptr,
        None => {
            return (
                SIG_ERROR,
                error_val("ffi-error", "ffi/lookup: no VM context"),
            )
        }
    };
    let vm = unsafe { &*vm_ptr };
    let lib = match vm.ffi().get_library(lib_id) {
        Some(lib) => lib,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "ffi-error",
                    format!("ffi/lookup: library {} not loaded", lib_id),
                ),
            )
        }
    };
    match lib.get_symbol(sym_name) {
        Ok(ptr) => (SIG_OK, Value::pointer(ptr as usize)),
        Err(e) => (
            SIG_ERROR,
            error_val("ffi-error", format!("ffi/lookup: {}", e)),
        ),
    }
}

// ── Signature creation ──────────────────────────────────────────────

pub fn prim_ffi_signature(args: &[Value]) -> (SignalBits, Value) {
    if args.len() < 2 || args.len() > 3 {
        return (
            SIG_ERROR,
            error_val("arity-error", "ffi/signature: expected 2 or 3 arguments"),
        );
    }
    let ret = match resolve_type_desc(&args[0], "ffi/signature") {
        Ok(t) => t,
        Err(e) => return e,
    };

    // Parse argument types from array or list
    let arg_vals = if let Some(arr) = args[1].as_array() {
        arr.borrow().clone()
    } else {
        match args[1].list_to_vec() {
            Ok(v) => v,
            Err(_) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "ffi/signature: expected array or list for arg types, got {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        }
    };

    let mut arg_types = Vec::with_capacity(arg_vals.len());
    for val in &arg_vals {
        match resolve_type_desc(val, "ffi/signature") {
            Ok(t) => arg_types.push(t),
            Err(e) => return e,
        }
    }

    // Optional third arg: fixed_args count for variadic
    let fixed_args = if args.len() == 3 {
        match args[2].as_int() {
            Some(n) if n >= 0 && (n as usize) <= arg_types.len() => Some(n as usize),
            Some(n) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "argument-error",
                        format!(
                            "ffi/signature: fixed_args {} out of range [0, {}]",
                            n,
                            arg_types.len()
                        ),
                    ),
                )
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "ffi/signature: expected integer for fixed_args, got {}",
                            args[2].type_name()
                        ),
                    ),
                )
            }
        }
    } else {
        None
    };

    let sig = Signature {
        convention: CallingConvention::Default,
        ret,
        args: arg_types,
        fixed_args,
    };
    (SIG_OK, Value::ffi_signature(sig))
}

// ── Function call ───────────────────────────────────────────────────

pub fn prim_ffi_call(args: &[Value]) -> (SignalBits, Value) {
    if args.len() < 2 {
        return (
            SIG_ERROR,
            error_val("arity-error", "ffi/call: expected at least 2 arguments"),
        );
    }
    if args[0].is_nil() {
        return (
            SIG_ERROR,
            error_val("type-error", "ffi/call: function pointer is nil"),
        );
    }
    let fn_addr = match args[0].as_pointer() {
        Some(addr) => addr,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("ffi/call: expected pointer, got {}", args[0].type_name()),
                ),
            )
        }
    };

    let sig = match args[1].as_ffi_signature() {
        Some(s) => s.clone(),
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("ffi/call: expected signature, got {}", args[1].type_name()),
                ),
            )
        }
    };

    let call_args = &args[2..];

    // Get or prepare cached CIF
    let cif_ref = match args[1].get_or_prepare_cif() {
        Some(cif) => cif,
        None => {
            return (
                SIG_ERROR,
                error_val("ffi-error", "ffi/call: failed to get CIF from signature"),
            )
        }
    };

    let result = match unsafe {
        crate::ffi::call::ffi_call(
            fn_addr as *const std::ffi::c_void,
            call_args,
            &sig,
            &cif_ref,
        )
    } {
        Ok(val) => (SIG_OK, val),
        Err(e) => (
            SIG_ERROR,
            error_val("ffi-error", format!("ffi/call: {}", e)),
        ),
    };

    // Check for errors from FFI callbacks that ran during this call.
    // If a callback errored, it wrote a zero return value to C and
    // stored the error here. Propagate it to the Elle caller.
    if let Some(cb_err) = crate::ffi::callback::take_callback_error() {
        return (SIG_ERROR, cb_err);
    }

    result
}

// ── Struct/array type creation ──────────────────────────────────────

pub fn prim_ffi_struct(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val("arity-error", "ffi/struct: expected 1 argument"),
        );
    }
    // Accept array or list of type descriptors
    let field_vals = if let Some(arr) = args[0].as_array() {
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

pub fn prim_ffi_array(args: &[Value]) -> (SignalBits, Value) {
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

pub fn prim_ffi_string(args: &[Value]) -> (SignalBits, Value) {
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

// ── Callback creation ───────────────────────────────────────────────

pub fn prim_ffi_callback(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val("arity-error", "ffi/callback: expected 2 arguments"),
        );
    }
    let sig = match args[0].as_ffi_signature() {
        Some(s) => s.clone(),
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "ffi/callback: expected signature, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };
    let closure_rc = match args[1].as_closure() {
        Some(c) => c.clone(),
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "ffi/callback: expected closure, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };

    // Validate arity: closure must accept the right number of arguments
    let expected_args = sig.args.len();
    let arity_ok = match closure_rc.arity {
        Arity::Exact(n) => n == expected_args,
        Arity::AtLeast(n) => expected_args >= n,
        Arity::Range(min, max) => expected_args >= min && expected_args <= max,
    };
    if !arity_ok {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "ffi/callback: signature has {} args but closure has arity {}",
                    expected_args, closure_rc.arity
                ),
            ),
        );
    }

    let callback = match crate::ffi::callback::create_callback(closure_rc, sig) {
        Ok(cb) => cb,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val("ffi-error", format!("ffi/callback: {}", e)),
            )
        }
    };

    // Store the callback in the FFI subsystem so it stays alive
    let vm_ptr = match crate::context::get_vm_context() {
        Some(ptr) => ptr,
        None => {
            return (
                SIG_ERROR,
                error_val("ffi-error", "ffi/callback: no VM context"),
            )
        }
    };
    let vm = unsafe { &mut *vm_ptr };
    let code_ptr = vm.ffi_mut().callbacks_mut().insert(callback);

    (SIG_OK, Value::pointer(code_ptr))
}

pub fn prim_ffi_callback_free(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val("arity-error", "ffi/callback-free: expected 1 argument"),
        );
    }
    if args[0].is_nil() {
        return (SIG_OK, Value::NIL); // free(nil) is a no-op
    }
    let addr = match args[0].as_pointer() {
        Some(a) => a,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "ffi/callback-free: expected pointer, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };

    let vm_ptr = match crate::context::get_vm_context() {
        Some(ptr) => ptr,
        None => {
            return (
                SIG_ERROR,
                error_val("ffi-error", "ffi/callback-free: no VM context"),
            )
        }
    };
    let vm = unsafe { &mut *vm_ptr };
    if vm.ffi_mut().callbacks_mut().remove(addr) {
        (SIG_OK, Value::NIL)
    } else {
        (
            SIG_ERROR,
            error_val(
                "ffi-error",
                format!("ffi/callback-free: no callback at address {:#x}", addr),
            ),
        )
    }
}

// ── PRIMITIVES table ────────────────────────────────────────────────

pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "ffi/native",
        func: prim_ffi_native,
        effect: Effect::ffi_raises(),
        arity: Arity::Exact(1),
        doc: "Load a shared library. Pass nil for the current process.",
        params: &["path"],
        category: "ffi",
        example: "(ffi/native \"libm.so.6\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "ffi/lookup",
        func: prim_ffi_lookup,
        effect: Effect::ffi_raises(),
        arity: Arity::Exact(2),
        doc: "Look up a symbol in a loaded library.",
        params: &["lib", "name"],
        category: "ffi",
        example: "(ffi/lookup lib \"strlen\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "ffi/signature",
        func: prim_ffi_signature,
        effect: Effect::raises(),
        arity: Arity::Range(2, 3),
        doc: "Create a reified function signature. Optional third arg for variadic functions.",
        params: &["return-type", "arg-types", "fixed-args"],
        category: "ffi",
        example: "(ffi/signature :int [:ptr :size :ptr :int] 3)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "ffi/call",
        func: prim_ffi_call,
        effect: Effect::ffi_raises(),
        arity: Arity::AtLeast(2),
        doc: "Call a C function through libffi.",
        params: &["fn-ptr", "sig"],
        category: "ffi",
        example: "(ffi/call sqrt-ptr sig 2.0)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "ffi/size",
        func: prim_ffi_size,
        effect: Effect::raises(),
        arity: Arity::Exact(1),
        doc: "Get the size of a C type in bytes.",
        params: &["type"],
        category: "ffi",
        example: "(ffi/size :i32) ;=> 4",
        aliases: &[],
    },
    PrimitiveDef {
        name: "ffi/align",
        func: prim_ffi_align,
        effect: Effect::raises(),
        arity: Arity::Exact(1),
        doc: "Get the alignment of a C type in bytes.",
        params: &["type"],
        category: "ffi",
        example: "(ffi/align :double) ;=> 8",
        aliases: &[],
    },
    PrimitiveDef {
        name: "ffi/malloc",
        func: prim_ffi_malloc,
        effect: Effect::ffi_raises(),
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
        effect: Effect::ffi_raises(),
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
        effect: Effect::ffi_raises(),
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
        effect: Effect::ffi_raises(),
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
        effect: Effect::ffi_raises(),
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
        effect: Effect::raises(),
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
        effect: Effect::raises(),
        arity: Arity::Exact(2),
        doc: "Create an array type descriptor from element type and count.",
        params: &["elem-type", "count"],
        category: "ffi",
        example: "(ffi/array :i32 10)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "ffi/callback",
        func: prim_ffi_callback,
        effect: Effect::ffi_raises(),
        arity: Arity::Exact(2),
        doc: "Create a C function pointer from an Elle closure. Returns a pointer.",
        params: &["sig", "closure"],
        category: "ffi",
        example: "(ffi/callback (ffi/signature :int [:ptr :ptr]) (fn (a b) 0))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "ffi/callback-free",
        func: prim_ffi_callback_free,
        effect: Effect::ffi_raises(),
        arity: Arity::Exact(1),
        doc: "Free a callback created by ffi/callback.",
        params: &["ptr"],
        category: "ffi",
        example: "(ffi/callback-free cb-ptr)",
        aliases: &[],
    },
];

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ffi_size() {
        let result = prim_ffi_size(&[Value::keyword("i32")]);
        assert_eq!(result.0, SIG_OK);
        assert_eq!(result.1.as_int(), Some(4));
    }

    #[test]
    fn test_ffi_size_void() {
        let result = prim_ffi_size(&[Value::keyword("void")]);
        assert_eq!(result.0, SIG_OK);
        assert!(result.1.is_nil());
    }

    #[test]
    fn test_ffi_size_unknown_type() {
        let result = prim_ffi_size(&[Value::keyword("nonsense")]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_align() {
        let result = prim_ffi_align(&[Value::keyword("double")]);
        assert_eq!(result.0, SIG_OK);
        assert_eq!(result.1.as_int(), Some(8));
    }

    #[test]
    fn test_ffi_signature() {
        let result = prim_ffi_signature(&[
            Value::keyword("double"),
            Value::array(vec![Value::keyword("double")]),
        ]);
        assert_eq!(result.0, SIG_OK);
        assert!(result.1.as_ffi_signature().is_some());
    }

    #[test]
    fn test_ffi_signature_unknown_ret() {
        let result = prim_ffi_signature(&[Value::keyword("bad"), Value::array(vec![])]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_malloc_free() {
        let result = prim_ffi_malloc(&[Value::int(100)]);
        assert_eq!(result.0, SIG_OK);
        assert!(result.1.as_managed_pointer().is_some());
        let ptr_val = result.1;

        let free_result = prim_ffi_free(&[ptr_val]);
        assert_eq!(free_result.0, SIG_OK);
    }

    #[test]
    fn test_ffi_free_nil() {
        let result = prim_ffi_free(&[Value::NIL]);
        assert_eq!(result.0, SIG_OK);
    }

    #[test]
    fn test_ffi_malloc_zero() {
        let result = prim_ffi_malloc(&[Value::int(0)]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_read_write_i32() {
        let alloc_result = prim_ffi_malloc(&[Value::int(4)]);
        assert_eq!(alloc_result.0, SIG_OK);
        let ptr = alloc_result.1;

        let write_result = prim_ffi_write(&[ptr, Value::keyword("i32"), Value::int(42)]);
        assert_eq!(write_result.0, SIG_OK);

        let read_result = prim_ffi_read(&[ptr, Value::keyword("i32")]);
        assert_eq!(read_result.0, SIG_OK);
        assert_eq!(read_result.1.as_int(), Some(42));

        prim_ffi_free(&[ptr]);
    }

    #[test]
    fn test_ffi_read_write_double() {
        let alloc_result = prim_ffi_malloc(&[Value::int(8)]);
        assert_eq!(alloc_result.0, SIG_OK);
        let ptr = alloc_result.1;

        let write_result = prim_ffi_write(&[ptr, Value::keyword("double"), Value::float(1.234)]);
        assert_eq!(write_result.0, SIG_OK);

        let read_result = prim_ffi_read(&[ptr, Value::keyword("double")]);
        assert_eq!(read_result.0, SIG_OK);
        assert_eq!(read_result.1.as_float(), Some(1.234));

        prim_ffi_free(&[ptr]);
    }

    #[test]
    fn test_ffi_read_null_error() {
        let result = prim_ffi_read(&[Value::NIL, Value::keyword("i32")]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_call_arity_error() {
        let result = prim_ffi_call(&[]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_native_wrong_type() {
        let result = prim_ffi_native(&[Value::int(42)]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_signature_variadic() {
        let result = prim_ffi_signature(&[
            Value::keyword("int"),
            Value::array(vec![
                Value::keyword("ptr"),
                Value::keyword("string"),
                Value::keyword("int"),
            ]),
            Value::int(2),
        ]);
        assert_eq!(result.0, SIG_OK);
        let sig = result.1.as_ffi_signature().unwrap();
        assert_eq!(sig.fixed_args, Some(2));
    }

    #[test]
    fn test_ffi_signature_cif_caching() {
        let result = prim_ffi_signature(&[
            Value::keyword("int"),
            Value::array(vec![Value::keyword("int")]),
        ]);
        assert_eq!(result.0, SIG_OK);
        let sig_val = result.1;

        // First access prepares the CIF
        let cif1 = sig_val.get_or_prepare_cif();
        assert!(cif1.is_some());
        drop(cif1);

        // Second access reuses the cached CIF
        let cif2 = sig_val.get_or_prepare_cif();
        assert!(cif2.is_some());
    }

    #[test]
    fn test_ffi_signature_variadic_out_of_range() {
        let result = prim_ffi_signature(&[
            Value::keyword("int"),
            Value::array(vec![Value::keyword("int")]),
            Value::int(5), // 5 > 1 arg
        ]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_signature_variadic_bad_type() {
        let result = prim_ffi_signature(&[
            Value::keyword("int"),
            Value::array(vec![Value::keyword("int")]),
            Value::string("bad"),
        ]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_string_null() {
        let result = prim_ffi_string(&[Value::NIL]);
        assert_eq!(result.0, SIG_OK);
        assert!(result.1.is_nil());
    }

    #[test]
    fn test_ffi_string_from_buffer() {
        // Allocate a buffer, write "hello\0" into it, read it back
        let alloc = prim_ffi_malloc(&[Value::int(16)]);
        assert_eq!(alloc.0, SIG_OK);
        let ptr = alloc.1;
        let addr = ptr.as_managed_pointer().unwrap().get().unwrap();
        unsafe {
            let p = addr as *mut u8;
            for (i, &b) in b"hello\0".iter().enumerate() {
                *p.add(i) = b;
            }
        }
        let result = prim_ffi_string(&[ptr]);
        assert_eq!(result.0, SIG_OK);
        assert_eq!(result.1.as_string(), Some("hello"));

        // Also test with max-len
        let result2 = prim_ffi_string(&[ptr, Value::int(3)]);
        assert_eq!(result2.0, SIG_OK);
        assert_eq!(result2.1.as_string(), Some("hel"));

        prim_ffi_free(&[ptr]);
    }

    #[test]
    fn test_ffi_string_wrong_type() {
        let result = prim_ffi_string(&[Value::int(42)]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_string_arity() {
        let result = prim_ffi_string(&[]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_struct_basic() {
        let result = prim_ffi_struct(&[Value::array(vec![
            Value::keyword("i32"),
            Value::keyword("double"),
        ])]);
        assert_eq!(result.0, SIG_OK);
        assert!(result.1.as_ffi_type().is_some());
    }

    #[test]
    fn test_ffi_struct_nested() {
        // Create inner struct
        let inner_result = prim_ffi_struct(&[Value::array(vec![
            Value::keyword("i8"),
            Value::keyword("i32"),
        ])]);
        assert_eq!(inner_result.0, SIG_OK);
        let inner = inner_result.1;

        // Create outer struct using inner
        let result = prim_ffi_struct(&[Value::array(vec![Value::keyword("i64"), inner])]);
        assert_eq!(result.0, SIG_OK);
    }

    #[test]
    fn test_ffi_struct_empty() {
        let result = prim_ffi_struct(&[Value::array(vec![])]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_struct_void_field() {
        let result = prim_ffi_struct(&[Value::array(vec![Value::keyword("void")])]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_array_basic() {
        let result = prim_ffi_array(&[Value::keyword("i32"), Value::int(10)]);
        assert_eq!(result.0, SIG_OK);
        assert!(result.1.as_ffi_type().is_some());
    }

    #[test]
    fn test_ffi_array_zero_count() {
        let result = prim_ffi_array(&[Value::keyword("i32"), Value::int(0)]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_array_negative_count() {
        let result = prim_ffi_array(&[Value::keyword("i32"), Value::int(-5)]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_size_with_struct() {
        // Create a struct type
        let struct_result = prim_ffi_struct(&[Value::array(vec![
            Value::keyword("i32"),
            Value::keyword("i32"),
        ])]);
        assert_eq!(struct_result.0, SIG_OK);
        let struct_type = struct_result.1;

        // Get its size
        let size_result = prim_ffi_size(&[struct_type]);
        assert_eq!(size_result.0, SIG_OK);
        assert_eq!(size_result.1.as_int(), Some(8));
    }

    #[test]
    fn test_ffi_align_with_struct() {
        let struct_result = prim_ffi_struct(&[Value::array(vec![
            Value::keyword("i8"),
            Value::keyword("double"),
        ])]);
        assert_eq!(struct_result.0, SIG_OK);
        let struct_type = struct_result.1;

        let align_result = prim_ffi_align(&[struct_type]);
        assert_eq!(align_result.0, SIG_OK);
        assert_eq!(align_result.1.as_int(), Some(8));
    }

    #[test]
    fn test_ffi_signature_with_struct() {
        // Create a struct type for use in signature
        let struct_result = prim_ffi_struct(&[Value::array(vec![
            Value::keyword("i32"),
            Value::keyword("double"),
        ])]);
        assert_eq!(struct_result.0, SIG_OK);
        let struct_type = struct_result.1;

        // Use struct as return type
        let sig_result =
            prim_ffi_signature(&[struct_type, Value::array(vec![Value::keyword("ptr")])]);
        assert_eq!(sig_result.0, SIG_OK);

        // Use struct as argument type
        let sig_result2 =
            prim_ffi_signature(&[Value::keyword("void"), Value::array(vec![struct_type])]);
        assert_eq!(sig_result2.0, SIG_OK);
    }

    #[test]
    fn test_ffi_read_write_struct() {
        // Create a struct type
        let struct_result = prim_ffi_struct(&[Value::array(vec![
            Value::keyword("i32"),
            Value::keyword("double"),
        ])]);
        assert_eq!(struct_result.0, SIG_OK);
        let struct_type = struct_result.1;

        // Allocate memory for the struct
        let size = prim_ffi_size(&[struct_type]);
        let alloc_result = prim_ffi_malloc(&[size.1]);
        assert_eq!(alloc_result.0, SIG_OK);
        let ptr = alloc_result.1;

        // Write struct
        let test_float = 2.5;
        let struct_val = Value::array(vec![Value::int(42), Value::float(test_float)]);
        let write_result = prim_ffi_write(&[ptr, struct_type, struct_val]);
        assert_eq!(write_result.0, SIG_OK);

        // Read struct back
        let read_result = prim_ffi_read(&[ptr, struct_type]);
        assert_eq!(read_result.0, SIG_OK);
        let arr = read_result.1.as_array().unwrap();
        let arr = arr.borrow();
        assert_eq!(arr[0].as_int(), Some(42));
        assert!((arr[1].as_float().unwrap() - test_float).abs() < 1e-10);

        prim_ffi_free(&[ptr]);
    }
}
