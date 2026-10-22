// audited: 2026-10-22
//! The ergonomic `ctx.*` allocation surface: one constructor per heap type,
//! each born on the ctx's heap in the ctx's own region.
//! docs/impl/region/ctx.md

use super::Alloc;
use crate::value::Value;

/// Generate the ergonomic `ctx.*` constructors, one spec line per heap type.
/// Each forwards to the matching `value::build::*` source with the ctx's own
/// heap and region, so a native body reads `ctx.string("x")`.
macro_rules! ctx_ctors {
    ( $(
        $(#[$attr:meta])*
        $name:ident ( $($arg:ident : $ty:ty),* $(,)? ) ;
    )* ) => {
        impl<'h> Alloc<'h> {
            $(
                $(#[$attr])*
                #[inline]
                // `&self`, not `&mut self`: `self.heap()` reborrows the ctx's
                // raw heap pointer for exactly this allocation, so nested
                // constructor calls (`ctx.pair(ctx.string(a), b)`) borrow-check —
                // the inner reborrow drops before the outer one is taken.
                pub fn $name(&self $(, $arg: $ty)*) -> Value {
                    crate::value::build::$name(self.heap() $(, $arg)*, self.region)
                }
            )*
        }
    };
}

ctx_ctors! {
    /// Allocate a string into the call's region.
    string (s: impl AsRef<str>);
    /// Allocate a cons cell into the call's region.
    pair (head: Value, tail: Value);
    /// Allocate an immutable array into the call's region.
    array (elements: Vec<Value>);
    /// Allocate a mutable `@array` into the call's region.
    array_mut (elements: Vec<Value>);
    /// Allocate an empty mutable `@struct` into the call's region.
    struct_mut ();
    /// Allocate a mutable `@struct` with entries into the call's region.
    struct_mut_from (
        entries: std::collections::BTreeMap<crate::value::heap::TableKey, Value>
    );
    /// Allocate an immutable struct (from an unsorted map) into the call's region.
    struct_from (
        fields: std::collections::BTreeMap<crate::value::heap::TableKey, Value>
    );
    /// Allocate an immutable struct (from pre-sorted entries) into the call's region.
    struct_from_sorted (
        entries: Vec<(crate::value::heap::TableKey, Value)>
    );
    /// Allocate a closure into the call's region.
    closure (c: crate::value::heap::Closure);
    /// Allocate a header over a code object's payload into the call's region.
    template (code: &crate::value::ClosureTemplate);
    /// Allocate a user box (`LBox`) into the call's region.
    lbox (value: Value);
    /// Allocate a compiler capture cell into the call's region.
    capture_cell (value: Value, origin: crate::value::heap::CellOrigin);
    /// Allocate a mutable `@string` into the call's region.
    string_mut (bytes: Vec<u8>);
    /// Allocate a mutable `@string` carrying `traits` into the call's region.
    string_mut_with_traits (bytes: &[u8], traits: Value);
    /// Allocate immutable bytes into the call's region.
    bytes (data: Vec<u8>);
    /// Allocate `len` zero bytes into the call's region, with no Rust buffer
    /// built first.
    zeroed_bytes (len: usize);
    /// Allocate the concatenation of `parts` as bytes into the call's region.
    joined_bytes (parts: &[&[u8]]);
    /// A bytes header in the call's region over `backing`, with no copy: a
    /// view (docs/impl/values.md). `backing` must be the payload of a string
    /// or bytes resident in a region of this heap.
    bytes_view (backing: &[u8]);
    /// A string header in the call's region over `backing`, a view as
    /// `bytes_view` is.
    string_view (backing: &str);
    /// Allocate mutable `@bytes` into the call's region.
    bytes_mut (data: Vec<u8>);
    /// Allocate mutable `@bytes` carrying `traits` into the call's region.
    bytes_mut_with_traits (data: &[u8], traits: Value);
    /// Allocate a syntax object into the call's region.
    syntax (s: crate::syntax::Syntax);
    /// Allocate an immutable set into the call's region.
    set (items: std::collections::BTreeSet<Value>);
    /// Allocate a mutable set into the call's region.
    set_mut (items: std::collections::BTreeSet<Value>);
    /// Allocate a managed FFI pointer into the call's region (NULL ⇒ nil).
    managed_pointer (addr: usize);
}

impl<'h> Alloc<'h> {
    /// Construct an error value `{:error :kind :message msg}` born on the ctx's
    /// heap in the call's region (the ergonomic forwarder a native body uses
    /// instead of `error_val_in`).
    ///
    /// The kind keyword is an immediate: identity only, **spelling not
    /// recorded**. `Alloc` holds no symbol table, so this method records
    /// nothing. [`NativeCtx::error`](super::NativeCtx::error) overrides it to
    /// record the spelling first, and every native that holds a ctx holds a
    /// `NativeCtx`. The bare-boundary constructors reach this method directly
    /// and have no memo to record into (docs/impl/symbol.md).
    #[inline]
    pub fn error(&self, kind: &str, msg: impl Into<String>) -> Value {
        crate::value::build::error(self.heap(), kind, msg, self.region)
    }

    /// Construct an error value with extra context fields, born in the call's
    /// region — the ctx forwarder for `error_val_extra_in`.
    #[inline]
    pub fn error_extra(
        &self,
        kind: &str,
        msg: impl Into<String>,
        extra: &[(&str, Value)],
    ) -> Value {
        crate::value::build::error_extra(self.heap(), kind, msg, extra, self.region)
    }

    /// The runtime no-match error for `match`, born in the call's region —
    /// the ctx forwarder for `match_fail_error_in`.
    #[inline]
    pub fn match_fail(&self, val: Value) -> Value {
        crate::value::build::match_fail(self.heap(), val, self.region)
    }

    /// Allocate an opaque external (plugin-provided) object into the call's
    /// region. Hand-written rather than macro-generated because of the generic
    /// `T`.
    #[inline]
    pub fn external<T: std::any::Any + 'static>(&self, type_name: &'static str, data: T) -> Value {
        crate::value::build::external(self.heap(), type_name, data, self.region)
    }

    /// Build a proper list (cons chain) into the call's region — every cell on
    /// the ctx's own heap. Hand-written rather than macro-generated because of
    /// the generic `IntoIterator`.
    #[inline]
    pub fn list(&self, values: impl IntoIterator<Item = Value>) -> Value {
        crate::value::build::list(self.heap(), values, self.region)
    }

    // ── Single-object ctors with no `value::build::*` twin ──────────────
    //
    // These wrap one `HeapObject` (no `RegionSlice` payload), so they allocate
    // directly into the call's region via `alloc`. Hand-written because their
    // arg shapes / construction logic are specific (a minted id, a handle
    // wrapper, an FFI descriptor).

    /// Allocate a dynamic `parameter` with `default` into the call's region.
    #[inline]
    pub fn parameter(&self, default: Value) -> Value {
        use crate::value::heap::HeapObject;
        self.alloc(HeapObject::Parameter {
            id: crate::value::parameter::mint(),
            default,
            traits: Value::NIL,
        })
    }

    /// Allocate a `fiber` into the call's region.
    #[inline]
    pub fn fiber(&self, f: crate::value::fiber::Fiber) -> Value {
        use crate::value::fiber::FiberHandle;
        use crate::value::heap::HeapObject;
        self.alloc(HeapObject::Fiber {
            handle: FiberHandle::new(f),
            traits: Value::NIL,
        })
    }

    /// Allocate a `fiber` from an existing handle into the call's region.
    #[inline]
    pub fn fiber_from_handle(&self, handle: crate::value::fiber::FiberHandle) -> Value {
        use crate::value::heap::HeapObject;
        self.alloc(HeapObject::Fiber {
            handle,
            traits: Value::NIL,
        })
    }

    /// Allocate an FFI compound type descriptor into the call's region.
    #[inline]
    pub fn ffi_type(&self, desc: crate::ffi::types::TypeDesc) -> Value {
        use crate::value::heap::HeapObject;
        self.alloc(HeapObject::FFIType(desc))
    }

    /// Allocate an FFI signature into the call's region.
    #[inline]
    pub fn ffi_signature(&self, sig: crate::ffi::types::Signature) -> Value {
        use crate::value::heap::{CifCache, HeapObject};
        #[cfg(feature = "ffi")]
        let cache: CifCache = std::cell::RefCell::new(None);
        #[cfg(not(feature = "ffi"))]
        let cache: CifCache = ();
        self.alloc(HeapObject::FFISignature(sig, cache))
    }

    /// Allocate a library handle into the call's region.
    #[inline]
    pub fn lib_handle(&self, id: u32) -> Value {
        use crate::value::heap::HeapObject;
        self.alloc(HeapObject::LibHandle(id))
    }
}
