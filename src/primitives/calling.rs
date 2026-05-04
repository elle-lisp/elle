//! FFI function call primitives

use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

pub(crate) fn prim_ffi_call(args: &[Value]) -> (SignalBits, Value) {
    if args[0].is_nil() {
        return (
            SIG_ERROR,
            error_val("type-error", "ffi/call: function pointer is nil"),
        );
    }
    let fn_addr = match args[0].as_pointer() {
        Some(addr) => addr,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("ffi/call: expected pointer, got {}", args[0].type_name()),
                ),
            )
        }
    };

    let sig = match args[1].as_ffi_signature() {
        Some(s) => s.clone(),
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("ffi/call: expected signature, got {}", args[1].type_name()),
                ),
            )
        }
    };

    let call_args = &args[2..];

    // Get or prepare cached CIF
    let cif_ref = match args[1].get_or_prepare_cif() {
        Some(cif) => cif,
        None => {
            return (
                SIG_ERROR,
                error_val("ffi-error", "ffi/call: failed to get CIF from signature"),
            )
        }
    };

    let result = match unsafe {
        crate::ffi::call::ffi_call(
            fn_addr as *const std::ffi::c_void,
            call_args,
            &sig,
            &cif_ref,
        )
    } {
        Ok(val) => (SIG_OK, val),
        Err(e) => (
            SIG_ERROR,
            error_val("ffi-error", format!("ffi/call: {}", e)),
        ),
    };

    // Check for errors from FFI callbacks that ran during this call.
    // If a callback errored, it wrote a zero return value to C and
    // stored the error here. Propagate it to the Elle caller.
    if let Some(cb_err) = crate::ffi::callback::take_callback_error() {
        return (SIG_ERROR, cb_err);
    }

    result
}

/// Declarative primitive definitions for FFI call operations.
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[PrimitiveDef {
    name: "ffi/call",
    func: prim_ffi_call,
    signal: Signal::ffi_errors(),
    arity: Arity::AtLeast(2),
    doc: "Call a C function through libffi.",
    params: &["fn-ptr", "sig"],
    category: "ffi",
    example: "(ffi/call sqrt-ptr sig 2.0)",
    aliases: &[],
}];
