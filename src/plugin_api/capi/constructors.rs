//! Stable-ABI value constructors: the `make_*` family plus external-object
//! wrapping. Each allocates into the call's region via the `CallCtx` capability
//! threaded in as the leading argument (see `with_ctx` in the module root).

use super::super::*;
use super::{from_value, to_value, with_ctx, CallCtx};

// ── Constructors ──────────────────────────────────────────────────────

pub(in crate::plugin_api) extern "C" fn make_int(n: i64) -> [u64; 2] {
    from_value(Value::int(n))
}

pub(in crate::plugin_api) extern "C" fn make_float(f: f64) -> [u64; 2] {
    from_value(Value::float(f))
}

pub(in crate::plugin_api) extern "C" fn make_bool(b: bool) -> [u64; 2] {
    from_value(Value::bool(b))
}

pub(in crate::plugin_api) extern "C" fn make_nil() -> [u64; 2] {
    from_value(Value::NIL)
}

pub(in crate::plugin_api) extern "C" fn make_string(
    ctx: *mut CallCtx,
    ptr: *const u8,
    len: usize,
) -> [u64; 2] {
    let s = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) };
    from_value(unsafe {
        with_ctx(ctx, |heap, region| {
            crate::value::build::string(heap, s, region)
        })
    })
}

pub(in crate::plugin_api) extern "C" fn make_bytes(
    ctx: *mut CallCtx,
    ptr: *const u8,
    len: usize,
) -> [u64; 2] {
    let data = unsafe { std::slice::from_raw_parts(ptr, len) };
    from_value(unsafe {
        with_ctx(ctx, |heap, region| {
            crate::value::build::bytes(heap, data.to_vec(), region)
        })
    })
}

pub(in crate::plugin_api) extern "C" fn make_keyword(ptr: *const u8, len: usize) -> [u64; 2] {
    let name = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) };
    from_value(Value::keyword(name))
}

pub(in crate::plugin_api) extern "C" fn make_array(
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

pub(in crate::plugin_api) extern "C" fn make_struct(
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
///
/// Fields are `pub(in crate::plugin_api)` — not private — because the ctx-region
/// tests construct one directly. Previously the struct sat in the `capi` root, so
/// its private fields were reachable from the sibling `tests` module; moving it
/// down into `constructors` requires the wider visibility to keep that access.
#[repr(C)]
pub(in crate::plugin_api) struct ElleKVRaw {
    pub(in crate::plugin_api) key: *const u8,
    pub(in crate::plugin_api) key_len: usize,
    pub(in crate::plugin_api) value: [u64; 2],
}

pub(in crate::plugin_api) extern "C" fn make_set(
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

pub(in crate::plugin_api) extern "C" fn make_error(
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
pub(in crate::plugin_api) struct ExternalWrapper {
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

pub(in crate::plugin_api) extern "C" fn make_external(
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

pub(in crate::plugin_api) extern "C" fn as_external(
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
