//! Stable plugin ABI implementation.
//!
//! This module provides:
//!
//! 1. **Named API functions** — `extern "C"` implementations of every slot
//!    in the `elle_api!` table. Registered by name, resolved by plugins at
//!    init time.
//!
//! 2. **Plugin dispatch table** — a mapping from `PrimitiveDef` address to
//!    the plugin's `extern "C"` function pointer. The VM checks for the
//!    sentinel before calling, and dispatches through this table.

use crate::io::request::IoRequest;
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::SignalBits;
use crate::value::types::{Arity, PrimFn, TableKey};
use crate::value::Value;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::c_void;
use std::sync::{Mutex, RwLock};

mod capi;
use capi::*;
// `CallCtx` is the opaque per-call capability named in the public `PluginPrimFn`
// signature; re-export it so `elle::plugin_api::CallCtx` is a nameable (but
// unforgeable — its fields are crate-private) type for embedders.
pub use capi::CallCtx;

// ── Compile-time ABI assertions ───────────────────────────────────────

// Value must be exactly 16 bytes (two u64) for transmute safety.
const _: () = assert!(std::mem::size_of::<Value>() == 16);
const _: () = assert!(std::mem::align_of::<Value>() == 8);

// PrimResult must match ElleResult layout across the ABI boundary.
const _: () = assert!(std::mem::size_of::<PrimResult>() == 24);
const _: () = assert!(std::mem::align_of::<PrimResult>() == 8);

// ── Plugin dispatch table ─────────────────────────────────────────────

/// Raw plugin primitive result, layout-compatible with `ElleResult` in
/// elle-plugin: `{ signal: u32, [4 pad], value: [u64; 2] }`.
#[repr(C)]
pub struct PrimResult {
    pub signal: u32,
    pub value: Value,
}

/// Plugin primitive function pointer (C ABI).
///
/// The leading `*mut CallCtx` is the per-call allocation capability (the call's
/// region + the dispatching instance's heap) that `call_plugin` builds and hands
/// in; the plugin threads it, unchanged, into every allocating constructor
/// (`make_string`, …). It replaces the former `PLUGIN_CALL_ALLOC` thread-local —
/// the capability is an explicit argument, not ambient per-thread state. Opaque
/// to the plugin, where it is mirrored as `ElleCtx` and never dereferenced.
pub type PluginPrimFn =
    unsafe extern "C" fn(ctx: *mut CallCtx, args: *const Value, nargs: usize) -> PrimResult;

/// Sentinel function used as the `func` field of plugin PrimitiveDefs.
/// Never actually called — the VM checks for this and dispatches through
/// the plugin function table instead.
fn plugin_sentinel(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    panic!("plugin primitive called without plugin dispatch — this is a bug")
}

/// The sentinel as a PrimFn value, for comparison in the VM.
pub const PLUGIN_SENTINEL: PrimFn = plugin_sentinel;

/// Address-keyed table of plugin function pointers.
/// Key = `&'static PrimitiveDef` pointer cast to usize.
static PLUGIN_FUNCS: RwLock<Option<HashMap<usize, PluginPrimFn>>> = RwLock::new(None);

/// Register a plugin function pointer for a PrimitiveDef.
pub fn register_plugin_fn(def: &'static PrimitiveDef, func: PluginPrimFn) {
    let mut table = PLUGIN_FUNCS.write().unwrap();
    let map = table.get_or_insert_with(HashMap::new);
    map.insert(def as *const PrimitiveDef as usize, func);
}

/// Call a plugin primitive by PrimitiveDef address lookup.
///
/// `region` is the call's own region — the same region `ctx` owns, threaded in
/// from `dispatch_native_call` (which minted it) so the stable-ABI constructors
/// land plugin allocations exactly where the ctx's would, WITHOUT a region
/// getter on `NativeCtx` (docs/impl/region/ctx.md "Plugins").
pub(crate) fn call_plugin(
    def: &PrimitiveDef,
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
    region: crate::hir::region::RuntimeRegion,
) -> (SignalBits, Value) {
    let key = def as *const PrimitiveDef as usize;
    let table = PLUGIN_FUNCS.read().unwrap();
    let func = table
        .as_ref()
        .and_then(|m| m.get(&key))
        .expect("plugin function not found — PrimitiveDef has sentinel but no registered fn");
    // Build this call's `(region, heap)` capability and pass it to the plugin as
    // an opaque first argument, so the stable-ABI constructors (`make_string`, …)
    // allocate into the call's own region on the dispatching instance's own heap
    // (docs/impl/region/ctx.md "Plugins"). The capability lives on this stack
    // frame for exactly the synchronous plugin call — no ambient slot to install
    // or clear, and no way for a (future) nested plugin call to clobber it.
    let symbols = ctx.vm().symbols_ptr;
    let mut call_ctx = CallCtx {
        region,
        heap: ctx.heap_mut(),
        symbols,
    };
    let result = unsafe { func(&mut call_ctx, args.as_ptr(), args.len()) };
    (SignalBits::new(result.signal as u64), result.value)
}

// ── API loader construction ───────────────────────────────────────────

/// Resolve an API function by name. This is the function that plugins call
/// at init time to look up each API function pointer by name.
extern "C" fn api_resolve(name_ptr: *const u8, name_len: usize) -> *const c_void {
    let name =
        unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(name_ptr, name_len)) };

    macro_rules! resolve_match {
        ($($fn_name:ident),*) => {
            match name {
                $(stringify!($fn_name) => $fn_name as *const c_void,)*
                _ => std::ptr::null(),
            }
        };
    }

    resolve_match!(
        make_int,
        make_float,
        make_bool,
        make_nil,
        make_string,
        make_bytes,
        make_keyword,
        make_array,
        make_struct,
        make_set,
        make_error,
        make_external,
        as_external,
        as_int,
        as_float,
        as_bool,
        is_nil,
        is_truthy,
        as_string,
        as_bytes,
        type_name_of,
        is_string,
        is_keyword,
        is_bytes,
        is_array,
        is_struct,
        is_int,
        is_float,
        is_bool_val,
        is_external,
        as_keyword_name,
        struct_get,
        struct_len,
        struct_key,
        struct_value,
        array_len,
        array_get,
        list_to_array,
        value_eq,
        make_poll_fd,
        intern_keyword,
        keyword_name
    )
}

/// Construct the `ElleApiLoader` for plugin initialization.
pub(crate) fn build_api_loader() -> ApiLoader {
    ApiLoader {
        // ABI version 3: plugin primitives receive an opaque per-call ctx (region
        // + heap) as their leading argument and thread it into the allocating
        // constructors (docs/impl/region/ctx.md "Plugins"). This changed the
        // primitive calling convention, so a v2 plugin (no ctx arg) is incompatible
        // and must be recompiled; the SDK's version guard turns the mismatch into a
        // clean load failure rather than a corrupt call.
        version: 3,
        resolve: api_resolve,
    }
}

/// Layout-compatible with `ElleApiLoader` in elle-plugin.
#[repr(C)]
pub(crate) struct ApiLoader {
    pub version: u32,
    pub resolve: extern "C" fn(name: *const u8, len: usize) -> *const c_void,
}

// ── Value transmute helpers ───────────────────────────────────────────
//
// Value is #[repr(C)] with `{ tag: u64, payload: u64 }`, matching [u64; 2].
// The compile-time assertions above verify size and alignment.

/// Raw C-ABI representation of a plugin's primitive definition.
/// Layout-compatible with `EllePrimDef` in elle-plugin.
#[repr(C)]
pub(crate) struct PrimDefRaw {
    pub name: *const u8,
    pub name_len: usize,
    pub func: PluginPrimFn,
    pub signal: u32,
    pub arity_kind: u8,
    pub arity_min: u16,
    pub arity_max: u16,
    pub doc: *const u8,
    pub doc_len: usize,
    pub category: *const u8,
    pub category_len: usize,
    pub example: *const u8,
    pub example_len: usize,
}

/// Convert a raw plugin `EllePrimDef` into a leaked `&'static PrimitiveDef`.
///
/// The PrimitiveDef has `func = plugin_sentinel` — the VM checks this
/// before calling and dispatches through the plugin table instead.
///
/// # Safety
/// The raw def must point to valid string data that lives for the process
/// lifetime (i.e., from a plugin's .so rodata section).
pub(crate) unsafe fn raw_def_to_primitive(raw: &PrimDefRaw) -> &'static PrimitiveDef {
    let name: &'static str = std::mem::transmute::<&str, &'static str>(
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(raw.name, raw.name_len)),
    );

    macro_rules! static_str {
        ($ptr:expr, $len:expr) => {
            if $ptr.is_null() || $len == 0 {
                ""
            } else {
                std::mem::transmute::<&str, &'static str>(std::str::from_utf8_unchecked(
                    std::slice::from_raw_parts($ptr, $len),
                ))
            }
        };
    }

    let doc: &'static str = static_str!(raw.doc, raw.doc_len);
    let category: &'static str = static_str!(raw.category, raw.category_len);
    let example: &'static str = static_str!(raw.example, raw.example_len);

    let arity = match raw.arity_kind {
        0 => Arity::Exact(raw.arity_min as usize),
        1 => Arity::AtLeast(raw.arity_min as usize),
        2 => Arity::Range(raw.arity_min as usize, raw.arity_max as usize),
        _ => Arity::AtLeast(0),
    };

    let signal = Signal {
        bits: SignalBits::new(raw.signal as u64),
        propagates: 0,
    };

    let def = Box::leak(Box::new(PrimitiveDef {
        name,
        func: plugin_sentinel,
        signal,
        arity,
        doc,
        params: &[],
        category,
        example,
        ..PrimitiveDef::DEFAULT
    }));

    register_plugin_fn(def, raw.func);

    def
}
