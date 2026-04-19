//! FFI library loading, symbol lookup, signature creation, and callback primitives

use crate::ffi::types::{CallingConvention, Signature};
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

use super::ffi::resolve_type_desc;

pub(crate) fn prim_ffi_native(args: &[Value]) -> (SignalBits, Value) {
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

    let path = if let Some(s) = args[0].with_string(|s| s.to_string()) {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "ffi/native: expected string or nil, got {}",
                    args[0].type_name()
                ),
            ),
        );
    };
    match vm.ffi_mut().load_library(&path) {
        Ok(id) => (SIG_OK, Value::lib_handle(id)),
        Err(e) => (
            SIG_ERROR,
            error_val("ffi-error", format!("ffi/native: {}", e)),
        ),
    }
}

pub(crate) fn prim_ffi_lookup(args: &[Value]) -> (SignalBits, Value) {
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
    let sym_name = if let Some(s) = args[1].with_string(|s| s.to_string()) {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("ffi/lookup: expected string, got {}", args[1].type_name()),
            ),
        );
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
    match lib.get_symbol(&sym_name) {
        Ok(ptr) => (SIG_OK, Value::pointer(ptr as usize)),
        Err(e) => (
            SIG_ERROR,
            error_val("ffi-error", format!("ffi/lookup: {}", e)),
        ),
    }
}

pub(crate) fn prim_ffi_signature(args: &[Value]) -> (SignalBits, Value) {
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
    let arg_vals = if let Some(arr) = args[1].as_array_mut() {
        arr.borrow().clone()
    } else if let Some(arr) = args[1].as_array() {
        arr.to_vec()
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

#[cfg(feature = "ffi")]
pub(crate) fn prim_ffi_callback(args: &[Value]) -> (SignalBits, Value) {
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
        Some(c) => std::rc::Rc::new(c.clone()),
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
    let arity_ok = match closure_rc.template.arity {
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
                    expected_args, closure_rc.template.arity
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

#[cfg(feature = "ffi")]
pub(crate) fn prim_ffi_callback_free(args: &[Value]) -> (SignalBits, Value) {
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

/// Declarative primitive definitions for FFI loading operations.
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "ffi/native",
        func: prim_ffi_native,
        signal: Signal::ffi_errors(),
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
        signal: Signal::ffi_errors(),
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
        signal: Signal::errors(),
        arity: Arity::Range(2, 3),
        doc: "Create a reified function signature. Optional third arg for variadic functions.",
        params: &["return-type", "arg-types", "fixed-args"],
        category: "ffi",
        example: "(ffi/signature :int [:ptr :size :ptr :int] 3)",
        aliases: &[],
    },
];

/// Callback primitives (require libffi).
#[cfg(feature = "ffi")]
pub(crate) const CALLBACK_PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "ffi/callback",
        func: prim_ffi_callback,
        signal: Signal::ffi_errors(),
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
        signal: Signal::ffi_errors(),
        arity: Arity::Exact(1),
        doc: "Free a callback created by ffi/callback.",
        params: &["ptr"],
        category: "ffi",
        example: "(ffi/callback-free cb-ptr)",
        aliases: &[],
    },
];
