//! C function calling primitives.

use super::types::parse_ctype;
use crate::ffi::call::FunctionCall;
use crate::ffi::types::FunctionSignature;
use crate::value::{Condition, Value};
use crate::vm::VM;

/// (call-c-function lib-id func-name return-type (arg-type ...) (arg-val ...)) -> result
///
/// Calls a C function with given arguments.
///
/// # Arguments
/// - lib-id: Library handle (from load-library)
/// - func-name: Name of C function as string
/// - return-type: Return type keyword (:int, :float, :double, :void, :pointer, etc.)
/// - arg-types: List of argument type keywords
/// - arg-values: List of argument values to pass
pub fn prim_call_c_function(vm: &VM, args: &[Value]) -> Result<Value, String> {
    if args.len() != 5 {
        return Err("call-c-function requires exactly 5 arguments".into());
    }

    // Parse library ID
    let lib_id = args[0]
        .as_int()
        .ok_or("First argument must be a library handle")? as u32;

    // Parse function name
    let func_name = args[1]
        .as_string()
        .ok_or("Second argument must be a function name string")?;

    // Parse return type
    let return_type = parse_ctype(&args[2])?;

    // Parse argument types
    let arg_types = if args[3].is_nil() {
        vec![]
    } else {
        let type_list = args[3].list_to_vec()?;
        type_list
            .iter()
            .map(parse_ctype)
            .collect::<Result<Vec<_>, _>>()?
    };

    // Parse argument values
    let arg_values = if args[4].is_nil() {
        vec![]
    } else {
        args[4].list_to_vec()?
    };

    // Check argument count matches
    if arg_types.len() != arg_values.len() {
        return Err(format!(
            "Argument count mismatch: expected {}, got {}",
            arg_types.len(),
            arg_values.len()
        ));
    }

    // Create function signature first
    let sig = FunctionSignature::new(func_name.to_string(), arg_types, return_type);

    // Get library and resolve symbol
    let lib = vm
        .ffi()
        .get_library(lib_id)
        .ok_or("Library handle not found".to_string())?;

    // Get function pointer directly from library
    let func_ptr = lib.get_symbol(func_name)?;

    // Create and execute function call
    let call = FunctionCall::new(sig, func_ptr)?;
    call.call(&arg_values)
}

pub fn prim_call_c_function_wrapper(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 5 {
        return Err(Condition::arity_error(
            "call-c-function: expected 5 arguments".to_string(),
        ));
    }

    // Parse library ID
    let lib_id = args[0].as_int().ok_or_else(|| {
        Condition::type_error(
            "call-c-function: first argument must be a library handle".to_string(),
        )
    })? as u32;

    // Parse function name
    let func_name = args[1].as_string().ok_or_else(|| {
        Condition::type_error(
            "call-c-function: second argument must be a function name string".to_string(),
        )
    })?;

    // Parse return type
    let return_type = parse_ctype(&args[2]).map_err(Condition::error)?;

    // Parse argument types
    let arg_types = if args[3].is_nil() {
        vec![]
    } else {
        let type_list = args[3]
            .list_to_vec()
            .map_err(|e| Condition::type_error(format!("call-c-function: {}", e)))?;
        type_list
            .iter()
            .map(parse_ctype)
            .collect::<Result<Vec<_>, _>>()
            .map_err(Condition::error)?
    };

    // Parse argument values
    let arg_values = if args[4].is_nil() {
        vec![]
    } else {
        args[4]
            .list_to_vec()
            .map_err(|e| Condition::type_error(format!("call-c-function: {}", e)))?
    };

    // Check argument count matches
    if arg_types.len() != arg_values.len() {
        return Err(Condition::arity_error(format!(
            "call-c-function: argument count mismatch: expected {}, got {}",
            arg_types.len(),
            arg_values.len()
        )));
    }

    // Create function signature
    let sig = FunctionSignature::new(func_name.to_string(), arg_types, return_type);

    // Get VM context
    let vm_ptr = super::context::get_vm_context()
        .ok_or_else(|| Condition::error("FFI not initialized".to_string()))?;
    unsafe {
        let vm = &*vm_ptr;
        let lib = vm
            .ffi()
            .get_library(lib_id)
            .ok_or_else(|| Condition::error("Library handle not found".to_string()))?;

        let func_ptr = lib.get_symbol(func_name).map_err(Condition::error)?;
        let call = FunctionCall::new(sig, func_ptr).map_err(Condition::error)?;
        call.call(&arg_values).map_err(Condition::error)
    }
}
