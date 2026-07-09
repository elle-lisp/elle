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

// ── Constructors ──────────────────────────────────────────────────────

pub(super) extern "C" fn make_int(n: i64) -> [u64; 2] {
    from_value(Value::int(n))
}

pub(super) extern "C" fn make_float(f: f64) -> [u64; 2] {
    from_value(Value::float(f))
}

pub(super) extern "C" fn make_bool(b: bool) -> [u64; 2] {
    from_value(Value::bool(b))
}

pub(super) extern "C" fn make_nil() -> [u64; 2] {
    from_value(Value::NIL)
}

pub(super) extern "C" fn make_string(ctx: *mut CallCtx, ptr: *const u8, len: usize) -> [u64; 2] {
    let s = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) };
    from_value(unsafe {
        with_ctx(ctx, |heap, region| {
            crate::value::build::string(heap, s, region)
        })
    })
}

pub(super) extern "C" fn make_bytes(ctx: *mut CallCtx, ptr: *const u8, len: usize) -> [u64; 2] {
    let data = unsafe { std::slice::from_raw_parts(ptr, len) };
    from_value(unsafe {
        with_ctx(ctx, |heap, region| {
            crate::value::build::bytes(heap, data.to_vec(), region)
        })
    })
}

pub(super) extern "C" fn make_keyword(ptr: *const u8, len: usize) -> [u64; 2] {
    let name = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) };
    from_value(Value::keyword(name))
}

pub(super) extern "C" fn make_array(
    ctx: *mut CallCtx,
    elems_ptr: *const [u64; 2],
    count: usize,
) -> [u64; 2] {
    let elems: Vec<Value> = if count == 0 {
        Vec::new()
    } else {
        unsafe {
            std::slice::from_raw_parts(elems_ptr, count)
                .iter()
                .map(|bits| to_value(*bits))
                .collect()
        }
    };
    from_value(unsafe {
        with_ctx(ctx, |heap, region| {
            crate::value::build::array(heap, elems, region)
        })
    })
}

pub(super) extern "C" fn make_struct(
    ctx: *mut CallCtx,
    kvs_ptr: *const ElleKVRaw,
    count: usize,
) -> [u64; 2] {
    let mut fields = BTreeMap::new();
    if count > 0 {
        let kvs = unsafe { std::slice::from_raw_parts(kvs_ptr, count) };
        for kv in kvs {
            let key_str = unsafe {
                std::str::from_utf8_unchecked(std::slice::from_raw_parts(kv.key, kv.key_len))
            };
            let value = unsafe { to_value(kv.value) };
            fields.insert(TableKey::Keyword(key_str.into()), value);
        }
    }
    from_value(unsafe {
        with_ctx(ctx, |heap, region| {
            crate::value::build::struct_from(heap, fields, region)
        })
    })
}

/// Layout-compatible with `ElleKV` in elle-plugin.
#[repr(C)]
pub(super) struct ElleKVRaw {
    key: *const u8,
    key_len: usize,
    value: [u64; 2],
}

pub(super) extern "C" fn make_set(
    ctx: *mut CallCtx,
    elems_ptr: *const [u64; 2],
    count: usize,
) -> [u64; 2] {
    let elems: Vec<Value> = if count == 0 {
        Vec::new()
    } else {
        unsafe {
            std::slice::from_raw_parts(elems_ptr, count)
                .iter()
                .map(|bits| to_value(*bits))
                .collect()
        }
    };
    use std::collections::BTreeSet;
    let set: BTreeSet<Value> = elems.into_iter().collect();
    from_value(unsafe {
        with_ctx(ctx, |heap, region| {
            crate::value::build::set(heap, set, region)
        })
    })
}

pub(super) extern "C" fn make_error(
    ctx: *mut CallCtx,
    kind_ptr: *const u8,
    kind_len: usize,
    msg_ptr: *const u8,
    msg_len: usize,
) -> [u64; 2] {
    let kind =
        unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(kind_ptr, kind_len)) };
    let msg =
        unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(msg_ptr, msg_len)) };
    from_value(unsafe {
        with_ctx(ctx, |heap, region| {
            crate::value::error_val_in(heap, kind, msg, region)
        })
    })
}

// ── External objects ──────────────────────────────────────────────────

/// Wrapper for external objects created through the stable ABI.
pub(super) struct ExternalWrapper {
    data: *mut c_void,
    drop_fn: Option<extern "C" fn(*mut c_void)>,
}

impl Drop for ExternalWrapper {
    fn drop(&mut self) {
        if let Some(f) = self.drop_fn {
            f(self.data);
        }
    }
}

pub(super) extern "C" fn make_external(
    ctx: *mut CallCtx,
    type_name_ptr: *const u8,
    type_name_len: usize,
    data: *mut c_void,
    drop_fn: Option<extern "C" fn(*mut c_void)>,
) -> [u64; 2] {
    let type_name_str = unsafe {
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(type_name_ptr, type_name_len))
    };
    // The type_name comes from the plugin's .so rodata — valid for process lifetime.
    let type_name: &'static str =
        unsafe { std::mem::transmute::<&str, &'static str>(type_name_str) };
    let wrapper = ExternalWrapper { data, drop_fn };
    from_value(unsafe {
        with_ctx(ctx, |heap, region| {
            crate::value::build::external(heap, type_name, wrapper, region)
        })
    })
}

pub(super) extern "C" fn as_external(
    val: [u64; 2],
    type_name_ptr: *const u8,
    type_name_len: usize,
) -> *mut c_void {
    let v = unsafe { to_value(val) };
    let expected = unsafe {
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(type_name_ptr, type_name_len))
    };
    if let Some(wrapper) = v.as_external::<ExternalWrapper>() {
        if v.external_type_name() == Some(expected) {
            return wrapper.data;
        }
    }
    std::ptr::null_mut()
}

// ── Accessors ─────────────────────────────────────────────────────────

pub(super) extern "C" fn as_int(val: [u64; 2], out: *mut i64) -> bool {
    let v = unsafe { to_value(val) };
    if let Some(n) = v.as_int() {
        unsafe { *out = n };
        true
    } else {
        false
    }
}

pub(super) extern "C" fn as_float(val: [u64; 2], out: *mut f64) -> bool {
    let v = unsafe { to_value(val) };
    if let Some(f) = v.as_float() {
        unsafe { *out = f };
        true
    } else {
        false
    }
}

pub(super) extern "C" fn as_bool(val: [u64; 2]) -> i32 {
    let v = unsafe { to_value(val) };
    if !v.is_bool() {
        -1
    } else if v.is_truthy() {
        1
    } else {
        0
    }
}

pub(super) extern "C" fn is_nil(val: [u64; 2]) -> bool {
    let v = unsafe { to_value(val) };
    v.is_nil()
}

pub(super) extern "C" fn is_truthy(val: [u64; 2]) -> bool {
    let v = unsafe { to_value(val) };
    v.is_truthy()
}

pub(super) extern "C" fn as_string(val: [u64; 2], out_len: *mut usize) -> *const u8 {
    let v = unsafe { to_value(val) };
    if let Some(ptr_and_len) = v.with_string(|s| (s.as_ptr(), s.len())) {
        let (ptr, len) = ptr_and_len;
        unsafe { *out_len = len };
        ptr
    } else {
        std::ptr::null()
    }
}

pub(super) extern "C" fn as_bytes(val: [u64; 2], out_len: *mut usize) -> *const u8 {
    let v = unsafe { to_value(val) };
    if let Some(b) = v.as_bytes() {
        unsafe { *out_len = b.len() };
        b.as_ptr()
    } else {
        std::ptr::null()
    }
}

pub(super) extern "C" fn type_name_of(val: [u64; 2], out_len: *mut usize) -> *const u8 {
    let v = unsafe { to_value(val) };
    let name = v.type_name();
    unsafe { *out_len = name.len() };
    name.as_ptr()
}

// ── Type predicates ───────────────────────────────────────────────────

pub(super) extern "C" fn is_string(val: [u64; 2]) -> bool {
    unsafe { to_value(val) }.is_string() || unsafe { to_value(val) }.is_string_mut()
}

pub(super) extern "C" fn is_keyword(val: [u64; 2]) -> bool {
    unsafe { to_value(val) }.is_keyword()
}

pub(super) extern "C" fn is_bytes(val: [u64; 2]) -> bool {
    let v = unsafe { to_value(val) };
    v.is_bytes() || v.is_bytes_mut()
}

pub(super) extern "C" fn is_array(val: [u64; 2]) -> bool {
    let v = unsafe { to_value(val) };
    v.is_array() || v.is_array_mut()
}

pub(super) extern "C" fn is_struct(val: [u64; 2]) -> bool {
    let v = unsafe { to_value(val) };
    v.is_struct() || v.is_struct_mut()
}

pub(super) extern "C" fn is_int(val: [u64; 2]) -> bool {
    unsafe { to_value(val) }.is_int()
}

pub(super) extern "C" fn is_float(val: [u64; 2]) -> bool {
    unsafe { to_value(val) }.is_float()
}

pub(super) extern "C" fn is_bool_val(val: [u64; 2]) -> bool {
    unsafe { to_value(val) }.is_bool()
}

pub(super) extern "C" fn is_external(val: [u64; 2]) -> bool {
    unsafe { to_value(val) }.is_external()
}

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

// ── Keyword access ────────────────────────────────────────────────────

pub(super) extern "C" fn as_keyword_name(val: [u64; 2], out_len: *mut usize) -> *const u8 {
    let v = unsafe { to_value(val) };
    if let Some(name) = v.as_keyword_name() {
        let interned = intern_str(name);
        unsafe { *out_len = interned.len() };
        interned.as_ptr()
    } else {
        std::ptr::null()
    }
}

// ── Equality ──────────────────────────────────────────────────────────

pub(super) extern "C" fn value_eq(a: [u64; 2], b: [u64; 2]) -> bool {
    let va = unsafe { to_value(a) };
    let vb = unsafe { to_value(b) };
    va == vb
}

// ── Struct access ─────────────────────────────────────────────────────

pub(super) extern "C" fn struct_get(val: [u64; 2], key_ptr: *const u8, key_len: usize) -> [u64; 2] {
    use crate::value::heap::{deref, HeapObject};
    use crate::value::types::sorted_struct_get;

    let v = unsafe { to_value(val) };
    let key_str =
        unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(key_ptr, key_len)) };
    let key = TableKey::Keyword(key_str.into());

    if !v.is_struct() {
        return from_value(Value::NIL);
    }

    let result = unsafe {
        match deref(v) {
            HeapObject::LStruct { data, .. } => sorted_struct_get(data, &key).copied(),
            _ => None,
        }
    };
    from_value(result.unwrap_or(Value::NIL))
}

pub(super) extern "C" fn struct_len(val: [u64; 2]) -> isize {
    use crate::value::heap::{deref, HeapObject};
    let v = unsafe { to_value(val) };
    if !v.is_struct() {
        return -1;
    }
    unsafe {
        match deref(v) {
            HeapObject::LStruct { data, .. } => data.len() as isize,
            _ => -1,
        }
    }
}

pub(super) extern "C" fn struct_key(val: [u64; 2], idx: usize, out_len: *mut usize) -> *const u8 {
    use crate::value::heap::{deref, HeapObject};
    let v = unsafe { to_value(val) };
    if !v.is_struct() {
        return std::ptr::null();
    }
    unsafe {
        match deref(v) {
            HeapObject::LStruct { data, .. } => {
                if idx >= data.len() {
                    return std::ptr::null();
                }
                let key = &data[idx].0;
                let s = match key {
                    TableKey::Keyword(s) | TableKey::String(s) => intern_str(s.clone()),
                    _ => return std::ptr::null(),
                };
                *out_len = s.len();
                s.as_ptr()
            }
            _ => std::ptr::null(),
        }
    }
}

pub(super) extern "C" fn struct_value(val: [u64; 2], idx: usize) -> [u64; 2] {
    use crate::value::heap::{deref, HeapObject};
    let v = unsafe { to_value(val) };
    if !v.is_struct() {
        return from_value(Value::NIL);
    }
    unsafe {
        match deref(v) {
            HeapObject::LStruct { data, .. } => {
                if idx < data.len() {
                    from_value(data[idx].1)
                } else {
                    from_value(Value::NIL)
                }
            }
            _ => from_value(Value::NIL),
        }
    }
}

// ── Array access ──────────────────────────────────────────────────────

pub(super) extern "C" fn array_len(val: [u64; 2]) -> isize {
    use crate::value::heap::{deref, HeapObject};

    let v = unsafe { to_value(val) };
    if !v.is_array() {
        return -1;
    }
    unsafe {
        match deref(v) {
            HeapObject::LArray { elements, .. } => elements.len() as isize,
            _ => -1,
        }
    }
}

pub(super) extern "C" fn array_get(val: [u64; 2], idx: usize) -> [u64; 2] {
    use crate::value::heap::{deref, HeapObject};

    let v = unsafe { to_value(val) };
    if !v.is_array() {
        return from_value(Value::NIL);
    }
    unsafe {
        match deref(v) {
            HeapObject::LArray { elements, .. } => {
                if idx < elements.len() {
                    from_value(elements[idx])
                } else {
                    from_value(Value::NIL)
                }
            }
            _ => from_value(Value::NIL),
        }
    }
}

// ── List → array conversion ───────────────────────────────────────────

/// Convert a proper list (pair chain) to an immutable array.
/// Returns nil if the value is not a proper list.
pub(super) extern "C" fn list_to_array(ctx: *mut CallCtx, val: [u64; 2]) -> [u64; 2] {
    let v = unsafe { to_value(val) };
    from_value(unsafe {
        with_ctx(ctx, |heap, region| match v.list_to_vec_in(heap) {
            Ok(items) => crate::value::build::array(heap, items, region),
            Err(_) => Value::NIL,
        })
    })
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
