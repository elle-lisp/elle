// audited: 2026-10-22
//! The typed extractors: each answers its payload for a value of its own type, and `None` for any other.
//!
//! docs/impl/values.md

use super::*;

impl Value {
    /// Extract as boolean if this is a bool.
    #[inline]
    pub fn as_bool(&self) -> Option<bool> {
        match self.tag {
            TAG_TRUE => Some(true),
            TAG_FALSE => Some(false),
            _ => None,
        }
    }
    /// Extract as integer if this is an int.
    #[inline]
    pub fn as_int(&self) -> Option<i64> {
        if self.is_int() {
            Some(self.payload as i64)
        } else {
            None
        }
    }
    /// Extract as float if this is a float.
    #[inline]
    pub fn as_float(&self) -> Option<f64> {
        if self.is_float() {
            Some(f64::from_bits(self.payload))
        } else {
            None
        }
    }
    /// Extract as number (float), coercing integers.
    #[inline]
    pub fn as_number(&self) -> Option<f64> {
        if let Some(i) = self.as_int() {
            Some(i as f64)
        } else {
            self.as_float()
        }
    }
    /// Extract symbol identity if this is a symbol.
    #[inline]
    pub fn as_symbol(&self) -> Option<crate::value::SymbolId> {
        if self.is_symbol() {
            Some(crate::value::SymbolId(self.payload))
        } else {
            None
        }
    }
    /// Extract raw C pointer address if this is a pointer.
    #[inline]
    pub fn as_pointer(&self) -> Option<usize> {
        if self.is_pointer() {
            Some(self.payload as usize)
        } else {
            None
        }
    }
    /// Whether this value is the keyword spelled `name` — one hash compare,
    /// no table, no allocation (docs/impl/symbol.md). Spelling recovery goes
    /// through
    /// `keyword::resolve_keyword_name` with a memo instead.
    #[inline]
    pub fn is_keyword_named(&self, name: &str) -> bool {
        self.keyword_hash() == Some(crate::value::keyword::keyword_hash(name))
    }
    /// Extract heap pointer if this is a heap value.
    #[inline]
    pub fn as_heap_ptr(&self) -> Option<*const ()> {
        if self.is_heap() {
            Some(self.payload as *const ())
        } else {
            None
        }
    }
    /// Extract as pair if this is a pair cell.
    #[inline]
    pub fn as_pair(&self) -> Option<&crate::value::heap::Pair> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_pair() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Pair(c) => Some(c),
            _ => None,
        }
    }
    /// Extract as struct if this is a struct.
    /// Returns a sorted slice of (key, value) pairs.
    #[inline]
    pub fn as_struct(&self) -> Option<&[(crate::value::heap::TableKey, Value)]> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_struct() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LStruct { data, .. } => Some(data),
            _ => None,
        }
    }
    /// Extract as closure if this is a closure.
    ///
    /// Returns a borrow of the arena-resident `Closure`. A holder that needs
    /// an owned `Rc<Closure>`, as a `Fiber` does, clones explicitly:
    /// `Rc::new(value.as_closure().unwrap().clone())`.
    /// `Closure::clone` is O(1) — every field is `Copy`.
    #[inline]
    pub fn as_closure(&self) -> Option<&crate::value::heap::Closure> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_closure() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Closure { closure, .. } => Some(closure),
            _ => None,
        }
    }
    /// Extract the primitive definition if this is a native function.
    #[inline]
    pub fn as_native_def(&self) -> Option<&'static crate::primitives::def::PrimitiveDef> {
        if !self.is_native_fn() {
            return None;
        }
        // Immediate: the payload is the prim_id — the def's dense id in the
        // primitive registry. No deref (a native-fn has no heap cell).
        crate::primitives::prim_def(self.payload as u32)
    }
    /// Extract the bare function pointer if this is a native function.
    #[inline]
    pub fn as_native_fn(&self) -> Option<crate::value::heap::PrimFn> {
        self.as_native_def().map(|def| def.func)
    }
    /// Extract as array (immutable) if this is one.
    #[inline]
    pub fn as_array(&self) -> Option<&[Value]> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_array() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LArray { elements, .. } => Some(elements),
            _ => None,
        }
    }
    /// Extract as set if this is a set.
    /// Returns a sorted slice of values (binary search for membership).
    #[inline]
    pub fn as_set(&self) -> Option<&[Value]> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_set() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LSet { data, .. } => Some(data.as_slice()),
            _ => None,
        }
    }
    /// Extract as bytes if this is a bytes value.
    #[inline]
    pub fn as_bytes(&self) -> Option<&[u8]> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_bytes() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LBytes { data, .. } => Some(data),
            _ => None,
        }
    }
    /// Extract as thread handle if this is a thread handle.
    #[inline]
    pub fn as_thread_handle(&self) -> Option<&crate::value::heap::ThreadHandle> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_thread() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::ThreadHandle { handle, .. } => Some(handle),
            _ => None,
        }
    }
    /// Extract as fiber handle if this is a fiber.
    #[inline]
    pub fn as_fiber(&self) -> Option<&crate::value::fiber::FiberHandle> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_fiber() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Fiber { handle, .. } => Some(handle),
            _ => None,
        }
    }
    /// Extract as syntax if this is a syntax object.
    #[inline]
    pub fn as_syntax(&self) -> Option<&crate::syntax::Syntax> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_syntax() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Syntax { syntax, .. } => Some(syntax),
            _ => None,
        }
    }
    /// Extract parameter (id, default) if this is a parameter.
    #[inline]
    pub fn as_parameter(&self) -> Option<(u32, Value)> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_parameter() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Parameter { id, default, .. } => Some((*id, *default)),
            _ => None,
        }
    }
    /// Extract as FFI signature if this is an FFI signature.
    #[inline]
    pub fn as_ffi_signature(&self) -> Option<&crate::ffi::types::Signature> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_ffi_sig() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::FFISignature(sig, ..) => Some(sig),
            _ => None,
        }
    }
    /// Extract as FFI type descriptor if this is an FFI type.
    #[inline]
    pub fn as_ffi_type(&self) -> Option<&crate::ffi::types::TypeDesc> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_ffi_type() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::FFIType(desc) => Some(desc),
            _ => None,
        }
    }
    /// Extract as library handle ID if this is a library handle.
    #[inline]
    pub fn as_lib_handle(&self) -> Option<u32> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_lib_handle() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LibHandle(id) => Some(*id),
            _ => None,
        }
    }
    /// Extract the managed pointer cell, if this is a managed pointer.
    #[inline]
    pub fn as_managed_pointer(&self) -> Option<&std::cell::Cell<Option<usize>>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_managed_pointer() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::ManagedPointer { addr, .. } => Some(addr),
            _ => None,
        }
    }
}
