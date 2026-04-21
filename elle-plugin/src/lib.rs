//! Stable plugin ABI for Elle.
//!
//! This crate defines the types and macros needed to write Elle plugins
//! that are ABI-stable. Plugins depend on this crate (not on `elle`),
//! so they can be compiled independently and loaded at runtime.
//!
//! The ABI surface is a single `ElleApiLoader` with a resolve function —
//! same pattern as `vkGetInstanceProcAddr`. Plugins look up what they
//! need by name at init time. Adding API functions to elle never breaks
//! existing plugins.

use std::ffi::c_void;

// ── Signal constants ──────────────────────────────────────────────────

pub const SIG_OK: u32 = 0;
pub const SIG_ERROR: u32 = 1;
pub const SIG_YIELD: u32 = 1 << 1;
pub const SIG_DEBUG: u32 = 1 << 2;
pub const SIG_FFI: u32 = 1 << 4;
pub const SIG_HALT: u32 = 1 << 8;
pub const SIG_IO: u32 = 1 << 9;
pub const SIG_TERMINAL: u32 = 1 << 10;
pub const SIG_EXEC: u32 = 1 << 11;
pub const SIG_FUEL: u32 = 1 << 12;

// ── Core types ────────────────────────────────────────────────────────

/// Opaque value — same size and layout as elle's internal Value.
/// Accessed only through API functions, never inspected directly.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct ElleValue {
    _bits: [u64; 2],
}

/// Result from a primitive: signal bits + value.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct ElleResult {
    pub signal: u32,
    pub value: ElleValue,
}

/// Key-value pair for struct construction.
#[repr(C)]
pub struct ElleKV {
    pub key: *const u8,
    pub key_len: usize,
    pub value: ElleValue,
}

/// The entire stable ABI contract — just this struct, forever.
///
/// Elle binary registers named C functions in a table. Plugins resolve
/// them by name at init time. Adding functions to elle never breaks
/// existing plugins.
#[repr(C)]
pub struct ElleApiLoader {
    pub version: u32,
    pub resolve: extern "C" fn(name: *const u8, len: usize) -> *const c_void,
}

/// Primitive function signature for the stable ABI.
///
/// Unlike internal primitives (which receive `&[Value]`), stable-ABI
/// primitives receive a raw pointer + length. The plugin accesses the
/// API via the global `api()` accessor provided by `define_plugin!`.
pub type EllePrimFn = extern "C" fn(args: *const ElleValue, nargs: usize) -> ElleResult;

/// Primitive metadata (all C-compatible).
///
/// Contains raw pointers to static string data (rodata). These are
/// safe to share across threads because the pointed-to data is immutable
/// and lives for the entire process.
#[repr(C)]
pub struct EllePrimDef {
    pub name: *const u8,
    pub name_len: usize,
    pub func: EllePrimFn,
    pub signal: u32,
    /// 0 = exact, 1 = at_least, 2 = range.
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

// SAFETY: EllePrimDef contains raw pointers to static string data and
// function pointers. Both are immutable and valid for the process lifetime.
unsafe impl Sync for EllePrimDef {}
unsafe impl Send for EllePrimDef {}

/// Plugin registration context passed to `elle_plugin_init`.
///
/// The plugin calls `register` to declare each primitive. After init
/// returns, elle converts collected definitions into internal form.
#[repr(C)]
pub struct EllePluginCtx {
    pub register: extern "C" fn(ctx: *mut EllePluginCtx, def: *const EllePrimDef),
    _opaque: *mut c_void,
}

/// Plugin entry point signature (v2 protocol).
pub type PluginInitFn = extern "C" fn(loader: &ElleApiLoader, ctx: &mut EllePluginCtx) -> i32;

// ── API function table ────────────────────────────────────────────────

/// Declare the set of API functions that elle exposes.
///
/// This macro generates:
/// - `Api` struct with one function pointer field per declared function
/// - `Api::load()` that resolves all function pointers from the loader
macro_rules! elle_api {
    ($(fn $name:ident($($arg:ty),*) -> $ret:ty;)*) => {
        /// Resolved function pointer cache. Built once at plugin init.
        ///
        /// Each field is a function pointer resolved from the loader.
        /// After construction, calls are direct — no indirection.
        pub struct Api {
            $(pub $name: extern "C" fn($($arg),*) -> $ret,)*
        }

        impl Api {
            /// Resolve all functions from the loader.
            ///
            /// Returns Err with the name of any function that couldn't
            /// be resolved (e.g., plugin compiled against a newer API
            /// than the elle binary provides).
            pub fn load(loader: &ElleApiLoader) -> Result<Self, &'static str> {
                Ok(Api {
                    $($name: {
                        let n = stringify!($name);
                        let ptr = (loader.resolve)(n.as_ptr(), n.len());
                        if ptr.is_null() {
                            return Err(stringify!($name));
                        }
                        unsafe { std::mem::transmute::<*const std::ffi::c_void, extern "C" fn($($arg),*) -> $ret>(ptr) }
                    },)*
                })
            }
        }
    };
}

elle_api! {
    // ── Constructors ──────────────────────────────────────────────
    fn make_int(i64) -> ElleValue;
    fn make_float(f64) -> ElleValue;
    fn make_bool(bool) -> ElleValue;
    fn make_nil() -> ElleValue;
    fn make_string(*const u8, usize) -> ElleValue;
    fn make_bytes(*const u8, usize) -> ElleValue;
    fn make_keyword(*const u8, usize) -> ElleValue;
    fn make_array(*const ElleValue, usize) -> ElleValue;
    fn make_struct(*const ElleKV, usize) -> ElleValue;
    fn make_set(*const ElleValue, usize) -> ElleValue;
    fn make_error(*const u8, usize, *const u8, usize) -> ElleValue;

    // ── External objects ──────────────────────────────────────────
    fn make_external(
        *const u8, usize,
        *mut c_void,
        Option<extern "C" fn(*mut c_void)>
    ) -> ElleValue;
    fn as_external(ElleValue, *const u8, usize) -> *mut c_void;

    // ── Accessors ─────────────────────────────────────────────────
    fn as_int(ElleValue, *mut i64) -> bool;
    fn as_float(ElleValue, *mut f64) -> bool;
    fn as_bool(ElleValue) -> i32;
    fn is_nil(ElleValue) -> bool;
    fn is_truthy(ElleValue) -> bool;
    fn as_string(ElleValue, *mut usize) -> *const u8;
    fn as_bytes(ElleValue, *mut usize) -> *const u8;
    fn type_name_of(ElleValue, *mut usize) -> *const u8;

    // ── Type predicates ───────────────────────────────────────────
    fn is_string(ElleValue) -> bool;
    fn is_keyword(ElleValue) -> bool;
    fn is_bytes(ElleValue) -> bool;
    fn is_array(ElleValue) -> bool;
    fn is_struct(ElleValue) -> bool;
    fn is_int(ElleValue) -> bool;
    fn is_float(ElleValue) -> bool;
    fn is_bool_val(ElleValue) -> bool;
    fn is_external(ElleValue) -> bool;

    // ── Keyword access ────────────────────────────────────────────
    fn as_keyword_name(ElleValue, *mut usize) -> *const u8;

    // ── Struct access ─────────────────────────────────────────────
    fn struct_get(ElleValue, *const u8, usize) -> ElleValue;
    fn struct_len(ElleValue) -> isize;
    fn struct_key(ElleValue, usize, *mut usize) -> *const u8;
    fn struct_value(ElleValue, usize) -> ElleValue;

    // ── Array access ──────────────────────────────────────────────
    fn array_len(ElleValue) -> isize;
    fn array_get(ElleValue, usize) -> ElleValue;

    // ── List → array ──────────────────────────────────────────────
    fn list_to_array(ElleValue) -> ElleValue;

    // ── Equality ──────────────────────────────────────────────────
    fn value_eq(ElleValue, ElleValue) -> bool;

    // ── Async ─────────────────────────────────────────────────────
    fn make_poll_fd(i32, u32) -> ElleValue;

    // ── Keyword interning ─────────────────────────────────────────
    fn intern_keyword(*const u8, usize) -> u64;
    fn keyword_name(u64, *mut usize) -> *const u8;
}

// ── Safe wrappers ─────────────────────────────────────────────────────

impl Api {
    pub fn int(&self, n: i64) -> ElleValue {
        (self.make_int)(n)
    }

    pub fn float(&self, f: f64) -> ElleValue {
        (self.make_float)(f)
    }

    pub fn boolean(&self, b: bool) -> ElleValue {
        (self.make_bool)(b)
    }

    pub fn nil(&self) -> ElleValue {
        (self.make_nil)()
    }

    pub fn string(&self, s: &str) -> ElleValue {
        (self.make_string)(s.as_ptr(), s.len())
    }

    pub fn bytes(&self, b: &[u8]) -> ElleValue {
        (self.make_bytes)(b.as_ptr(), b.len())
    }

    pub fn keyword(&self, s: &str) -> ElleValue {
        (self.make_keyword)(s.as_ptr(), s.len())
    }

    pub fn array(&self, elems: &[ElleValue]) -> ElleValue {
        (self.make_array)(elems.as_ptr(), elems.len())
    }

    pub fn set(&self, elems: &[ElleValue]) -> ElleValue {
        (self.make_set)(elems.as_ptr(), elems.len())
    }

    pub fn build_struct(&self, fields: &[(&str, ElleValue)]) -> ElleValue {
        let kvs: Vec<ElleKV> = fields
            .iter()
            .map(|(k, v)| ElleKV {
                key: k.as_ptr(),
                key_len: k.len(),
                value: *v,
            })
            .collect();
        (self.make_struct)(kvs.as_ptr(), kvs.len())
    }

    pub fn error(&self, kind: &str, msg: &str) -> ElleValue {
        (self.make_error)(kind.as_ptr(), kind.len(), msg.as_ptr(), msg.len())
    }

    pub fn poll_fd(&self, fd: i32, events: u32) -> ElleValue {
        (self.make_poll_fd)(fd, events)
    }

    // ── Result helpers ────────────────────────────────────────────

    pub fn ok(&self, v: ElleValue) -> ElleResult {
        ElleResult {
            signal: SIG_OK,
            value: v,
        }
    }

    pub fn err(&self, kind: &str, msg: &str) -> ElleResult {
        ElleResult {
            signal: SIG_ERROR,
            value: self.error(kind, msg),
        }
    }

    pub fn yield_io(&self, request: ElleValue) -> ElleResult {
        ElleResult {
            signal: SIG_YIELD | SIG_IO,
            value: request,
        }
    }

    // ── External object helpers ───────────────────────────────────

    /// Wrap a Rust value as an opaque external object.
    ///
    /// The value is heap-allocated and freed when elle GCs the value.
    pub fn external<T: 'static>(&self, name: &str, data: T) -> ElleValue {
        let ptr = Box::into_raw(Box::new(data)) as *mut c_void;
        extern "C" fn drop_fn<T>(p: *mut c_void) {
            unsafe {
                drop(Box::from_raw(p as *mut T));
            }
        }
        (self.make_external)(name.as_ptr(), name.len(), ptr, Some(drop_fn::<T>))
    }

    /// Extract a reference to a previously-wrapped external object.
    ///
    /// Returns None if the value is not an external of the given type name.
    pub fn get_external<'a, T: 'static>(&self, v: ElleValue, name: &str) -> Option<&'a T> {
        let ptr = (self.as_external)(v, name.as_ptr(), name.len());
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { &*(ptr as *const T) })
        }
    }

    /// Extract a mutable reference to a previously-wrapped external object.
    ///
    /// # Safety
    /// Caller must ensure exclusive access to the external object.
    pub unsafe fn get_external_mut<'a, T: 'static>(
        &self,
        v: ElleValue,
        name: &str,
    ) -> Option<&'a mut T> {
        let ptr = (self.as_external)(v, name.as_ptr(), name.len());
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *(ptr as *mut T) })
        }
    }

    // ── Accessor helpers ──────────────────────────────────────────

    pub fn get_int(&self, v: ElleValue) -> Option<i64> {
        let mut out = 0i64;
        if (self.as_int)(v, &mut out) {
            Some(out)
        } else {
            None
        }
    }

    pub fn get_float(&self, v: ElleValue) -> Option<f64> {
        let mut out = 0f64;
        if (self.as_float)(v, &mut out) {
            Some(out)
        } else {
            None
        }
    }

    pub fn get_bool(&self, v: ElleValue) -> Option<bool> {
        match (self.as_bool)(v) {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    pub fn get_string<'a>(&self, v: ElleValue) -> Option<&'a str> {
        let mut len = 0usize;
        let ptr = (self.as_string)(v, &mut len);
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) })
        }
    }

    pub fn get_bytes<'a>(&self, v: ElleValue) -> Option<&'a [u8]> {
        let mut len = 0usize;
        let ptr = (self.as_bytes)(v, &mut len);
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { std::slice::from_raw_parts(ptr, len) })
        }
    }

    pub fn get_array_len(&self, v: ElleValue) -> Option<usize> {
        let len = (self.array_len)(v);
        if len < 0 {
            None
        } else {
            Some(len as usize)
        }
    }

    pub fn get_array_item(&self, v: ElleValue, idx: usize) -> ElleValue {
        (self.array_get)(v, idx)
    }

    /// Convert a proper list (cons chain) to an immutable array.
    /// Returns `None` if the value is not a proper list.
    pub fn list_to_array(&self, v: ElleValue) -> Option<ElleValue> {
        let result = (self.list_to_array)(v);
        if self.check_nil(result) {
            None
        } else {
            Some(result)
        }
    }

    pub fn get_struct_field(&self, v: ElleValue, key: &str) -> ElleValue {
        (self.struct_get)(v, key.as_ptr(), key.len())
    }

    pub fn get_struct_len(&self, v: ElleValue) -> Option<usize> {
        let n = (self.struct_len)(v);
        if n < 0 {
            None
        } else {
            Some(n as usize)
        }
    }

    pub fn get_struct_key<'a>(&self, v: ElleValue, idx: usize) -> Option<&'a str> {
        let mut len = 0usize;
        let ptr = (self.struct_key)(v, idx, &mut len);
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) })
        }
    }

    pub fn get_struct_value(&self, v: ElleValue, idx: usize) -> ElleValue {
        (self.struct_value)(v, idx)
    }

    /// Iterate struct entries as (key, value) pairs.
    pub fn struct_entries(&self, v: ElleValue) -> Vec<(&str, ElleValue)> {
        let n = match self.get_struct_len(v) {
            Some(n) => n,
            None => return Vec::new(),
        };
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            if let Some(k) = self.get_struct_key(v, i) {
                out.push((k, self.get_struct_value(v, i)));
            }
        }
        out
    }

    pub fn kw_intern(&self, name: &str) -> u64 {
        (self.intern_keyword)(name.as_ptr(), name.len())
    }

    pub fn kw_name<'a>(&self, hash: u64) -> Option<&'a str> {
        let mut len = 0usize;
        let ptr = (self.keyword_name)(hash, &mut len);
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) })
        }
    }

    // ── Type predicates ────────────────────────────────────────

    pub fn check_string(&self, v: ElleValue) -> bool {
        (self.is_string)(v)
    }
    pub fn check_keyword(&self, v: ElleValue) -> bool {
        (self.is_keyword)(v)
    }
    pub fn check_bytes(&self, v: ElleValue) -> bool {
        (self.is_bytes)(v)
    }
    pub fn check_array(&self, v: ElleValue) -> bool {
        (self.is_array)(v)
    }
    pub fn check_struct(&self, v: ElleValue) -> bool {
        (self.is_struct)(v)
    }
    pub fn check_int(&self, v: ElleValue) -> bool {
        (self.is_int)(v)
    }
    pub fn check_float(&self, v: ElleValue) -> bool {
        (self.is_float)(v)
    }
    pub fn check_bool(&self, v: ElleValue) -> bool {
        (self.is_bool_val)(v)
    }
    pub fn check_nil(&self, v: ElleValue) -> bool {
        (self.is_nil)(v)
    }
    pub fn check_external(&self, v: ElleValue) -> bool {
        (self.is_external)(v)
    }

    pub fn get_keyword_name<'a>(&self, v: ElleValue) -> Option<&'a str> {
        let mut len = 0usize;
        let ptr = (self.as_keyword_name)(v, &mut len);
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) })
        }
    }

    pub fn eq(&self, a: ElleValue, b: ElleValue) -> bool {
        (self.value_eq)(a, b)
    }

    pub fn type_name<'a>(&self, v: ElleValue) -> &'a str {
        let mut len = 0usize;
        let ptr = (self.type_name_of)(v, &mut len);
        if ptr.is_null() || len == 0 {
            "unknown"
        } else {
            unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) }
        }
    }

    /// Extract argument at index, panicking on out of bounds.
    ///
    /// # Safety
    /// `args` must point to a valid array of at least `nargs` elements.
    #[inline]
    pub unsafe fn arg(&self, args: *const ElleValue, nargs: usize, idx: usize) -> ElleValue {
        assert!(
            idx < nargs,
            "argument index {} out of bounds (nargs={})",
            idx,
            nargs
        );
        *args.add(idx)
    }

    /// Get argument slice from raw pointer + length.
    ///
    /// # Safety
    /// `args` must point to a valid array of at least `nargs` elements.
    #[inline]
    pub unsafe fn args<'a>(&self, args: *const ElleValue, nargs: usize) -> &'a [ElleValue] {
        if nargs == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(args, nargs)
        }
    }
}

// ── Plugin entry point macro ──────────────────────────────────────────

/// Generate the `elle_plugin_init` entry point for a stable-ABI plugin.
///
/// Usage:
/// ```ignore
/// elle_plugin::define_plugin!("prefix/", &PRIMITIVES);
/// ```
///
/// Expands to:
/// - A `static API: OnceLock<Api>` storing the resolved API
/// - A `pub fn api() -> &'static Api` accessor
/// - A `#[no_mangle] pub extern "C" fn elle_plugin_init` entry point
#[macro_export]
macro_rules! define_plugin {
    ($prefix:expr, $prims:expr) => {
        static API: std::sync::OnceLock<$crate::Api> = std::sync::OnceLock::new();

        /// Get the resolved API. Panics if called before plugin init.
        pub fn api() -> &'static $crate::Api {
            API.get()
                .expect("plugin not initialized — api() called before elle_plugin_init")
        }

        #[no_mangle]
        pub extern "C" fn elle_plugin_init(
            loader: &$crate::ElleApiLoader,
            ctx: &mut $crate::EllePluginCtx,
        ) -> i32 {
            let resolved = match $crate::Api::load(loader) {
                Ok(a) => a,
                Err(_name) => return -1,
            };
            API.set(resolved).ok();
            for def in $prims.iter() {
                (ctx.register)(
                    ctx as *mut $crate::EllePluginCtx,
                    def as *const $crate::EllePrimDef,
                );
            }
            0
        }
    };
}

// ── Primitive definition helpers ──────────────────────────────────────

/// Helper to build a static `EllePrimDef` from string literals.
///
/// Signal constants: `SIG_OK`, `SIG_ERROR`, `SIG_IO`, etc.
/// Arity kinds: 0 = exact, 1 = at_least, 2 = range.
impl EllePrimDef {
    /// Construct with exact arity.
    pub const fn exact(
        name: &'static str,
        func: EllePrimFn,
        signal: u32,
        arity: u16,
        doc: &'static str,
        category: &'static str,
        example: &'static str,
    ) -> Self {
        EllePrimDef {
            name: name.as_ptr(),
            name_len: name.len(),
            func,
            signal,
            arity_kind: 0,
            arity_min: arity,
            arity_max: arity,
            doc: doc.as_ptr(),
            doc_len: doc.len(),
            category: category.as_ptr(),
            category_len: category.len(),
            example: example.as_ptr(),
            example_len: example.len(),
        }
    }

    /// Construct with at-least arity.
    pub const fn at_least(
        name: &'static str,
        func: EllePrimFn,
        signal: u32,
        min: u16,
        doc: &'static str,
        category: &'static str,
        example: &'static str,
    ) -> Self {
        EllePrimDef {
            name: name.as_ptr(),
            name_len: name.len(),
            func,
            signal,
            arity_kind: 1,
            arity_min: min,
            arity_max: 0,
            doc: doc.as_ptr(),
            doc_len: doc.len(),
            category: category.as_ptr(),
            category_len: category.len(),
            example: example.as_ptr(),
            example_len: example.len(),
        }
    }

    /// Construct with range arity.
    #[allow(clippy::too_many_arguments)]
    pub const fn range(
        name: &'static str,
        func: EllePrimFn,
        signal: u32,
        min: u16,
        max: u16,
        doc: &'static str,
        category: &'static str,
        example: &'static str,
    ) -> Self {
        EllePrimDef {
            name: name.as_ptr(),
            name_len: name.len(),
            func,
            signal,
            arity_kind: 2,
            arity_min: min,
            arity_max: max,
            doc: doc.as_ptr(),
            doc_len: doc.len(),
            category: category.as_ptr(),
            category_len: category.len(),
            example: example.as_ptr(),
            example_len: example.len(),
        }
    }
}
