//! Library loading and management primitives.

use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, Value};
use crate::vm::VM;

/// (load-library path) -> library-handle
pub fn prim_load_library(vm: &mut VM, args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("load-library requires exactly 1 argument".into());
    }

    let path = args[0]
        .as_string()
        .ok_or("load-library requires a string path")?;

    let lib_id = vm.ffi_mut().load_library(path)?;
    Ok(Value::int(lib_id as i64))
}

/// (list-libraries) -> ((id path) ...)
pub fn prim_list_libraries(vm: &VM, _args: &[Value]) -> Result<Value, String> {
    let libs = vm.ffi().loaded_libraries();

    let mut result = Value::NIL;
    for (id, path) in libs.into_iter().rev() {
        let entry = crate::value::cons(
            Value::int(id as i64),
            crate::value::cons(Value::string(path), Value::EMPTY_LIST),
        );
        result = crate::value::cons(entry, result);
    }

    Ok(result)
}

pub fn prim_load_library_wrapper(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val("arity-error", "load-library: expected 1 argument"),
        );
    }

    let path = match args[0].as_string() {
        Some(p) => p,
        None => {
            return (
                SIG_ERROR,
                error_val("type-error", "load-library: expected string path"),
            );
        }
    };

    // Get VM context
    let vm_ptr = match super::context::get_vm_context() {
        Some(ptr) => ptr,
        None => {
            return (SIG_ERROR, error_val("error", "FFI not initialized"));
        }
    };

    unsafe {
        let vm = &mut *vm_ptr;
        match vm.ffi_mut().load_library(path) {
            Ok(lib_id) => (SIG_OK, Value::int(lib_id as i64)),
            Err(e) => (SIG_ERROR, error_val("error", e)),
        }
    }
}

pub fn prim_list_libraries_wrapper(_args: &[Value]) -> (SignalBits, Value) {
    let vm_ptr = match super::context::get_vm_context() {
        Some(ptr) => ptr,
        None => {
            return (SIG_ERROR, error_val("error", "FFI not initialized"));
        }
    };

    unsafe {
        let vm = &*vm_ptr;
        let libs = vm.ffi().loaded_libraries();
        let mut result = Value::NIL;
        for (id, path) in libs.into_iter().rev() {
            let entry = crate::value::cons(
                Value::int(id as i64),
                crate::value::cons(Value::string(path), Value::EMPTY_LIST),
            );
            result = crate::value::cons(entry, result);
        }
        (SIG_OK, result)
    }
}
