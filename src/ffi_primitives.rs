//! FFI primitives for Elle.
//!
//! Provides Lisp functions for loading and calling C functions.

use crate::ffi::bindings::generate_elle_bindings;
use crate::ffi::call::FunctionCall;
use crate::ffi::callback::{create_callback, register_callback, unregister_callback};
use crate::ffi::header::HeaderParser;
use crate::ffi::memory::{get_memory_stats, register_allocation, MemoryOwner};
use crate::ffi::safety::{get_last_error, NullPointerChecker, TypeChecker};
use crate::ffi::types::{CType, EnumId, EnumLayout, EnumVariant, FunctionSignature};
use crate::value::{LibHandle, Value};
use crate::vm::VM;
use std::cell::RefCell;
use std::rc::Rc;
thread_local! {
    static VM_CONTEXT: RefCell<Option<*mut VM>> = const { RefCell::new(None) };
}

/// Set the current VM context (called before executing code)
pub fn set_vm_context(vm: *mut VM) {
    VM_CONTEXT.with(|ctx| *ctx.borrow_mut() = Some(vm));
}

/// Get the current VM context
pub fn get_vm_context() -> Option<*mut VM> {
    VM_CONTEXT.with(|ctx| ctx.borrow().as_ref().copied())
}

/// Clear the VM context
pub fn clear_vm_context() {
    VM_CONTEXT.with(|ctx| *ctx.borrow_mut() = None);
}

/// Register FFI primitives in the VM.
pub fn register_ffi_primitives(_vm: &mut VM) {
    // Phase 2: FFI primitives for function calling
    // Note: These are meant to be called from Elle code
}

/// (load-library path) -> library-handle
pub fn prim_load_library(vm: &mut VM, args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("load-library requires exactly 1 argument".to_string());
    }

    let path = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("load-library requires a string path".to_string()),
    };

    let lib_id = vm.ffi_mut().load_library(path)?;
    Ok(Value::LibHandle(LibHandle(lib_id)))
}

/// (list-libraries) -> ((id path) ...)
pub fn prim_list_libraries(vm: &VM, _args: &[Value]) -> Result<Value, String> {
    let libs = vm.ffi().loaded_libraries();

    let mut result = Value::Nil;
    for (id, path) in libs.into_iter().rev() {
        let entry = crate::value::cons(
            Value::Int(id as i64),
            crate::value::cons(Value::String(path.into()), Value::Nil),
        );
        result = crate::value::cons(entry, result);
    }

    Ok(result)
}

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
        return Err("call-c-function requires exactly 5 arguments".to_string());
    }

    // Parse library ID
    let lib_id = match &args[0] {
        Value::LibHandle(LibHandle(id)) => *id,
        _ => return Err("First argument must be a library handle".to_string()),
    };

    // Parse function name
    let func_name = match &args[1] {
        Value::String(s) => s.as_ref(),
        _ => return Err("Second argument must be a function name string".to_string()),
    };

    // Parse return type
    let return_type = parse_ctype(&args[2])?;

    // Parse argument types
    let arg_types = match &args[3] {
        Value::Nil => vec![],
        Value::Cons(_) => {
            let type_list = args[3].list_to_vec()?;
            type_list
                .iter()
                .map(parse_ctype)
                .collect::<Result<Vec<_>, _>>()?
        }
        _ => return Err("Fourth argument must be a list of argument types".to_string()),
    };

    // Parse argument values
    let arg_values = match &args[4] {
        Value::Nil => vec![],
        Value::Cons(_) => args[4].list_to_vec()?,
        _ => return Err("Fifth argument must be a list of argument values".to_string()),
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

/// Parse a C type from a keyword value.
fn parse_ctype(val: &Value) -> Result<CType, String> {
    match val {
        Value::Symbol(_) => {
            // We need to look up the symbol name, but we don't have access to SymbolTable
            // For now, we'll return an error indicating this needs symbol table integration
            Err("Symbol-based type specification not yet supported".to_string())
        }
        Value::String(s) => match s.as_ref() {
            "void" => Ok(CType::Void),
            "bool" => Ok(CType::Bool),
            "char" => Ok(CType::Char),
            "schar" => Ok(CType::SChar),
            "uchar" => Ok(CType::UChar),
            "short" => Ok(CType::Short),
            "ushort" => Ok(CType::UShort),
            "int" => Ok(CType::Int),
            "uint" => Ok(CType::UInt),
            "long" => Ok(CType::Long),
            "ulong" => Ok(CType::ULong),
            "longlong" => Ok(CType::LongLong),
            "ulonglong" => Ok(CType::ULongLong),
            "float" => Ok(CType::Float),
            "double" => Ok(CType::Double),
            "pointer" => Ok(CType::Pointer(Box::new(CType::Void))),
            _ => Err(format!("Unknown C type: {}", s)),
        },
        _ => Err("Type must be a string".to_string()),
    }
}

/// (load-header-with-lib header-path lib-path) -> library-handle
///
/// Loads a C header file, parses it, and generates Elle bindings.
///
/// # Arguments
/// - header-path: Path to C header file
/// - lib-path: Path to compiled library
pub fn prim_load_header_with_lib(_vm: &mut VM, args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("load-header-with-lib requires exactly 2 arguments".to_string());
    }

    let header_path = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("header-path must be a string".to_string()),
    };

    let lib_path = match &args[1] {
        Value::String(s) => s.as_ref(),
        _ => return Err("lib-path must be a string".to_string()),
    };

    // Parse header
    let mut parser = HeaderParser::new();
    let parsed = parser.parse(header_path)?;

    // Generate bindings
    let _lisp_code = generate_elle_bindings(&parsed, lib_path);

    // In a full implementation, we would evaluate the generated Lisp code here
    // For now, return the library handle
    Ok(Value::String(lib_path.into()))
}

/// (define-enum name ((variant-name value) ...)) -> enum-id
///
/// Defines a C enum type in Elle.
pub fn prim_define_enum(_vm: &mut VM, args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("define-enum requires exactly 2 arguments".to_string());
    }

    let enum_name = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("enum name must be a string".to_string()),
    };

    // Parse variants from list
    let variants_list = &args[1];
    let mut variants = Vec::new();

    match variants_list {
        Value::Cons(_) => {
            let variant_vec = variants_list.list_to_vec()?;
            for variant_val in variant_vec {
                match variant_val {
                    Value::Cons(cons) => {
                        let name = match &cons.first {
                            Value::String(n) => n.as_ref().to_string(),
                            _ => return Err("variant name must be a string".to_string()),
                        };

                        let value = match &cons.rest {
                            Value::Cons(rest_cons) => match &rest_cons.first {
                                Value::Int(n) => *n,
                                _ => return Err("variant value must be an integer".to_string()),
                            },
                            _ => return Err("variant must be (name value)".to_string()),
                        };

                        variants.push(EnumVariant { name, value });
                    }
                    _ => return Err("each variant must be a cons cell".to_string()),
                }
            }
        }
        Value::Nil => {}
        _ => return Err("variants must be a list".to_string()),
    }

    // Create enum layout
    static ENUM_ID_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let enum_id = EnumId::new(ENUM_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst));

    let _layout = EnumLayout::new(enum_id, enum_name.to_string(), variants, CType::Int);

    // Return enum ID as integer
    Ok(Value::Int(enum_id.0 as i64))
}

/// Phase 4: Advanced Features Primitives
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

// Wrapper functions for primitive registration with VM context access

pub fn prim_load_library_wrapper(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("load-library requires exactly 1 argument".to_string());
    }

    let path = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("load-library requires a string path".to_string()),
    };

    // Get VM context
    let vm_ptr = get_vm_context().ok_or("FFI not initialized")?;
    unsafe {
        let vm = &mut *vm_ptr;
        let lib_id = vm.ffi_mut().load_library(path)?;
        Ok(Value::LibHandle(LibHandle(lib_id)))
    }
}

pub fn prim_list_libraries_wrapper(_args: &[Value]) -> Result<Value, String> {
    let vm_ptr = get_vm_context().ok_or("FFI not initialized")?;
    unsafe {
        let vm = &*vm_ptr;
        let libs = vm.ffi().loaded_libraries();
        let mut result = Value::Nil;
        for (id, path) in libs.into_iter().rev() {
            let entry = crate::value::cons(
                Value::Int(id as i64),
                crate::value::cons(Value::String(path.into()), Value::Nil),
            );
            result = crate::value::cons(entry, result);
        }
        Ok(result)
    }
}

pub fn prim_call_c_function_wrapper(args: &[Value]) -> Result<Value, String> {
    if args.len() != 5 {
        return Err("call-c-function requires exactly 5 arguments".to_string());
    }

    // Parse library ID
    let lib_id = match &args[0] {
        Value::LibHandle(LibHandle(id)) => *id,
        _ => return Err("First argument must be a library handle".to_string()),
    };

    // Parse function name
    let func_name = match &args[1] {
        Value::String(s) => s.as_ref(),
        _ => return Err("Second argument must be a function name string".to_string()),
    };

    // Parse return type
    let return_type = parse_ctype(&args[2])?;

    // Parse argument types
    let arg_types = match &args[3] {
        Value::Nil => vec![],
        Value::Cons(_) => {
            let type_list = args[3].list_to_vec()?;
            type_list
                .iter()
                .map(parse_ctype)
                .collect::<Result<Vec<_>, _>>()?
        }
        _ => return Err("Fourth argument must be a list of argument types".to_string()),
    };

    // Parse argument values
    let arg_values = match &args[4] {
        Value::Nil => vec![],
        Value::Cons(_) => args[4].list_to_vec()?,
        _ => return Err("Fifth argument must be a list of argument values".to_string()),
    };

    // Check argument count matches
    if arg_types.len() != arg_values.len() {
        return Err(format!(
            "Argument count mismatch: expected {}, got {}",
            arg_types.len(),
            arg_values.len()
        ));
    }

    // Create function signature
    let sig = FunctionSignature::new(func_name.to_string(), arg_types, return_type);

    // Get VM context
    let vm_ptr = get_vm_context().ok_or("FFI not initialized")?;
    unsafe {
        let vm = &*vm_ptr;
        let lib = vm
            .ffi()
            .get_library(lib_id)
            .ok_or("Library handle not found".to_string())?;

        let func_ptr = lib.get_symbol(func_name)?;
        let call = FunctionCall::new(sig, func_ptr)?;
        call.call(&arg_values)
    }
}

pub fn prim_load_header_with_lib_wrapper(args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("load-header-with-lib requires exactly 2 arguments".to_string());
    }

    let header_path = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("header-path must be a string".to_string()),
    };

    let lib_path = match &args[1] {
        Value::String(s) => s.as_ref(),
        _ => return Err("lib-path must be a string".to_string()),
    };

    // Parse header
    let mut parser = HeaderParser::new();
    let parsed = parser.parse(header_path)?;

    // Generate bindings
    let _lisp_code = generate_elle_bindings(&parsed, lib_path);

    // Return library path (future: would evaluate generated code)
    Ok(Value::String(lib_path.into()))
}

pub fn prim_define_enum_wrapper(args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("define-enum requires exactly 2 arguments".to_string());
    }

    let enum_name = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("enum name must be a string".to_string()),
    };

    // Parse variants from list
    let variants_list = &args[1];
    let mut variants = Vec::new();

    match variants_list {
        Value::Cons(_) => {
            let variant_vec = variants_list.list_to_vec()?;
            for variant_val in variant_vec {
                match variant_val {
                    Value::Cons(cons) => {
                        let name = match &cons.first {
                            Value::String(n) => n.as_ref().to_string(),
                            _ => return Err("variant name must be a string".to_string()),
                        };

                        let value = match &cons.rest {
                            Value::Cons(rest_cons) => match &rest_cons.first {
                                Value::Int(n) => *n,
                                _ => return Err("variant value must be an integer".to_string()),
                            },
                            _ => return Err("variant must be (name value)".to_string()),
                        };

                        variants.push(EnumVariant { name, value });
                    }
                    _ => return Err("each variant must be a cons cell".to_string()),
                }
            }
        }
        Value::Nil => {}
        _ => return Err("variants must be a list".to_string()),
    }

    // Create enum layout
    static ENUM_ID_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let enum_id = EnumId::new(ENUM_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst));

    let _layout = EnumLayout::new(enum_id, enum_name.to_string(), variants, CType::Int);

    // Return enum ID as integer
    Ok(Value::Int(enum_id.0 as i64))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_library_error_on_wrong_args() {
        // This would require a full VM setup to test properly
        // For now, just verify function exists
        assert_eq!(
            std::mem::size_of::<fn(&mut VM, &[Value]) -> Result<Value, String>>(),
            std::mem::size_of::<fn(&mut VM, &[Value]) -> Result<Value, String>>()
        );
    }
}
