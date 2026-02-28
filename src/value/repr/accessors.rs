//! Value accessors for extracting typed data from Values.

use super::{
    Value, PAYLOAD_MASK, PTRVAL_PAYLOAD_MASK, SYMBOL_ID_MASK, TAG_FALSE, TAG_NAN, TAG_NAN_MASK,
    TAG_TRUE,
};

impl Value {
    // =========================================================================
    // Immediate Value Extractors
    // =========================================================================

    /// Extract as boolean if this is a bool.
    #[inline]
    pub fn as_bool(&self) -> Option<bool> {
        match self.0 {
            TAG_TRUE => Some(true),
            TAG_FALSE => Some(false),
            _ => None,
        }
    }

    /// Extract as integer if this is an int.
    #[inline]
    pub fn as_int(&self) -> Option<i64> {
        if self.is_int() {
            // Sign-extend from 48 bits
            let raw = (self.0 & PAYLOAD_MASK) as i64;
            // Check sign bit (bit 47)
            if raw & (1 << 47) != 0 {
                // Negative: extend sign bits
                Some(raw | !PAYLOAD_MASK as i64)
            } else {
                Some(raw)
            }
        } else {
            None
        }
    }

    /// Extract as float if this is a float.
    #[inline]
    pub fn as_float(&self) -> Option<f64> {
        if (self.0 & TAG_NAN_MASK) == TAG_NAN {
            // Reconstruct NaN/Infinity from our special tag
            // The payload contains the upper 16 bits of the float bits
            // The lower 48 bits are always zero for NaN/Infinity
            let upper_16 = self.0 & PAYLOAD_MASK;
            let bits = upper_16 << 48;
            Some(f64::from_bits(bits))
        } else if self.is_float() {
            Some(f64::from_bits(self.0))
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

    /// Extract symbol ID if this is a symbol.
    #[inline]
    pub fn as_symbol(&self) -> Option<u32> {
        if self.is_symbol() {
            Some((self.0 & SYMBOL_ID_MASK) as u32)
        } else {
            None
        }
    }

    /// Extract raw C pointer address if this is a pointer.
    #[inline]
    pub fn as_pointer(&self) -> Option<usize> {
        if self.is_pointer() {
            Some((self.0 & PTRVAL_PAYLOAD_MASK) as usize)
        } else {
            None
        }
    }

    /// Extract keyword name if this is a keyword.
    #[inline]
    pub fn as_keyword_name(&self) -> Option<&str> {
        if self.is_keyword() {
            let ptr = (self.0 & PTRVAL_PAYLOAD_MASK) as *const crate::value::heap::HeapObject;
            match unsafe { &*ptr } {
                crate::value::heap::HeapObject::String(s) => Some(s),
                _ => None,
            }
        } else {
            None
        }
    }

    /// Extract heap pointer if this is a heap value.
    #[inline]
    pub fn as_heap_ptr(&self) -> Option<*const ()> {
        if self.is_heap() {
            Some((self.0 & PAYLOAD_MASK) as *const ())
        } else {
            None
        }
    }

    // =========================================================================
    // Heap Type Predicates
    // =========================================================================

    /// Check if this is a string (SSO or heap).
    #[inline]
    pub fn is_string(&self) -> bool {
        use crate::value::heap::HeapTag;
        (self.0 & super::TAG_SSO_MASK) == super::TAG_SSO || self.heap_tag() == Some(HeapTag::String)
    }

    /// Check if this is a cons cell.
    #[inline]
    pub fn is_cons(&self) -> bool {
        use crate::value::heap::HeapTag;
        self.heap_tag() == Some(HeapTag::Cons)
    }

    /// Check if this is an array.
    #[inline]
    pub fn is_array(&self) -> bool {
        use crate::value::heap::HeapTag;
        self.heap_tag() == Some(HeapTag::Array)
    }

    /// Check if this is a table.
    #[inline]
    pub fn is_table(&self) -> bool {
        use crate::value::heap::HeapTag;
        self.heap_tag() == Some(HeapTag::Table)
    }

    /// Check if this is a struct.
    #[inline]
    pub fn is_struct(&self) -> bool {
        use crate::value::heap::HeapTag;
        self.heap_tag() == Some(HeapTag::Struct)
    }

    /// Check if this is a closure.
    #[inline]
    pub fn is_closure(&self) -> bool {
        use crate::value::heap::HeapTag;
        self.heap_tag() == Some(HeapTag::Closure)
    }

    /// Check if this is a cell.
    #[inline]
    pub fn is_cell(&self) -> bool {
        use crate::value::heap::HeapTag;
        self.heap_tag() == Some(HeapTag::Cell)
    }

    /// Check if this is a fiber.
    #[inline]
    pub fn is_fiber(&self) -> bool {
        use crate::value::heap::HeapTag;
        self.heap_tag() == Some(HeapTag::Fiber)
    }

    /// Check if this is a buffer.
    #[inline]
    pub fn is_buffer(&self) -> bool {
        use crate::value::heap::HeapTag;
        self.heap_tag() == Some(HeapTag::Buffer)
    }

    /// Check if this is a bytes value.
    #[inline]
    pub fn is_bytes(&self) -> bool {
        use crate::value::heap::HeapTag;
        self.heap_tag() == Some(HeapTag::Bytes)
    }

    /// Check if this is a blob value.
    #[inline]
    pub fn is_blob(&self) -> bool {
        use crate::value::heap::HeapTag;
        self.heap_tag() == Some(HeapTag::Blob)
    }

    /// Check if this is a syntax object.
    #[inline]
    pub fn is_syntax(&self) -> bool {
        use crate::value::heap::HeapTag;
        self.heap_tag() == Some(HeapTag::Syntax)
    }

    /// Check if this is a proper list (nil or cons ending in nil).
    pub fn is_list(&self) -> bool {
        let mut current = *self;
        loop {
            if current.is_nil() || current.is_empty_list() {
                return true;
            }
            if let Some(cons) = current.as_cons() {
                current = cons.rest;
            } else {
                return false;
            }
        }
    }

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

    /// Access string contents via closure. Works for both SSO and heap strings.
    /// Returns None if this is not a string.
    #[inline]
    pub fn with_string<R>(&self, f: impl FnOnce(&str) -> R) -> Option<R> {
        if (self.0 & super::TAG_SSO_MASK) == super::TAG_SSO {
            let payload = self.0 & super::PAYLOAD_MASK;
            let mut buf = [0u8; 6];
            for (i, byte) in buf.iter_mut().enumerate() {
                *byte = ((payload >> (i * 8)) & 0xFF) as u8;
            }
            // Find length: first zero byte, or 6 if all non-zero
            let len = buf.iter().position(|&b| b == 0).unwrap_or(6);
            // SAFETY: Value::string() only creates SSO from valid UTF-8
            let s = unsafe { std::str::from_utf8_unchecked(&buf[..len]) };
            Some(f(s))
        } else {
            use crate::value::heap::{deref, HeapObject};
            if !self.is_heap() {
                return None;
            }
            match unsafe { deref(*self) } {
                HeapObject::String(s) => Some(f(s)),
                _ => None,
            }
        }
    }

    /// Extract as cons if this is a cons cell.
    #[inline]
    pub fn as_cons(&self) -> Option<&crate::value::heap::Cons> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Cons(c) => Some(c),
            _ => None,
        }
    }

    /// Extract as array if this is an array.
    #[inline]
    pub fn as_array(&self) -> Option<&std::cell::RefCell<Vec<Value>>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Array(v) => Some(v),
            _ => None,
        }
    }

    /// Extract as table if this is a table.
    #[inline]
    pub fn as_table(
        &self,
    ) -> Option<&std::cell::RefCell<std::collections::BTreeMap<crate::value::heap::TableKey, Value>>>
    {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Table(t) => Some(t),
            _ => None,
        }
    }

    /// Extract as struct if this is a struct.
    #[inline]
    pub fn as_struct(
        &self,
    ) -> Option<&std::collections::BTreeMap<crate::value::heap::TableKey, Value>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Struct(s) => Some(s),
            _ => None,
        }
    }

    /// Extract as closure if this is a closure.
    #[inline]
    pub fn as_closure(&self) -> Option<&std::rc::Rc<crate::value::heap::Closure>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Closure(c) => Some(c),
            _ => None,
        }
    }

    /// Extract as cell if this is a cell.
    #[inline]
    pub fn as_cell(&self) -> Option<&std::cell::RefCell<Value>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Cell(c, _) => Some(c),
            _ => None,
        }
    }

    /// Check if this is a compiler-created local cell (auto-unwrapped by LoadUpvalue).
    #[inline]
    pub fn is_local_cell(&self) -> bool {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return false;
        }
        unsafe { matches!(deref(*self), HeapObject::Cell(_, true)) }
    }

    /// Extract as native function if this is a native function.
    #[inline]
    pub fn as_native_fn(&self) -> Option<&crate::value::heap::NativeFn> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::NativeFn(f) => Some(f),
            _ => None,
        }
    }

    /// Extract as tuple if this is a tuple.
    #[inline]
    pub fn as_tuple(&self) -> Option<&[Value]> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Tuple(elems) => Some(elems),
            _ => None,
        }
    }

    /// Check if this value is a tuple.
    #[inline]
    pub fn is_tuple(&self) -> bool {
        self.as_tuple().is_some()
    }

    /// Extract as buffer if this is a buffer.
    #[inline]
    pub fn as_buffer(&self) -> Option<&std::cell::RefCell<Vec<u8>>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Buffer(b) => Some(b),
            _ => None,
        }
    }

    /// Extract as bytes if this is a bytes value.
    #[inline]
    pub fn as_bytes(&self) -> Option<&[u8]> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Bytes(b) => Some(b),
            _ => None,
        }
    }

    /// Extract as blob if this is a blob value.
    #[inline]
    pub fn as_blob(&self) -> Option<&std::cell::RefCell<Vec<u8>>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Blob(b) => Some(b),
            _ => None,
        }
    }

    /// Extract as thread handle if this is a thread handle.
    #[inline]
    pub fn as_thread_handle(&self) -> Option<&crate::value::heap::ThreadHandle> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::ThreadHandle(h) => Some(h),
            _ => None,
        }
    }

    /// Extract as fiber handle if this is a fiber.
    #[inline]
    pub fn as_fiber(&self) -> Option<&crate::value::fiber::FiberHandle> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Fiber(handle) => Some(handle),
            _ => None,
        }
    }

    /// Extract as syntax if this is a syntax object.
    #[inline]
    pub fn as_syntax(&self) -> Option<&std::rc::Rc<crate::syntax::Syntax>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Syntax(s) => Some(s),
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
        } else if (self.0 & super::TAG_SSO_MASK) == super::TAG_SSO {
            "string"
        } else if self.is_symbol() {
            "symbol"
        } else if self.is_keyword() {
            "keyword"
        } else if self.is_pointer() {
            "pointer"
        } else if self.is_heap() {
            unsafe { deref(*self).type_name() }
        } else {
            "unknown"
        }
    }

    /// Check if this is a binding.
    #[inline]
    pub fn is_binding(&self) -> bool {
        use crate::value::heap::HeapTag;
        self.heap_tag() == Some(HeapTag::Binding)
    }

    /// Extract as FFI signature if this is an FFI signature.
    #[inline]
    pub fn as_ffi_signature(&self) -> Option<&crate::ffi::types::Signature> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::FFISignature(sig, _) => Some(sig),
            _ => None,
        }
    }

    /// Extract as FFI type descriptor if this is an FFI type.
    #[inline]
    pub fn as_ffi_type(&self) -> Option<&crate::ffi::types::TypeDesc> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::FFIType(desc) => Some(desc),
            _ => None,
        }
    }

    /// Get or prepare the cached CIF for an FFI signature.
    /// Returns None if this is not an FFI signature.
    ///
    /// The CIF is lazily prepared on first access and cached for reuse.
    pub fn get_or_prepare_cif(&self) -> Option<std::cell::Ref<'_, libffi::middle::Cif>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
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

    /// Extract as library handle ID if this is a library handle.
    #[inline]
    pub fn as_lib_handle(&self) -> Option<u32> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
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
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::ManagedPointer(cell) => Some(cell),
            _ => None,
        }
    }

    /// Extract as binding inner if this is a binding.
    #[inline]
    pub fn as_binding(&self) -> Option<&std::cell::RefCell<crate::value::heap::BindingInner>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Binding(inner) => Some(inner),
            _ => None,
        }
    }

    /// Convert a proper list to a Vec.
    pub fn list_to_vec(&self) -> Result<Vec<Value>, &'static str> {
        // Syntax lists: unwrap SyntaxKind::List items as Value::syntax each
        if let Some(syntax) = self.as_syntax() {
            if let crate::syntax::SyntaxKind::List(items) = &syntax.kind {
                return Ok(items
                    .iter()
                    .map(|item| Value::syntax(item.clone()))
                    .collect());
            }
            return Err("Not a proper list");
        }
        let mut result = Vec::new();
        let mut current = *self;
        loop {
            if current.is_nil() || current.is_empty_list() {
                return Ok(result);
            }
            if let Some(cons) = current.as_cons() {
                result.push(cons.first);
                current = cons.rest;
            } else {
                return Err("Not a proper list");
            }
        }
    }
}
