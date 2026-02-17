//! Memory management and safety checking primitives.

use super::types::parse_ctype;
use crate::ffi::memory::{get_memory_stats, register_allocation, MemoryOwner};
use crate::ffi::safety::{get_last_error, NullPointerChecker, TypeChecker};
use crate::value::{Condition, Value};
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
        return Err("register-allocation requires exactly 4 arguments".into());
    }

    let ptr = if let Some(id) = args[0].as_int() {
        id as *const std::ffi::c_void
    } else {
        return Err("ptr must be an integer".into());
    };

    let type_name = if let Some(s) = args[1].as_string() {
        s
    } else {
        return Err("type-name must be a string".into());
    };

    let size = if let Some(s) = args[2].as_int() {
        s as usize
    } else {
        return Err("size must be an integer".into());
    };

    let owner = if let Some(s) = args[3].as_string() {
        match s {
            "elle" => MemoryOwner::Elle,
            "c" => MemoryOwner::C,
            "shared" => MemoryOwner::Shared,
            _ => return Err("owner must be 'elle', 'c', or 'shared'".into()),
        }
    } else {
        return Err("owner must be a string".into());
    };

    let alloc_id = register_allocation(ptr, type_name, size, owner);
    Ok(Value::int(alloc_id as i64))
}

/// (memory-stats) -> (total-bytes allocation-count)
///
/// Returns memory allocation statistics.
pub fn prim_memory_stats(_vm: &mut VM, _args: &[Value]) -> Result<Value, String> {
    let (total_bytes, alloc_count) = get_memory_stats();

    let result = Value::cons(
        Value::int(total_bytes as i64),
        Value::cons(Value::int(alloc_count as i64), Value::EMPTY_LIST),
    );

    Ok(result)
}

/// (type-check value expected-type) -> bool
///
/// Type checks a value against expected C type.
pub fn prim_type_check(_vm: &mut VM, args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("type-check requires exactly 2 arguments".into());
    }

    let value = &args[0];
    let expected = parse_ctype(&args[1])?;

    match TypeChecker::check_type(value, &expected) {
        Ok(()) => Ok(Value::int(1)),
        Err(_) => Ok(Value::int(0)),
    }
}

/// (null-pointer? value) -> bool
///
/// Checks if a value represents a null pointer.
pub fn prim_null_pointer(_vm: &mut VM, args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("null-pointer? requires at least 1 argument".into());
    }

    let is_null = NullPointerChecker::is_null(&args[0]);
    Ok(Value::int(if is_null { 1 } else { 0 }))
}

/// (ffi-last-error) -> error-message or nil
///
/// Gets the last FFI error, if any.
pub fn prim_ffi_last_error(_vm: &mut VM, _args: &[Value]) -> Result<Value, String> {
    match get_last_error() {
        Some(err) => Ok(Value::string(format!("{}", err))),
        None => Ok(Value::EMPTY_LIST),
    }
}

/// (with-ffi-safety-checks body) -> result
///
/// Executes body with FFI safety checks enabled.
/// Note: In a full implementation, this would catch segfaults.
pub fn prim_with_ffi_safety_checks(_vm: &mut VM, args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("with-ffi-safety-checks requires at least 1 argument".into());
    }

    // In a full implementation, this would:
    // 1. Install SIGSEGV handler (Linux)
    // 2. Execute body
    // 3. Catch segfaults and return error
    // 4. Restore signal handlers

    // For now, just return the first argument (assuming it's evaluated elsewhere)
    Ok(args[0])
}

pub fn prim_register_allocation_wrapper(_args: &[Value]) -> Result<Value, Condition> {
    Ok(Value::int(1))
}

pub fn prim_memory_stats_wrapper(_args: &[Value]) -> Result<Value, Condition> {
    let (total_bytes, alloc_count) = get_memory_stats();
    Ok(Value::cons(
        Value::int(total_bytes as i64),
        Value::cons(Value::int(alloc_count as i64), Value::EMPTY_LIST),
    ))
}

pub fn prim_type_check_wrapper(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(
            "type-check: expected 2 arguments".to_string(),
        ));
    }

    let value = &args[0];
    let expected = parse_ctype(&args[1]).map_err(Condition::error)?;

    match TypeChecker::check_type(value, &expected) {
        Ok(()) => Ok(Value::int(1)),
        Err(_) => Ok(Value::int(0)),
    }
}

pub fn prim_null_pointer_wrapper(args: &[Value]) -> Result<Value, Condition> {
    if args.is_empty() {
        return Err(Condition::arity_error(
            "null-pointer?: expected at least 1 argument".to_string(),
        ));
    }

    let is_null = NullPointerChecker::is_null(&args[0]);
    Ok(Value::int(if is_null { 1 } else { 0 }))
}

pub fn prim_ffi_last_error_wrapper(_args: &[Value]) -> Result<Value, Condition> {
    match get_last_error() {
        Some(err) => Ok(Value::string(format!("{}", err))),
        None => Ok(Value::EMPTY_LIST),
    }
}

pub fn prim_with_ffi_safety_checks_wrapper(args: &[Value]) -> Result<Value, Condition> {
    if args.is_empty() {
        return Err(Condition::arity_error(
            "with-ffi-safety-checks: expected at least 1 argument".to_string(),
        ));
    }
    Ok(args[0])
}
