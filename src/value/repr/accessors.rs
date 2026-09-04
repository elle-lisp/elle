//! Value accessors for extracting typed data from Values.

use std::any::Any;

use super::{
    Value, TAG_ARRAY, TAG_ARRAY_MUT, TAG_BYTES, TAG_BYTES_MUT, TAG_CAPTURE_CELL, TAG_CLOSURE,
    TAG_CONS, TAG_EXTERNAL, TAG_FALSE, TAG_FFI_SIG, TAG_FFI_TYPE, TAG_FIBER, TAG_LBOX,
    TAG_LIB_HANDLE, TAG_MANAGED_PTR, TAG_NATIVE_FN, TAG_PARAMETER, TAG_SET, TAG_SET_MUT,
    TAG_STRING, TAG_STRING_MUT, TAG_STRUCT, TAG_STRUCT_MUT, TAG_SYNTAX, TAG_THREAD, TAG_TRUE,
};

mod predicates;

mod conversions;

impl Value {
    // =========================================================================
    // Immediate Value Extractors
    // =========================================================================

    /// Extract the keyword hash. Returns None if not a keyword.
    /// Fast path — no lock acquisition, no allocation.
    #[inline]
    pub fn keyword_hash(&self) -> Option<u64> {
        if self.is_keyword() {
            Some(self.payload)
        } else {
            None
        }
    }

    // =========================================================================
    // Heap Type Predicates
    // =========================================================================

    /// Get the heap tag if this is a heap value.
    #[inline]
    pub fn heap_tag(&self) -> Option<crate::value::heap::HeapTag> {
        use crate::value::heap::deref;
        if self.is_heap() {
            Some(unsafe { deref(*self).tag() })
        } else {
            None
        }
    }

    // =========================================================================
    // Heap Value Extractors
    // =========================================================================

    /// The contents of an immutable string, borrowed from its region pages.
    /// `None` for every other value, `@string` included — a mutable store is
    /// behind a `RefCell` and cannot hand out a bare borrow.
    #[inline]
    pub fn as_str(&self) -> Option<&str> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LString { s, .. } => {
                // SAFETY: LString's bytes are always valid UTF-8 (enforced by
                // constructors). The arena outlives the borrow.
                Some(unsafe { std::str::from_utf8_unchecked(s.as_slice()) })
            }
            _ => None,
        }
    }

    /// Access string contents via closure. Works for heap strings.
    /// Returns None if this is not a string.
    #[inline]
    pub fn with_string<R>(&self, f: impl FnOnce(&str) -> R) -> Option<R> {
        self.as_str().map(f)
    }

    /// Compare two string values lexicographically.
    /// Returns None if either value is not a string.
    pub fn compare_str(&self, other: &Value) -> Option<std::cmp::Ordering> {
        self.with_string(|sa| other.with_string(|sb| sa.cmp(sb)))
            .flatten()
    }

    /// Compare two keyword values by hash — the portable order sorted
    /// containers rely on, deterministic in every instance and build.
    /// Returns None if either value is not a keyword.
    pub fn compare_keyword(&self, other: &Value) -> Option<std::cmp::Ordering> {
        match (self.keyword_hash(), other.keyword_hash()) {
            (Some(a), Some(b)) => Some(a.cmp(&b)),
            _ => None,
        }
    }

    /// Get a human-readable type name.
    pub fn type_name(&self) -> &'static str {
        use crate::value::heap::deref;
        if self.is_nil() {
            "nil"
        } else if self.is_empty_list() {
            "list" // empty list is still a list
        } else if self.is_bool() {
            "boolean"
        } else if self.is_int() {
            "integer"
        } else if self.is_float() {
            "float"
        } else if self.is_symbol() {
            "symbol"
        } else if self.is_keyword() {
            "keyword"
        } else if self.is_pointer() {
            "ptr"
        } else if self.is_native_fn() {
            // Immediate (tag below the heap boundary) — never deref.
            "native-fn"
        } else if self.is_heap() {
            unsafe { deref(*self).type_name() }
        } else {
            "unknown"
        }
    }

    /// Get or prepare the cached CIF for an FFI signature.
    /// Returns None if this is not an FFI signature.
    ///
    /// The CIF is lazily prepared on first access and cached for reuse.
    #[cfg(feature = "ffi")]
    pub fn get_or_prepare_cif(&self) -> Option<std::cell::Ref<'_, libffi::middle::Cif>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_ffi_sig() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::FFISignature(sig, cif_cache) => {
                // Prepare CIF if not cached
                {
                    let mut cache = cif_cache.borrow_mut();
                    if cache.is_none() {
                        *cache = Some(crate::ffi::call::prepare_cif(sig));
                    }
                }
                // Return a Ref to the cached CIF
                Some(std::cell::Ref::map(cif_cache.borrow(), |opt| {
                    opt.as_ref().unwrap()
                }))
            }
            _ => None,
        }
    }

    /// Try to extract an external object's data as a specific Rust type.
    pub fn as_external<T: Any + 'static>(&self) -> Option<&T> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_external() {
            return None;
        }
        unsafe {
            match deref(*self) {
                HeapObject::External { obj, .. } => obj.data.downcast_ref::<T>(),
                _ => None,
            }
        }
    }

    /// Get the type name of an external object, if this value is one.
    pub fn external_type_name(&self) -> Option<&'static str> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_external() {
            return None;
        }
        unsafe {
            match deref(*self) {
                HeapObject::External { obj, .. } => Some(obj.type_name),
                _ => None,
            }
        }
    }

    /// Convert a proper cons-list to a `Vec`. Plain lists only: a syntax-wrapped
    /// list allocates fresh syntax wrappers, so it needs [`Self::list_to_vec_in`], which
    /// takes the heap. A syntax object here is reported as an improper list.
    pub fn list_to_vec(&self) -> Result<Vec<Value>, &'static str> {
        if self.as_syntax().is_some() {
            return Err("Not a proper list");
        }
        self.cons_list_to_vec()
    }

    /// Convert a proper list to a `Vec`, unwrapping a syntax-wrapped list into
    /// `Value::syntax` items born in one fresh region on `heap`. The explicit heap
    /// is the allocation this performs made visible in the signature (the honesty
    /// invariant); a plain cons-list behaves exactly like [`Self::list_to_vec`].
    pub fn list_to_vec_in(
        &self,
        heap: &mut crate::value::fiberheap::FiberHeap,
    ) -> Result<Vec<Value>, &'static str> {
        if let Some(syntax) = self.as_syntax() {
            if let crate::syntax::SyntaxKind::List(items) = &syntax.kind {
                let region = heap.new_runtime_region();
                return Ok(items
                    .iter()
                    .map(|item| crate::value::build::syntax(&mut *heap, *item, region))
                    .collect());
            }
            return Err("Not a proper list");
        }
        self.cons_list_to_vec()
    }

    /// The allocation-free cons-list walk shared by `list_to_vec` and
    /// `list_to_vec_in`. Handles a syntax-wrapped `nil`/empty list reached as a
    /// list tail (e.g. from `letrec` in macros).
    fn cons_list_to_vec(&self) -> Result<Vec<Value>, &'static str> {
        let mut result = Vec::new();
        let mut current = *self;
        loop {
            if current.is_nil() || current.is_empty_list() {
                return Ok(result);
            }
            if let Some(syntax) = current.as_syntax() {
                match &syntax.kind {
                    crate::syntax::SyntaxKind::Nil => return Ok(result),
                    crate::syntax::SyntaxKind::List(items) if items.is_empty() => {
                        return Ok(result)
                    }
                    _ => {}
                }
            }
            if let Some(pair) = current.as_pair() {
                result.push(pair.first);
                current = pair.rest;
            } else {
                return Err("Not a proper list");
            }
        }
    }
}
