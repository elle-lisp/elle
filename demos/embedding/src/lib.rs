#![allow(improper_ctypes_definitions)]
//! C-ABI embedding surface for Elle.
//!
//! Provides opaque `ElleCtx` wrapping VM + SymbolTable. Host programs link
//! against libelle_embed.so and drive the lifecycle through exported functions.

use elle::context::{set_symbol_table, set_vm_context};
use elle::pipeline::register_repl_binding;
use elle::plugin_api::{PluginPrimFn, PrimResult, PLUGIN_SENTINEL};
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::types::Arity;
use elle::{compile_file, init_stdlib, register_primitives, SymbolTable, Value, VM};

use std::ffi::c_void;

// ── Opaque context ──────────────────────────────────────────────────

struct ElleCtx {
    vm: VM,
    symbols: SymbolTable,
    last_result: Option<Value>,
}

// ── Lifecycle ───────────────────────────────────────────────────────

/// Create an Elle runtime context. Returns an opaque pointer.
#[no_mangle]
pub extern "C" fn elle_init() -> *mut c_void {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _signals = register_primitives(&mut vm, &mut symbols);
    set_vm_context(&mut vm as *mut VM);
    set_symbol_table(&mut symbols as *mut SymbolTable);
    init_stdlib(&mut vm, &mut symbols);

    let ctx = Box::new(ElleCtx {
        vm,
        symbols,
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
    unsafe {
        drop(Box::from_raw(ctx as *mut ElleCtx));
    }
    set_vm_context(std::ptr::null_mut());
    set_symbol_table(std::ptr::null_mut());
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

    set_vm_context(&mut ctx.vm as *mut VM);
    set_symbol_table(&mut ctx.symbols as *mut SymbolTable);

    match compile_file(source, &mut ctx.symbols, "<embed>") {
        Ok(compiled) => match ctx.vm.execute_scheduled(&compiled.bytecode, &ctx.symbols) {
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
/// `unsafe extern "C" fn(args: *const Value, nargs: usize) -> PrimResult`.
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
        example: "",
        aliases: &[],
    }));

    elle::plugin_api::register_plugin_fn(def, func);

    let sym_id = ctx.symbols.intern(static_name);
    let native = Value::native_fn(def);
    register_repl_binding(
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
