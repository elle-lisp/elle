//! C function pointer callback primitives.

use super::types::parse_ctype;
use crate::ffi::callback::{create_callback, register_callback, unregister_callback};
use crate::value::Value;
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
    let arg_types = match &args[1] {
        Value::Nil => vec![],
        Value::Cons(_) => {
            let type_list = args[1].list_to_vec()?;
            type_list
                .iter()
                .map(parse_ctype)
                .collect::<Result<Vec<_>, _>>()?
        }
        _ => return Err("arg-types must be a list".to_string()),
    };

    // Parse return type
    let return_type = parse_ctype(&args[2])?;

    // Create callback
    let (cb_id, _info) = create_callback(arg_types, return_type);

    // Register the closure with the callback registry
    let closure_rc = Rc::new(closure.clone());
    if !register_callback(cb_id, closure_rc) {
        return Err(format!("Failed to register callback with ID {}", cb_id));
    }

    // Return callback ID as integer
    Ok(Value::Int(cb_id as i64))
}

/// (free-callback callback-id) -> nil
///
/// Frees a callback by ID, unregistering it and cleaning up.
pub fn prim_free_callback(_vm: &mut VM, args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("free-callback requires exactly 1 argument".to_string());
    }

    let cb_id = match &args[0] {
        Value::Int(id) => *id as u32,
        _ => return Err("callback-id must be an integer".to_string()),
    };

    // Unregister the callback from the registry
    if unregister_callback(cb_id) {
        Ok(Value::Nil)
    } else {
        Err(format!("Callback with ID {} not found", cb_id))
    }
}

pub fn prim_make_c_callback_wrapper(args: &[Value]) -> Result<Value, String> {
    if args.len() != 3 {
        return Err("make-c-callback requires exactly 3 arguments".to_string());
    }

    let closure = &args[0];

    // Parse argument types
    let arg_types = match &args[1] {
        Value::Nil => vec![],
        Value::Cons(_) => {
            let type_list = args[1].list_to_vec()?;
            type_list
                .iter()
                .map(parse_ctype)
                .collect::<Result<Vec<_>, _>>()?
        }
        _ => return Err("arg-types must be a list".to_string()),
    };

    // Parse return type
    let return_type = parse_ctype(&args[2])?;

    // Create callback
    let (cb_id, _info) = create_callback(arg_types, return_type);

    // Register the closure with the callback registry
    let closure_rc = Rc::new(closure.clone());
    if !register_callback(cb_id, closure_rc) {
        return Err(format!("Failed to register callback with ID {}", cb_id));
    }

    // Return callback ID as integer
    Ok(Value::Int(cb_id as i64))
}

pub fn prim_free_callback_wrapper(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("free-callback requires exactly 1 argument".to_string());
    }

    let cb_id = match &args[0] {
        Value::Int(id) => *id as u32,
        _ => return Err("callback-id must be an integer".to_string()),
    };

    // Unregister the callback from the registry
    if unregister_callback(cb_id) {
        Ok(Value::Nil)
    } else {
        Err(format!("Callback with ID {} not found", cb_id))
    }
}
