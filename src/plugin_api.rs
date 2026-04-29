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
use crate::value::{error_val, Value};

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::c_void;
use std::sync::{Mutex, RwLock};

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
pub type PluginPrimFn = unsafe extern "C" fn(args: *const Value, nargs: usize) -> PrimResult;

/// Sentinel function used as the `func` field of plugin PrimitiveDefs.
/// Never actually called — the VM checks for this and dispatches through
/// the plugin function table instead.
fn plugin_sentinel(_args: &[Value]) -> (SignalBits, Value) {
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
pub(crate) fn call_plugin(def: &PrimitiveDef, args: &[Value]) -> (SignalBits, Value) {
    let key = def as *const PrimitiveDef as usize;
    let table = PLUGIN_FUNCS.read().unwrap();
    let func = table
        .as_ref()
        .and_then(|m| m.get(&key))
        .expect("plugin function not found — PrimitiveDef has sentinel but no registered fn");
    let result = unsafe { func(args.as_ptr(), args.len()) };
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
        version: 1,
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

#[inline(always)]
unsafe fn to_value(v: [u64; 2]) -> Value {
    std::mem::transmute::<[u64; 2], Value>(v)
}

#[inline(always)]
fn from_value(v: Value) -> [u64; 2] {
    unsafe { std::mem::transmute::<Value, [u64; 2]>(v) }
}

// ── Constructors ──────────────────────────────────────────────────────

extern "C" fn make_int(n: i64) -> [u64; 2] {
    from_value(Value::int(n))
}

extern "C" fn make_float(f: f64) -> [u64; 2] {
    from_value(Value::float(f))
}

extern "C" fn make_bool(b: bool) -> [u64; 2] {
    from_value(Value::bool(b))
}

extern "C" fn make_nil() -> [u64; 2] {
    from_value(Value::NIL)
}

extern "C" fn make_string(ptr: *const u8, len: usize) -> [u64; 2] {
    let s = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) };
    from_value(Value::string(s))
}

extern "C" fn make_bytes(ptr: *const u8, len: usize) -> [u64; 2] {
    let data = unsafe { std::slice::from_raw_parts(ptr, len) };
    from_value(Value::bytes(data.to_vec()))
}

extern "C" fn make_keyword(ptr: *const u8, len: usize) -> [u64; 2] {
    let name = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) };
    from_value(Value::keyword(name))
}

extern "C" fn make_array(elems_ptr: *const [u64; 2], count: usize) -> [u64; 2] {
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
    from_value(Value::array(elems))
}

extern "C" fn make_struct(kvs_ptr: *const ElleKVRaw, count: usize) -> [u64; 2] {
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
    from_value(Value::struct_from(fields))
}

/// Layout-compatible with `ElleKV` in elle-plugin.
#[repr(C)]
struct ElleKVRaw {
    key: *const u8,
    key_len: usize,
    value: [u64; 2],
}

extern "C" fn make_set(elems_ptr: *const [u64; 2], count: usize) -> [u64; 2] {
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
    from_value(Value::set(set))
}

extern "C" fn make_error(
    kind_ptr: *const u8,
    kind_len: usize,
    msg_ptr: *const u8,
    msg_len: usize,
) -> [u64; 2] {
    let kind =
        unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(kind_ptr, kind_len)) };
    let msg =
        unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(msg_ptr, msg_len)) };
    from_value(error_val(kind, msg))
}

// ── External objects ──────────────────────────────────────────────────

/// Wrapper for external objects created through the stable ABI.
struct ExternalWrapper {
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

extern "C" fn make_external(
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
    from_value(Value::external(type_name, wrapper))
}

extern "C" fn as_external(
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

extern "C" fn as_int(val: [u64; 2], out: *mut i64) -> bool {
    let v = unsafe { to_value(val) };
    if let Some(n) = v.as_int() {
        unsafe { *out = n };
        true
    } else {
        false
    }
}

extern "C" fn as_float(val: [u64; 2], out: *mut f64) -> bool {
    let v = unsafe { to_value(val) };
    if let Some(f) = v.as_float() {
        unsafe { *out = f };
        true
    } else {
        false
    }
}

extern "C" fn as_bool(val: [u64; 2]) -> i32 {
    let v = unsafe { to_value(val) };
    if !v.is_bool() {
        -1
    } else if v.is_truthy() {
        1
    } else {
        0
    }
}

extern "C" fn is_nil(val: [u64; 2]) -> bool {
    let v = unsafe { to_value(val) };
    v.is_nil()
}

extern "C" fn is_truthy(val: [u64; 2]) -> bool {
    let v = unsafe { to_value(val) };
    v.is_truthy()
}

extern "C" fn as_string(val: [u64; 2], out_len: *mut usize) -> *const u8 {
    let v = unsafe { to_value(val) };
    if let Some(ptr_and_len) = v.with_string(|s| (s.as_ptr(), s.len())) {
        let (ptr, len) = ptr_and_len;
        unsafe { *out_len = len };
        ptr
    } else {
        std::ptr::null()
    }
}

extern "C" fn as_bytes(val: [u64; 2], out_len: *mut usize) -> *const u8 {
    let v = unsafe { to_value(val) };
    if let Some(b) = v.as_bytes() {
        unsafe { *out_len = b.len() };
        b.as_ptr()
    } else {
        std::ptr::null()
    }
}

extern "C" fn type_name_of(val: [u64; 2], out_len: *mut usize) -> *const u8 {
    let v = unsafe { to_value(val) };
    let name = v.type_name();
    unsafe { *out_len = name.len() };
    name.as_ptr()
}

// ── Type predicates ───────────────────────────────────────────────────

extern "C" fn is_string(val: [u64; 2]) -> bool {
    unsafe { to_value(val) }.is_string() || unsafe { to_value(val) }.is_string_mut()
}

extern "C" fn is_keyword(val: [u64; 2]) -> bool {
    unsafe { to_value(val) }.is_keyword()
}

extern "C" fn is_bytes(val: [u64; 2]) -> bool {
    let v = unsafe { to_value(val) };
    v.is_bytes() || v.is_bytes_mut()
}

extern "C" fn is_array(val: [u64; 2]) -> bool {
    let v = unsafe { to_value(val) };
    v.is_array() || v.is_array_mut()
}

extern "C" fn is_struct(val: [u64; 2]) -> bool {
    let v = unsafe { to_value(val) };
    v.is_struct() || v.is_struct_mut()
}

extern "C" fn is_int(val: [u64; 2]) -> bool {
    unsafe { to_value(val) }.is_int()
}

extern "C" fn is_float(val: [u64; 2]) -> bool {
    unsafe { to_value(val) }.is_float()
}

extern "C" fn is_bool_val(val: [u64; 2]) -> bool {
    unsafe { to_value(val) }.is_bool()
}

extern "C" fn is_external(val: [u64; 2]) -> bool {
    unsafe { to_value(val) }.is_external()
}

// ── String interning for API returns ──────────────────────────────────
//
// Several API functions return string pointers that must outlive the call.
// Instead of Box::leak (which leaks on every call), we intern into a
// HashSet so repeated lookups reuse the same allocation.

static INTERNED: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);

fn intern_str(s: String) -> &'static str {
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

extern "C" fn as_keyword_name(val: [u64; 2], out_len: *mut usize) -> *const u8 {
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

extern "C" fn value_eq(a: [u64; 2], b: [u64; 2]) -> bool {
    let va = unsafe { to_value(a) };
    let vb = unsafe { to_value(b) };
    va == vb
}

// ── Struct access ─────────────────────────────────────────────────────

extern "C" fn struct_get(val: [u64; 2], key_ptr: *const u8, key_len: usize) -> [u64; 2] {
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

extern "C" fn struct_len(val: [u64; 2]) -> isize {
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

extern "C" fn struct_key(val: [u64; 2], idx: usize, out_len: *mut usize) -> *const u8 {
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

extern "C" fn struct_value(val: [u64; 2], idx: usize) -> [u64; 2] {
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

extern "C" fn array_len(val: [u64; 2]) -> isize {
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

extern "C" fn array_get(val: [u64; 2], idx: usize) -> [u64; 2] {
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
extern "C" fn list_to_array(val: [u64; 2]) -> [u64; 2] {
    let v = unsafe { to_value(val) };
    match v.list_to_vec() {
        Ok(items) => from_value(Value::array(items)),
        Err(_) => from_value(Value::NIL),
    }
}

// ── Async ─────────────────────────────────────────────────────────────

extern "C" fn make_poll_fd(fd: i32, events: u32) -> [u64; 2] {
    from_value(IoRequest::poll_fd(fd, events))
}

// ── Keyword interning ─────────────────────────────────────────────────

extern "C" fn intern_keyword(name_ptr: *const u8, name_len: usize) -> u64 {
    let name =
        unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(name_ptr, name_len)) };
    crate::value::keyword::intern_keyword(name)
}

extern "C" fn keyword_name(hash: u64, out_len: *mut usize) -> *const u8 {
    if let Some(name) = crate::value::keyword::keyword_name(hash) {
        let interned = intern_str(name);
        unsafe { *out_len = interned.len() };
        interned.as_ptr()
    } else {
        std::ptr::null()
    }
}

// ── PrimitiveDef construction from plugin-side raw def ────────────────

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
        aliases: &[],
    }));

    register_plugin_fn(def, raw.func);

    def
}
