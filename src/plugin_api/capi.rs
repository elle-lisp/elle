use super::*;

// ── Plugin-boundary allocation capability (docs/impl/region/ctx.md "Plugins") ──
//
// The stable-ABI constructors (`make_string`, …) must allocate into the *call's*
// region on the *call's* heap, yet a C function returns a value with no knowledge
// of either. `call_plugin` builds this `(region, heap)` capability as a `CallCtx`
// and passes it — as an opaque first argument — to the plugin primitive, which
// threads it back into every allocating constructor (`make_string(ctx, …)`). The
// capability is thus a value on the call stack, not ambient per-thread state: it
// cannot be stale, missing, or — should an API function ever dispatch a nested
// plugin — clobbered by a sibling call. The heap pointer is the dispatching
// instance's own heap, so two embedded instances on one thread allocate into
// their own heaps. `CallCtx` is opaque to the plugin (mirrored there as
// `ElleCtx`, never dereferenced); its layout lives entirely on this side.
#[repr(C)]
pub struct CallCtx {
    pub(crate) region: crate::hir::region::RuntimeRegion,
    pub(crate) heap: *mut crate::value::fiberheap::FiberHeap,
}

/// Run `f` with the call's heap and region, taken from the `CallCtx` the plugin
/// passed back. `ctx` is the non-null pointer `call_plugin` handed to the plugin
/// primitive for exactly this call.
#[inline]
unsafe fn with_ctx<R>(
    ctx: *mut CallCtx,
    f: impl FnOnce(&mut crate::value::fiberheap::FiberHeap, crate::hir::region::RuntimeRegion) -> R,
) -> R {
    debug_assert!(
        !ctx.is_null(),
        "plugin ABI constructor called with a null ctx"
    );
    // SAFETY: `call_plugin` builds `CallCtx` from the dispatching ctx's live heap
    // and passes `&mut it` for exactly the synchronous plugin call; the plugin
    // hands that same pointer straight back here. The heap outlives the call.
    let cx = &*ctx;
    f(&mut *cx.heap, cx.region)
}

#[inline(always)]
pub(super) unsafe fn to_value(v: [u64; 2]) -> Value {
    std::mem::transmute::<[u64; 2], Value>(v)
}

#[inline(always)]
pub(super) fn from_value(v: Value) -> [u64; 2] {
    unsafe { std::mem::transmute::<Value, [u64; 2]>(v) }
}

// ── ABI entry points, grouped by concern ──────────────────────────────
//
// The `extern "C"` slots resolved by name in `api_resolve` live in three
// submodules. Re-export them here so the parent's `use capi::*` continues to
// name every function; the transmute helpers and `CallCtx` above stay in the
// root because all three groups (and the async/keyword entry points below)
// thread them in.

mod accessors;
mod collections;
mod constructors;

pub(super) use accessors::{
    as_bool, as_bytes, as_float, as_int, as_keyword_name, as_string, is_array, is_bool_val,
    is_bytes, is_external, is_float, is_int, is_keyword, is_nil, is_string, is_struct, is_truthy,
    type_name_of, value_eq,
};
pub(super) use collections::{
    array_get, array_len, list_to_array, struct_get, struct_key, struct_len, struct_value,
};
pub(super) use constructors::{
    as_external, make_array, make_bool, make_bytes, make_error, make_external, make_float,
    make_int, make_keyword, make_nil, make_set, make_string, make_struct,
};
// `ElleKVRaw` is not an `api_resolve` slot — only the ctx-region tests build one,
// so re-export it (into the shared `super::*` prelude) solely under `cfg(test)`.
#[cfg(test)]
pub(super) use constructors::ElleKVRaw;

// ── String interning for API returns ──────────────────────────────────
//
// Several API functions return string pointers that must outlive the call.
// Instead of Box::leak (which leaks on every call), we intern into a
// HashSet so repeated lookups reuse the same allocation.

static INTERNED: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);

pub(super) fn intern_str(s: String) -> &'static str {
    let mut guard = INTERNED.lock().unwrap();
    let set = guard.get_or_insert_with(HashSet::new);
    if let Some(existing) = set.get(s.as_str()) {
        existing
    } else {
        let leaked: &'static str = Box::leak(s.into_boxed_str());
        set.insert(leaked);
        leaked
    }
}

// ── Async ─────────────────────────────────────────────────────────────

pub(super) extern "C" fn make_poll_fd(ctx: *mut CallCtx, fd: i32, events: u32) -> [u64; 2] {
    from_value(unsafe {
        with_ctx(ctx, |heap, region| {
            let alloc = crate::primitives::ctx::Alloc::with_region(region, heap);
            IoRequest::poll_fd(&alloc, fd, events)
        })
    })
}

// ── Keyword interning ─────────────────────────────────────────────────

pub(super) extern "C" fn intern_keyword(name_ptr: *const u8, name_len: usize) -> u64 {
    let name =
        unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(name_ptr, name_len)) };
    crate::value::keyword::intern_keyword(name)
}

pub(super) extern "C" fn keyword_name(hash: u64, out_len: *mut usize) -> *const u8 {
    if let Some(name) = crate::value::keyword::keyword_name(hash) {
        let interned = intern_str(name);
        unsafe { *out_len = interned.len() };
        interned.as_ptr()
    } else {
        std::ptr::null()
    }
}

// ── PrimitiveDef construction from plugin-side raw def ────────────────

#[cfg(test)]
mod tests;
