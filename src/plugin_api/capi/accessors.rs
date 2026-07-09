//! Stable-ABI value accessors, type predicates, keyword access, and equality.
//! These read from a `Value` (never allocate), so they take no `CallCtx`; the
//! shared `to_value` transmute lives in the module root.

use super::{intern_str, to_value};

// ── Accessors ─────────────────────────────────────────────────────────

pub(in crate::plugin_api) extern "C" fn as_int(val: [u64; 2], out: *mut i64) -> bool {
    let v = unsafe { to_value(val) };
    if let Some(n) = v.as_int() {
        unsafe { *out = n };
        true
    } else {
        false
    }
}

pub(in crate::plugin_api) extern "C" fn as_float(val: [u64; 2], out: *mut f64) -> bool {
    let v = unsafe { to_value(val) };
    if let Some(f) = v.as_float() {
        unsafe { *out = f };
        true
    } else {
        false
    }
}

pub(in crate::plugin_api) extern "C" fn as_bool(val: [u64; 2]) -> i32 {
    let v = unsafe { to_value(val) };
    if !v.is_bool() {
        -1
    } else if v.is_truthy() {
        1
    } else {
        0
    }
}

pub(in crate::plugin_api) extern "C" fn is_nil(val: [u64; 2]) -> bool {
    let v = unsafe { to_value(val) };
    v.is_nil()
}

pub(in crate::plugin_api) extern "C" fn is_truthy(val: [u64; 2]) -> bool {
    let v = unsafe { to_value(val) };
    v.is_truthy()
}

pub(in crate::plugin_api) extern "C" fn as_string(val: [u64; 2], out_len: *mut usize) -> *const u8 {
    let v = unsafe { to_value(val) };
    if let Some(ptr_and_len) = v.with_string(|s| (s.as_ptr(), s.len())) {
        let (ptr, len) = ptr_and_len;
        unsafe { *out_len = len };
        ptr
    } else {
        std::ptr::null()
    }
}

pub(in crate::plugin_api) extern "C" fn as_bytes(val: [u64; 2], out_len: *mut usize) -> *const u8 {
    let v = unsafe { to_value(val) };
    if let Some(b) = v.as_bytes() {
        unsafe { *out_len = b.len() };
        b.as_ptr()
    } else {
        std::ptr::null()
    }
}

pub(in crate::plugin_api) extern "C" fn type_name_of(
    val: [u64; 2],
    out_len: *mut usize,
) -> *const u8 {
    let v = unsafe { to_value(val) };
    let name = v.type_name();
    unsafe { *out_len = name.len() };
    name.as_ptr()
}

// ── Type predicates ───────────────────────────────────────────────────

pub(in crate::plugin_api) extern "C" fn is_string(val: [u64; 2]) -> bool {
    unsafe { to_value(val) }.is_string() || unsafe { to_value(val) }.is_string_mut()
}

pub(in crate::plugin_api) extern "C" fn is_keyword(val: [u64; 2]) -> bool {
    unsafe { to_value(val) }.is_keyword()
}

pub(in crate::plugin_api) extern "C" fn is_bytes(val: [u64; 2]) -> bool {
    let v = unsafe { to_value(val) };
    v.is_bytes() || v.is_bytes_mut()
}

pub(in crate::plugin_api) extern "C" fn is_array(val: [u64; 2]) -> bool {
    let v = unsafe { to_value(val) };
    v.is_array() || v.is_array_mut()
}

pub(in crate::plugin_api) extern "C" fn is_struct(val: [u64; 2]) -> bool {
    let v = unsafe { to_value(val) };
    v.is_struct() || v.is_struct_mut()
}

pub(in crate::plugin_api) extern "C" fn is_int(val: [u64; 2]) -> bool {
    unsafe { to_value(val) }.is_int()
}

pub(in crate::plugin_api) extern "C" fn is_float(val: [u64; 2]) -> bool {
    unsafe { to_value(val) }.is_float()
}

pub(in crate::plugin_api) extern "C" fn is_bool_val(val: [u64; 2]) -> bool {
    unsafe { to_value(val) }.is_bool()
}

pub(in crate::plugin_api) extern "C" fn is_external(val: [u64; 2]) -> bool {
    unsafe { to_value(val) }.is_external()
}

// ── Keyword access ────────────────────────────────────────────────────

pub(in crate::plugin_api) extern "C" fn as_keyword_name(
    val: [u64; 2],
    out_len: *mut usize,
) -> *const u8 {
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

pub(in crate::plugin_api) extern "C" fn value_eq(a: [u64; 2], b: [u64; 2]) -> bool {
    let va = unsafe { to_value(a) };
    let vb = unsafe { to_value(b) };
    va == vb
}
