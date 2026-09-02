//! FFI library loading, symbol lookup, signature creation, call, and callback primitives

use crate::ffi::types::{CallingConvention, Signature};
use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

use super::ffi::resolve_type_desc;

// ── FFI call (requires libffi) ───────────────────────────────────────

#[cfg(feature = "ffi")]
pub(crate) fn prim_ffi_call(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args[0].is_nil() {
        return (
            SIG_ERROR,
            ctx.error("type-error", "ffi/call: function pointer is nil"),
        );
    }
    let fn_addr = prim_arg!(ctx, args, 0, as_pointer, "ffi/call", "pointer");

    let sig = prim_arg!(ctx, args, 1, as_ffi_signature, "ffi/call", "signature").clone();

    let call_args = &args[2..];

    // Get or prepare cached CIF
    let cif_ref = match args[1].get_or_prepare_cif() {
        Some(cif) => cif,
        None => {
            return (
                SIG_ERROR,
                ctx.error("ffi-error", "ffi/call: failed to get CIF from signature"),
            )
        }
    };

    let result = match unsafe {
        crate::ffi::call::ffi_call(
            fn_addr as *const std::ffi::c_void,
            call_args,
            &sig,
            &cif_ref,
            ctx,
        )
    } {
        Ok(val) => (SIG_OK, val),
        Err(e) => (
            SIG_ERROR,
            ctx.error("ffi-error", format!("ffi/call: {}", e)),
        ),
    };

    // Check for errors from FFI callbacks that ran during this call.
    // If a callback errored, it wrote a zero return value to C and
    // stored the error on the VM's FFI subsystem. Propagate it to the Elle caller.
    if let Some(cb_err) = ctx.vm().ffi_mut().take_callback_error() {
        return (SIG_ERROR, cb_err);
    }

    result
}

// ── FFI loading ─────────────────────────────────────────────────────

pub(crate) fn prim_ffi_native(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let vm = ctx.vm();

    // nil → load self process (dlopen(NULL))
    if args[0].is_nil() {
        return match vm.ffi_mut().load_self() {
            Ok(id) => (SIG_OK, ctx.lib_handle(id)),
            Err(e) => (
                SIG_ERROR,
                ctx.error("ffi-error", format!("ffi/native: {}", e)),
            ),
        };
    }

    let path = if let Some(s) = args[0].with_string(|s| s.to_string()) {
        s
    } else {
        return type_error!(ctx, args[0], "ffi/native", "string or nil");
    };
    match vm.ffi_mut().load_library(&path) {
        Ok(id) => (SIG_OK, ctx.lib_handle(id)),
        Err(e) => (
            SIG_ERROR,
            ctx.error("ffi-error", format!("ffi/native: {}", e)),
        ),
    }
}

pub(crate) fn prim_ffi_lookup(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let lib_id = prim_arg!(ctx, args, 0, as_lib_handle, "ffi/lookup", "library handle");
    let sym_name = if let Some(s) = args[1].with_string(|s| s.to_string()) {
        s
    } else {
        return type_error!(ctx, args[1], "ffi/lookup", "string");
    };
    let vm = ctx.vm();
    match vm.ffi().get_symbol(lib_id, &sym_name) {
        Ok(ptr) => (SIG_OK, Value::pointer(ptr as usize)),
        Err(e) => (
            SIG_ERROR,
            ctx.error("ffi-error", format!("ffi/lookup: {}", e)),
        ),
    }
}

/// Register an ordered teardown for a loaded library — a zero-arg C symbol to call
/// at an explicit `ffi/run-teardowns`. The library mapping is never unloaded (it is
/// process-global), so this is optional graceful cleanup, never required to avoid a
/// crash and never run automatically by the runtime.
pub(crate) fn prim_ffi_on_unload(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let lib_id = prim_arg!(
        ctx,
        args,
        0,
        as_lib_handle,
        "ffi/on-unload",
        "library handle"
    );
    let sym_name = if let Some(s) = args[1].with_string(|s| s.to_string()) {
        s
    } else {
        return type_error!(ctx, args[1], "ffi/on-unload", "string");
    };
    match ctx.vm().ffi().register_teardown(lib_id, &sym_name) {
        Ok(()) => (SIG_OK, Value::NIL),
        Err(e) => (
            SIG_ERROR,
            ctx.error("ffi-error", format!("ffi/on-unload: {}", e)),
        ),
    }
}

/// Run every registered FFI library teardown (`ffi/on-unload`), in reverse load
/// order. Explicit-only — the program calls this when it knows teardown is safe
/// (e.g. after `sys/join`ing workers); it never unloads the libraries.
pub(crate) fn prim_ffi_run_teardowns(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    crate::ffi::registry::run_teardowns();
    (SIG_OK, Value::NIL)
}

pub(crate) fn prim_ffi_signature(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let ret = match resolve_type_desc(&args[0], "ffi/signature", ctx) {
        Ok(t) => t,
        Err(e) => return e,
    };

    // Parse argument types from array or list
    let arg_vals = if let Some(arr) = args[1].as_array_mut() {
        arr.borrow().clone()
    } else if let Some(arr) = args[1].as_array() {
        arr.to_vec()
    } else {
        match args[1].list_to_vec_in(ctx.heap_mut()) {
            Ok(v) => v,
            Err(_) => {
                return type_error!(ctx, args[1], "ffi/signature", "array or list for arg types")
            }
        }
    };

    let mut arg_types = Vec::with_capacity(arg_vals.len());
    for val in &arg_vals {
        match resolve_type_desc(val, "ffi/signature", ctx) {
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
                    ctx.error(
                        "argument-error",
                        format!(
                            "ffi/signature: fixed_args {} out of range [0, {}]",
                            n,
                            arg_types.len()
                        ),
                    ),
                )
            }
            None => return type_error!(ctx, args[2], "ffi/signature", "integer for fixed_args"),
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
    (SIG_OK, ctx.ffi_signature(sig))
}

#[cfg(feature = "ffi")]
pub(crate) fn prim_ffi_callback(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let sig = prim_arg!(ctx, args, 0, as_ffi_signature, "ffi/callback", "signature").clone();
    let closure_rc = match args[1].as_closure() {
        Some(c) => std::rc::Rc::new(c.clone()),
        None => return type_error!(ctx, args[1], "ffi/callback", "closure"),
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
            ctx.error(
                "arity-error",
                format!(
                    "ffi/callback: signature has {} args but closure has arity {}",
                    expected_args, closure_rc.template.arity
                ),
            ),
        );
    }

    // Capture the driving VM in the callback so the C-invoked trampoline reaches
    // it without a shared context (the callback is single-VM by its limitation).
    // The closure VALUE rides along so each invocation installs it as the body's
    // executing-closure register (see `CallbackData::closure_value`).
    let callback = match crate::ffi::callback::create_callback(closure_rc, args[1], sig, ctx.vm()) {
        Ok(cb) => cb,
        Err(e) => {
            return (
                SIG_ERROR,
                ctx.error("ffi-error", format!("ffi/callback: {}", e)),
            )
        }
    };

    // Store the callback in the driving VM's FFI subsystem so it stays alive.
    let code_ptr = ctx.vm().ffi_mut().callbacks_mut().insert(callback);

    (SIG_OK, Value::pointer(code_ptr))
}

#[cfg(feature = "ffi")]
pub(crate) fn prim_ffi_callback_free(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args[0].is_nil() {
        return (SIG_OK, Value::NIL); // free(nil) is a no-op
    }
    let addr = prim_arg!(ctx, args, 0, as_pointer, "ffi/callback-free", "pointer");

    if ctx.vm().ffi_mut().callbacks_mut().remove(addr) {
        (SIG_OK, Value::NIL)
    } else {
        (
            SIG_ERROR,
            ctx.error(
                "ffi-error",
                format!("ffi/callback-free: no callback at address {:#x}", addr),
            ),
        )
    }
}

// Declarative primitive definitions for FFI loading operations.
primitive! {
    "ffi/native" => prim_ffi_native {
        signal: Signal::ffi_errors(),
        arity: Arity::Exact(1),
        doc: "Load a shared library by Linux-style name (resolved to the host's .dylib/.dll). Pass nil for the current process.",
        params: &["path"],
        category: "ffi",
        example: "(ffi/native \"libm.so.6\")",
        effect: RegionEffect::Fresh,
    }
    "ffi/lookup" => prim_ffi_lookup {
        signal: Signal::ffi_errors(),
        arity: Arity::Exact(2),
        doc: "Look up a symbol in a loaded library.",
        params: &["lib", "name"],
        category: "ffi",
        example: "(ffi/lookup lib \"strlen\")",
        effect: RegionEffect::Immediate,
    }
    "ffi/on-unload" => prim_ffi_on_unload {
        signal: Signal::ffi_errors(),
        arity: Arity::Exact(2),
        doc: "Register a teardown C symbol (zero-arg) to run for a library at an \
              explicit (ffi/run-teardowns). The library mapping is never unloaded, \
              so this is optional graceful cleanup — never required to avoid a crash, \
              never run automatically.",
        params: &["lib", "symbol"],
        category: "ffi",
        example: "(ffi/on-unload git-lib \"git_libgit2_shutdown\")",
        effect: RegionEffect::Immediate,
    }
    "ffi/run-teardowns" => prim_ffi_run_teardowns {
        signal: Signal::errors(),
        arity: Arity::Exact(0),
        doc: "Run all registered FFI library teardowns (ffi/on-unload), in reverse \
              load order. Explicit-only; never unloads the libraries. Call only when \
              workers that used the libraries have quiesced.",
        params: &[],
        category: "ffi",
        example: "(ffi/run-teardowns)",
        effect: RegionEffect::Immediate,
    }
    "ffi/signature" => prim_ffi_signature {
        signal: Signal::errors(),
        arity: Arity::Range(2, 3),
        doc: "Create a reified function signature. Optional third arg for variadic functions.",
        params: &["return-type", "arg-types", "fixed-args"],
        category: "ffi",
        example: "(ffi/signature :int [:ptr :size :ptr :int] 3)",
        effect: RegionEffect::Fresh,
    }
}

#[cfg(feature = "ffi")]
primitive!(
    /// FFI call and callback primitives (require libffi).
    pub(crate) static CALLBACK_PRIMITIVES =
        "ffi/call" => prim_ffi_call {
            signal: Signal::ffi_errors(),
            arity: Arity::AtLeast(2),
            doc: "Call a C function through libffi.",
            params: &["fn-ptr", "sig"],
            category: "ffi",
            example: "(ffi/call sqrt-ptr sig 2.0)",
            // Fresh, not Mixed: every arg is marshalled BY COPY (CString,
            // AlignedBuffer — src/ffi/to_c.rs), `:ptr` accepts only
            // user-managed pointer values (never an Elle heap payload), and
            // the result is converted from C memory into the call's own
            // region. No Elle reference survives the call, so the arg clique
            // would be pure over-keep — it leaked one region per heap arg per
            // call once call-result-arg clique increfs became real
            // (region-ffi-callback-arg-uaf.lisp's bounded assert is the pin).
            effect: RegionEffect::Fresh,
        }
        "ffi/callback" => prim_ffi_callback {
            signal: Signal::ffi_errors(),
            arity: Arity::Exact(2),
            doc: "Create a C function pointer from an Elle closure. Returns a pointer.",
            params: &["sig", "closure"],
            category: "ffi",
            example: "(ffi/callback (ffi/signature :int [:ptr :ptr]) (fn (a b) 0))",
            effect: RegionEffect::Stores { args: &[1] },
        }
        "ffi/callback-free" => prim_ffi_callback_free {
            signal: Signal::ffi_errors(),
            arity: Arity::Exact(1),
            doc: "Free a callback created by ffi/callback.",
            params: &["ptr"],
            category: "ffi",
            example: "(ffi/callback-free cb-ptr)",
            effect: RegionEffect::Immediate,
        }
);
