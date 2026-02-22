//! C function pointer callback primitives.

use super::types::parse_ctype;
use crate::ffi::callback::{create_callback, register_callback, unregister_callback};
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, Value};
use crate::vm::VM;
use std::rc::Rc;

/// (make-c-callback closure arg-types return-type) -> callback-handle
///
/// Creates a C callback from an Elle closure.
///
/// # Arguments
/// - closure: Elle function/closure to call
/// - arg-types: List of argument types
/// - return-type: Return type of callback
pub fn prim_make_c_callback(_vm: &mut VM, args: &[Value]) -> Result<Value, String> {
    if args.len() != 3 {
        return Err("make-c-callback requires exactly 3 arguments".to_string());
    }

    let closure = &args[0];

    // Parse argument types
    let arg_types = if args[1].is_nil() {
        vec![]
    } else {
        let type_list = args[1].list_to_vec()?;
        type_list
            .iter()
            .map(parse_ctype)
            .collect::<Result<Vec<_>, _>>()?
    };

    // Parse return type
    let return_type = parse_ctype(&args[2])?;

    // Create callback
    let (cb_id, _info) = create_callback(arg_types, return_type);

    // Register the closure with the callback registry
    let closure_rc = Rc::new(*closure);
    if !register_callback(cb_id, closure_rc) {
        return Err(format!("Failed to register callback with ID {}", cb_id));
    }

    // Return callback ID as integer
    Ok(Value::int(cb_id as i64))
}

/// (free-callback callback-id) -> nil
///
/// Frees a callback by ID, unregistering it and cleaning up.
pub fn prim_free_callback(_vm: &mut VM, args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("free-callback requires exactly 1 argument".to_string());
    }

    let cb_id = args[0].as_int().ok_or("callback-id must be an integer")? as u32;

    // Unregister the callback from the registry
    if unregister_callback(cb_id) {
        Ok(Value::NIL)
    } else {
        Err(format!("Callback with ID {} not found", cb_id))
    }
}

pub fn prim_make_c_callback_wrapper(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 3 {
        return (
            SIG_ERROR,
            error_val("arity-error", "make-c-callback: expected 3 arguments"),
        );
    }

    let closure = &args[0];

    // Parse argument types
    let arg_types = if args[1].is_nil() {
        vec![]
    } else {
        let type_list = match args[1].list_to_vec() {
            Ok(list) => list,
            Err(e) => {
                return (
                    SIG_ERROR,
                    error_val("type-error", format!("make-c-callback: {}", e)),
                );
            }
        };
        match type_list
            .iter()
            .map(parse_ctype)
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(types) => types,
            Err(e) => {
                return (SIG_ERROR, error_val("error", e));
            }
        }
    };

    // Parse return type
    let return_type = match parse_ctype(&args[2]) {
        Ok(ty) => ty,
        Err(e) => {
            return (SIG_ERROR, error_val("error", e));
        }
    };

    // Create callback
    let (cb_id, _info) = create_callback(arg_types, return_type);

    // Register the closure with the callback registry
    let closure_rc = Rc::new(*closure);
    if !register_callback(cb_id, closure_rc) {
        return (
            SIG_ERROR,
            error_val(
                "error",
                format!(
                    "make-c-callback: failed to register callback with ID {}",
                    cb_id
                ),
            ),
        );
    }

    // Return callback ID as integer
    (SIG_OK, Value::int(cb_id as i64))
}

/// (free-callback callback-id) -> nil
///
/// Frees a callback by ID, unregistering it and cleaning up.
pub fn prim_free_callback_wrapper(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val("arity-error", "free-callback: expected 1 argument"),
        );
    }

    let cb_id = match args[0].as_int() {
        Some(id) => id as u32,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    "free-callback: callback-id must be an integer",
                ),
            );
        }
    };

    // Unregister the callback from the registry
    if unregister_callback(cb_id) {
        (SIG_OK, Value::NIL)
    } else {
        (
            SIG_ERROR,
            error_val(
                "error",
                format!("free-callback: callback with ID {} not found", cb_id),
            ),
        )
    }
}
