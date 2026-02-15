//! Library loading and management primitives.

use crate::error::{LError, LResult};
use crate::value::{LibHandle, Value};
use crate::vm::VM;

/// (load-library path) -> library-handle
pub fn prim_load_library(vm: &mut VM, args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("load-library requires exactly 1 argument".into());
    }

    let path = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("load-library requires a string path".into()),
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

pub fn prim_load_library_wrapper(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err("load-library requires exactly 1 argument"
            .to_string()
            .into());
    }

    let path = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("load-library requires a string path".into()),
    };

    // Get VM context
    let vm_ptr = super::context::get_vm_context().ok_or("FFI not initialized".to_string())?;
    unsafe {
        let vm = &mut *vm_ptr;
        let lib_id = vm.ffi_mut().load_library(path).map_err(LError::from)?;
        Ok(Value::LibHandle(LibHandle(lib_id)))
    }
}

pub fn prim_list_libraries_wrapper(_args: &[Value]) -> LResult<Value> {
    let vm_ptr = super::context::get_vm_context().ok_or("FFI not initialized".to_string())?;
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
