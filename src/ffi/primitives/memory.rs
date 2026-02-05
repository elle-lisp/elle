//! Memory management and safety checking primitives.

use super::types::parse_ctype;
use crate::ffi::memory::{get_memory_stats, register_allocation, MemoryOwner};
use crate::ffi::safety::{get_last_error, NullPointerChecker, TypeChecker};
use crate::value::Value;
use crate::vm::VM;

/// (register-allocation ptr type-name size owner) -> alloc-id
///
/// Registers a memory allocation for tracking.
///
/// # Arguments
/// - ptr: Pointer value (encoded as integer)
/// - type-name: Name of type (for debugging)
/// - size: Size in bytes
/// - owner: :elle or :c
pub fn prim_register_allocation(_vm: &mut VM, args: &[Value]) -> Result<Value, String> {
    if args.len() != 4 {
        return Err("register-allocation requires exactly 4 arguments".to_string());
    }

    let ptr = match &args[0] {
        Value::Int(id) => *id as *const std::ffi::c_void,
        _ => return Err("ptr must be an integer".to_string()),
    };

    let type_name = match &args[1] {
        Value::String(s) => s.as_ref(),
        _ => return Err("type-name must be a string".to_string()),
    };

    let size = match &args[2] {
        Value::Int(s) => *s as usize,
        _ => return Err("size must be an integer".to_string()),
    };

    let owner = match &args[3] {
        Value::String(s) => match s.as_ref() {
            "elle" => MemoryOwner::Elle,
            "c" => MemoryOwner::C,
            "shared" => MemoryOwner::Shared,
            _ => return Err("owner must be 'elle', 'c', or 'shared'".to_string()),
        },
        _ => return Err("owner must be a string".to_string()),
    };

    let alloc_id = register_allocation(ptr, type_name, size, owner);
    Ok(Value::Int(alloc_id as i64))
}

/// (memory-stats) -> (total-bytes allocation-count)
///
/// Returns memory allocation statistics.
pub fn prim_memory_stats(_vm: &mut VM, _args: &[Value]) -> Result<Value, String> {
    let (total_bytes, alloc_count) = get_memory_stats();

    let result = crate::value::cons(
        Value::Int(total_bytes as i64),
        crate::value::cons(Value::Int(alloc_count as i64), Value::Nil),
    );

    Ok(result)
}

/// (type-check value expected-type) -> bool
///
/// Type checks a value against expected C type.
pub fn prim_type_check(_vm: &mut VM, args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("type-check requires exactly 2 arguments".to_string());
    }

    let value = &args[0];
    let expected = parse_ctype(&args[1])?;

    match TypeChecker::check_type(value, &expected) {
        Ok(()) => Ok(Value::Int(1)),
        Err(_) => Ok(Value::Int(0)),
    }
}

/// (null-pointer? value) -> bool
///
/// Checks if a value represents a null pointer.
pub fn prim_null_pointer(_vm: &mut VM, args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("null-pointer? requires at least 1 argument".to_string());
    }

    let is_null = NullPointerChecker::is_null(&args[0]);
    Ok(Value::Int(if is_null { 1 } else { 0 }))
}

/// (ffi-last-error) -> error-message or nil
///
/// Gets the last FFI error, if any.
pub fn prim_ffi_last_error(_vm: &mut VM, _args: &[Value]) -> Result<Value, String> {
    match get_last_error() {
        Some(err) => Ok(Value::String(format!("{}", err).into())),
        None => Ok(Value::Nil),
    }
}

/// (with-ffi-safety-checks body) -> result
///
/// Executes body with FFI safety checks enabled.
/// Note: In a full implementation, this would catch segfaults.
pub fn prim_with_ffi_safety_checks(_vm: &mut VM, args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("with-ffi-safety-checks requires at least 1 argument".to_string());
    }

    // In a full implementation, this would:
    // 1. Install SIGSEGV handler (Linux)
    // 2. Execute body
    // 3. Catch segfaults and return error
    // 4. Restore signal handlers

    // For now, just return the first argument (assuming it's evaluated elsewhere)
    Ok(args[0].clone())
}

pub fn prim_register_allocation_wrapper(_args: &[Value]) -> Result<Value, String> {
    Ok(Value::Int(1))
}

pub fn prim_memory_stats_wrapper(_args: &[Value]) -> Result<Value, String> {
    let (total_bytes, alloc_count) = get_memory_stats();
    Ok(crate::value::list(vec![
        Value::Int(total_bytes as i64),
        Value::Int(alloc_count as i64),
    ]))
}

pub fn prim_type_check_wrapper(args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("type-check requires 2 arguments".to_string());
    }

    let value = &args[0];
    let expected = parse_ctype(&args[1])?;

    match TypeChecker::check_type(value, &expected) {
        Ok(()) => Ok(Value::Int(1)),
        Err(_) => Ok(Value::Int(0)),
    }
}

pub fn prim_null_pointer_wrapper(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("null-pointer? requires at least 1 argument".to_string());
    }

    let is_null = NullPointerChecker::is_null(&args[0]);
    Ok(Value::Int(if is_null { 1 } else { 0 }))
}

pub fn prim_ffi_last_error_wrapper(_args: &[Value]) -> Result<Value, String> {
    match get_last_error() {
        Some(err) => Ok(Value::String(format!("{}", err).into())),
        None => Ok(Value::Nil),
    }
}

pub fn prim_with_ffi_safety_checks_wrapper(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("with-ffi-safety-checks requires at least 1 argument".to_string());
    }
    Ok(args[0].clone())
}
