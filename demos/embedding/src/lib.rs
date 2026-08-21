#![allow(improper_ctypes_definitions)]
//! C-ABI embedding surface for Elle.
//!
//! Provides opaque `ElleCtx` wrapping VM + SymbolTable. Host programs link
//! against libelle_embed.so and drive the lifecycle through exported functions.

use elle::plugin_api::{PluginPrimFn, PrimResult, PLUGIN_SENTINEL};
use elle::primitives::def::PrimitiveDef;
use elle::runtime::Runtime;
use elle::signals::Signal;
use elle::value::types::Arity;
use elle::{compile_file, Value};

use std::ffi::c_void;

// ── Opaque context ──────────────────────────────────────────────────

/// A `Runtime` owns the heap, VM, symbol table, and compile context as one
/// per-instance bundle, points the VM at its own symbol table and `CompileCtx`,
/// and runs the RC teardown sweep on drop. The compile context is threaded
/// explicitly into every compile/execute call (see `elle_eval`); nothing is read
/// from a shared slot, so two `ElleCtx`s coexist in one process.
struct ElleCtx {
    runtime: Runtime,
    last_result: Option<Value>,
}

// ── Lifecycle ───────────────────────────────────────────────────────

/// Create an Elle runtime context. Returns an opaque pointer.
#[no_mangle]
pub extern "C" fn elle_init() -> *mut c_void {
    // `Runtime::new()` registers primitives, loads the stdlib, and points the VM
    // at this instance's own symbol table and compile context — the whole
    // embedding lifecycle in one call.
    let ctx = Box::new(ElleCtx {
        runtime: Runtime::new(),
        last_result: None,
    });
    Box::into_raw(ctx) as *mut c_void
}

/// Destroy an Elle runtime context.
///
/// # Safety
/// `ctx` must be a pointer returned by `elle_init`, or null.
#[no_mangle]
pub unsafe extern "C" fn elle_destroy(ctx: *mut c_void) {
    if ctx.is_null() {
        return;
    }
    // Dropping the `Runtime` runs the RC teardown sweep; the VM's symbol-table
    // and compile-context pointers drop with the instance, so no manual teardown
    // is needed.
    unsafe {
        drop(Box::from_raw(ctx as *mut ElleCtx));
    }
}

// ── Eval ────────────────────────────────────────────────────────────

/// Compile and execute Elle source code. Returns 0 on success, -1 on error.
///
/// # Safety
/// `ctx` must be a valid `elle_init` pointer. `src` must point to `len`
/// bytes of valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn elle_eval(ctx: *mut c_void, src: *const u8, len: usize) -> i32 {
    if ctx.is_null() || src.is_null() {
        return -1;
    }
    let ctx = unsafe { &mut *(ctx as *mut ElleCtx) };
    let source = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(src, len)) };

    // The pipeline needs the three disjoint borrows at once: the VM (execution),
    // the symbol table (interning/resolution), and the compile context (macro
    // expansion + meta). Compile against this instance's context, then run the
    // bytecode under the async scheduler.
    let (vm, symbols, cctx) = ctx.runtime.parts();
    match compile_file(source, symbols, cctx, "<embed>") {
        Ok(compiled) => match vm.execute_scheduled(&compiled.bytecode, symbols, cctx) {
            Ok(value) => {
                ctx.last_result = Some(value);
                0
            }
            Err(_) => -1,
        },
        Err(_) => -1,
    }
}

// ── Result access ───────────────────────────────────────────────────

/// Get the result as an integer. Returns false if not an int.
///
/// # Safety
/// `ctx` must be a valid `elle_init` pointer. `out` must be non-null.
#[no_mangle]
pub unsafe extern "C" fn elle_result_int(ctx: *mut c_void, out: *mut i64) -> bool {
    if ctx.is_null() {
        return false;
    }
    let ctx = unsafe { &*(ctx as *mut ElleCtx) };
    match ctx.last_result {
        Some(v) => match v.as_int() {
            Some(n) => {
                unsafe { *out = n };
                true
            }
            None => false,
        },
        None => false,
    }
}

// ── Custom primitive registration ───────────────────────────────────

/// Register a host primitive. The func pointer uses the same ABI as plugins:
/// `unsafe extern "C" fn(ctx: *mut CallCtx, args: *const Value, nargs: usize) ->
/// PrimResult`. The leading `ctx` is the opaque per-call allocation capability
/// (region + heap); a host primitive that constructs heap values must thread it
/// into the elle constructors, exactly as a `.so` plugin does.
///
/// # Safety
/// `name` must point to `name_len` bytes of valid UTF-8 that outlive the
/// context. `func` must be a valid function pointer.
#[no_mangle]
pub unsafe extern "C" fn elle_register_prim(
    ctx: *mut c_void,
    name: *const u8,
    name_len: usize,
    func: PluginPrimFn,
    arity: u16,
) {
    if ctx.is_null() || name.is_null() {
        return;
    }
    let ctx = unsafe { &mut *(ctx as *mut ElleCtx) };
    let name_str =
        unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(name, name_len)) };
    let static_name: &'static str = unsafe { std::mem::transmute::<&str, &'static str>(name_str) };

    let def = Box::leak(Box::new(PrimitiveDef {
        name: static_name,
        func: PLUGIN_SENTINEL,
        signal: Signal::silent(),
        arity: Arity::Exact(arity as usize),
        doc: "",
        params: &[],
        category: "host",
        ..PrimitiveDef::DEFAULT
    }));

    elle::plugin_api::register_plugin_fn(def, func);

    let sym_id = ctx.runtime.symbols().intern(static_name);
    let native = Value::native_fn(def);
    // The binding's region is rooted through this instance's own heap; the
    // compile context and heap are taken as disjoint borrows of the runtime.
    let (cctx, heap) = ctx.runtime.compile_and_heap();
    cctx.register_repl_binding(
        heap,
        sym_id,
        native,
        Signal::silent(),
        Some(Arity::Exact(arity as usize)),
    );
}

// ── Value constructors (re-exports for C hosts) ─────────────────────

#[no_mangle]
pub extern "C" fn elle_make_int(n: i64) -> [u64; 2] {
    unsafe { std::mem::transmute::<Value, [u64; 2]>(Value::int(n)) }
}

#[no_mangle]
pub extern "C" fn elle_make_nil() -> [u64; 2] {
    unsafe { std::mem::transmute::<Value, [u64; 2]>(Value::NIL) }
}

// Re-export PrimResult for C header consumers
#[no_mangle]
pub extern "C" fn elle_prim_result(signal: u32, value: [u64; 2]) -> PrimResult {
    PrimResult {
        signal,
        value: unsafe { std::mem::transmute::<[u64; 2], Value>(value) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use elle::plugin_api::CallCtx;

    // End-to-end pin for the v3 plugin ABI: a host primitive is dispatched through
    // the *real* path (VM sentinel → `call_plugin` → the registered fn) and must
    // receive the opaque per-call `ctx` as arg0 and the argument vector after it.
    //
    // This is the coverage the constructor-seam test cannot give: it exercises the
    // C-ABI argument *order* agreed between `call_plugin` (host) and the primitive
    // (plugin). `first` returns `args[0]`; if `call_plugin` failed to pass `ctx`
    // first (the pre-fix calling convention), `args` would land on `nargs` and the
    // deref would read garbage rather than 42 — so the arg threading is pinned.
    unsafe extern "C" fn first(_ctx: *mut CallCtx, args: *const Value, nargs: usize) -> PrimResult {
        assert!(nargs >= 1, "first/2 dispatched with too few args");
        PrimResult {
            signal: 0, // SIG_OK
            value: unsafe { *args },
        }
    }

    #[test]
    fn v3_abi_threads_ctx_then_args_end_to_end() {
        let ctx = elle_init();
        assert!(!ctx.is_null());

        let name = "first";
        unsafe { elle_register_prim(ctx, name.as_ptr(), name.len(), first, 2) };

        let src = "(first 42 99)";
        let rc = unsafe { elle_eval(ctx, src.as_ptr(), src.len()) };
        assert_eq!(rc, 0, "eval of a call to the host primitive must succeed");

        let mut out = 0i64;
        assert!(
            unsafe { elle_result_int(ctx, &mut out) },
            "result must be an int",
        );
        assert_eq!(
            out, 42,
            "the host primitive must see its arguments *after* the ctx and return args[0]",
        );

        unsafe { elle_destroy(ctx) };
    }
}
