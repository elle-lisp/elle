//! Stable-ABI struct and array indexing plus list→array conversion. Struct/array
//! reads borrow the heap object directly (no `CallCtx`); `list_to_array` allocates
//! a fresh array and so threads the call's `CallCtx` in.

use super::super::*;
use super::{from_value, intern_str, to_value, with_ctx, CallCtx};

// ── Struct access ─────────────────────────────────────────────────────

pub(in crate::plugin_api) extern "C" fn struct_get(
    val: [u64; 2],
    key_ptr: *const u8,
    key_len: usize,
) -> [u64; 2] {
    use crate::value::heap::{deref, HeapObject};
    use crate::value::types::sorted_struct_get;

    let v = unsafe { to_value(val) };
    let key_str =
        unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(key_ptr, key_len)) };
    let key = TableKey::keyword(key_str);

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

pub(in crate::plugin_api) extern "C" fn struct_len(val: [u64; 2]) -> isize {
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

pub(in crate::plugin_api) extern "C" fn struct_key(
    ctx: *mut super::CallCtx,
    val: [u64; 2],
    idx: usize,
    out_len: *mut usize,
) -> *const u8 {
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
                    TableKey::Keyword(hash) => {
                        let symbols = ctx.as_ref().and_then(|c| c.symbols.as_ref());
                        match crate::value::keyword::resolve_keyword_name(symbols, *hash) {
                            Some(name) => intern_str(name.to_string()),
                            None => return std::ptr::null(),
                        }
                    }
                    TableKey::String(s) => intern_str(s.clone()),
                    _ => return std::ptr::null(),
                };
                *out_len = s.len();
                s.as_ptr()
            }
            _ => std::ptr::null(),
        }
    }
}

pub(in crate::plugin_api) extern "C" fn struct_value(val: [u64; 2], idx: usize) -> [u64; 2] {
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

pub(in crate::plugin_api) extern "C" fn array_len(val: [u64; 2]) -> isize {
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

pub(in crate::plugin_api) extern "C" fn array_get(val: [u64; 2], idx: usize) -> [u64; 2] {
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
pub(in crate::plugin_api) extern "C" fn list_to_array(
    ctx: *mut CallCtx,
    val: [u64; 2],
) -> [u64; 2] {
    let v = unsafe { to_value(val) };
    from_value(unsafe {
        with_ctx(ctx, |heap, region| match v.list_to_vec_in(heap) {
            Ok(items) => crate::value::build::array(heap, items, region),
            Err(_) => Value::NIL,
        })
    })
}
