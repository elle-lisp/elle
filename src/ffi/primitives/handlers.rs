//! FFI primitives for custom type handler registration and management.

use crate::ffi::handlers::{TypeHandler, TypeId};
use crate::ffi::marshal::CValue;
use crate::ffi::types::CType;
use crate::value::Value;
use crate::vm::VM;
use std::sync::Arc;

/// Built-in handler wrapper for Elle closures.
///
/// Note: This stores type information only. Actual closure invocation
/// requires VM context and must be handled at marshaling time.
#[derive(Clone)]
struct ClosureHandler {
    handler_name: String,
}

impl TypeHandler for ClosureHandler {
    fn elle_to_c(&self, _value: &Value, _ctype: &CType) -> Result<CValue, String> {
        // This is a placeholder - actual invocation would require VM context
        // For now, return an error indicating this needs VM integration
        Err(format!(
            "Closure-based handler '{}' requires VM context for invocation",
            self.handler_name
        ))
    }

    fn c_to_elle(&self, _cval: &CValue, _ctype: &CType) -> Result<Value, String> {
        // This is a placeholder - actual invocation would require VM context
        Err(format!(
            "Closure-based handler '{}' requires VM context for invocation",
            self.handler_name
        ))
    }

    fn can_handle(&self, _ctype: &CType) -> bool {
        true
    }
}

/// (define-custom-handler name elle-to-c-fn c-to-elle-fn priority) -> nil
///
/// Registers a custom type handler for marshaling between Elle and C.
///
/// # Arguments
/// - name: String name for the custom type
/// - elle-to-c-fn: Function taking (value ctype) -> CValue
/// - c-to-elle-fn: Function taking (cval ctype) -> Value
/// - priority: Integer priority (higher = first to try)
pub fn prim_define_custom_handler(vm: &VM, args: &[Value]) -> Result<Value, String> {
    if args.len() != 4 {
        return Err("define-custom-handler requires exactly 4 arguments".to_string());
    }

    let name = match &args[0] {
        Value::String(s) => s.as_ref().to_string(),
        _ => return Err("Handler name must be a string".to_string()),
    };

    let _priority = match &args[3] {
        Value::Int(p) => *p as i32,
        _ => return Err("Priority must be an integer".to_string()),
    };

    // Note: args[1] is elle_to_c_fn and args[2] is c_to_elle_fn
    // These would be used at marshaling time when VM context is available

    let handler = Arc::new(ClosureHandler {
        handler_name: name.clone(),
    });

    let type_id = TypeId::new(name);
    vm.ffi().handler_registry().register(type_id, handler)?;

    Ok(Value::Nil)
}

/// (unregister-custom-handler name) -> nil
///
/// Unregisters a custom type handler.
///
/// # Arguments
/// - name: String name of the handler to remove
pub fn prim_unregister_custom_handler(vm: &VM, args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("unregister-custom-handler requires exactly 1 argument".to_string());
    }

    let name = match &args[0] {
        Value::String(s) => s.as_ref().to_string(),
        _ => return Err("Handler name must be a string".to_string()),
    };

    let type_id = TypeId::new(name);
    vm.ffi().handler_registry().unregister(&type_id)?;

    Ok(Value::Nil)
}

/// (list-custom-handlers) -> ((name priority) ...)
///
/// Lists all registered custom type handlers.
pub fn prim_list_custom_handlers(vm: &VM, _args: &[Value]) -> Result<Value, String> {
    let handlers = vm.ffi().handler_registry().list_handlers()?;

    let mut result = Value::Nil;
    for (type_id, metadata) in handlers.into_iter().rev() {
        let entry = crate::value::cons(
            Value::String(type_id.name().into()),
            crate::value::cons(Value::Int(metadata.priority as i64), Value::Nil),
        );
        result = crate::value::cons(entry, result);
    }

    Ok(result)
}

/// (custom-handler-registered? name) -> bool
///
/// Checks if a custom type handler is registered.
///
/// # Arguments
/// - name: String name of the handler
pub fn prim_custom_handler_registered(vm: &VM, args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("custom-handler-registered? requires exactly 1 argument".to_string());
    }

    let name = match &args[0] {
        Value::String(s) => s.as_ref().to_string(),
        _ => return Err("Handler name must be a string".to_string()),
    };

    let type_id = TypeId::new(name);
    let registered = vm.ffi().handler_registry().has_handler(&type_id)?;

    Ok(Value::Int(if registered { 1 } else { 0 }))
}

/// (clear-custom-handlers) -> nil
///
/// Clears all registered custom type handlers.
pub fn prim_clear_custom_handlers(vm: &VM, _args: &[Value]) -> Result<Value, String> {
    vm.ffi().handler_registry().clear()?;
    Ok(Value::Nil)
}

// Wrapper functions for context-aware calls

pub fn prim_define_custom_handler_wrapper(args: &[Value]) -> Result<Value, String> {
    if args.len() != 4 {
        return Err("define-custom-handler requires exactly 4 arguments".to_string());
    }

    let name = match &args[0] {
        Value::String(s) => s.as_ref().to_string(),
        _ => return Err("Handler name must be a string".to_string()),
    };

    let _priority = match &args[3] {
        Value::Int(p) => *p as i32,
        _ => return Err("Priority must be an integer".to_string()),
    };

    // Note: args[1] is elle_to_c_fn and args[2] is c_to_elle_fn
    // These would be used at marshaling time when VM context is available

    let handler = Arc::new(ClosureHandler {
        handler_name: name.clone(),
    });

    let type_id = TypeId::new(name);

    // Get VM context
    let vm_ptr = super::context::get_vm_context().ok_or("FFI not initialized")?;
    unsafe {
        let vm = &*vm_ptr;
        vm.ffi().handler_registry().register(type_id, handler)?;
    }

    Ok(Value::Nil)
}

pub fn prim_unregister_custom_handler_wrapper(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("unregister-custom-handler requires exactly 1 argument".to_string());
    }

    let name = match &args[0] {
        Value::String(s) => s.as_ref().to_string(),
        _ => return Err("Handler name must be a string".to_string()),
    };

    let type_id = TypeId::new(name);

    let vm_ptr = super::context::get_vm_context().ok_or("FFI not initialized")?;
    unsafe {
        let vm = &*vm_ptr;
        vm.ffi().handler_registry().unregister(&type_id)?;
    }

    Ok(Value::Nil)
}

pub fn prim_list_custom_handlers_wrapper(_args: &[Value]) -> Result<Value, String> {
    let vm_ptr = super::context::get_vm_context().ok_or("FFI not initialized")?;
    unsafe {
        let vm = &*vm_ptr;
        let handlers = vm.ffi().handler_registry().list_handlers()?;

        let mut result = Value::Nil;
        for (type_id, metadata) in handlers.into_iter().rev() {
            let entry = crate::value::cons(
                Value::String(type_id.name().into()),
                crate::value::cons(Value::Int(metadata.priority as i64), Value::Nil),
            );
            result = crate::value::cons(entry, result);
        }

        Ok(result)
    }
}

pub fn prim_custom_handler_registered_wrapper(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("custom-handler-registered? requires exactly 1 argument".to_string());
    }

    let name = match &args[0] {
        Value::String(s) => s.as_ref().to_string(),
        _ => return Err("Handler name must be a string".to_string()),
    };

    let type_id = TypeId::new(name);

    let vm_ptr = super::context::get_vm_context().ok_or("FFI not initialized")?;
    unsafe {
        let vm = &*vm_ptr;
        let registered = vm.ffi().handler_registry().has_handler(&type_id)?;
        Ok(Value::Int(if registered { 1 } else { 0 }))
    }
}

pub fn prim_clear_custom_handlers_wrapper(_args: &[Value]) -> Result<Value, String> {
    let vm_ptr = super::context::get_vm_context().ok_or("FFI not initialized")?;
    unsafe {
        let vm = &*vm_ptr;
        vm.ffi().handler_registry().clear()?;
    }

    Ok(Value::Nil)
}
