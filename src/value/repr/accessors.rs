//! Value accessors for extracting typed data from Values.

use std::any::Any;

use super::{
    Value, TAG_ARRAY, TAG_ARRAY_MUT, TAG_BYTES, TAG_BYTES_MUT, TAG_CAPTURE_CELL, TAG_CLOSURE,
    TAG_CONS, TAG_EXTERNAL, TAG_FALSE, TAG_FFI_SIG, TAG_FFI_TYPE, TAG_FIBER, TAG_LBOX,
    TAG_LIB_HANDLE, TAG_MANAGED_PTR, TAG_NATIVE_FN, TAG_PARAMETER, TAG_SET, TAG_SET_MUT,
    TAG_STRING, TAG_STRING_MUT, TAG_STRUCT, TAG_STRUCT_MUT, TAG_SYNTAX, TAG_THREAD, TAG_TRUE,
};

impl Value {
    // =========================================================================
    // Immediate Value Extractors
    // =========================================================================

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

    /// Extract symbol ID if this is a symbol.
    #[inline]
    pub fn as_symbol(&self) -> Option<u32> {
        if self.is_symbol() {
            Some(self.payload as u32)
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

    /// Extract keyword name if this is a keyword.
    /// Acquires RwLock read lock and allocates a String.
    /// Use `keyword_hash()` when only comparing, not displaying.
    #[inline]
    pub fn as_keyword_name(&self) -> Option<String> {
        if self.is_keyword() {
            crate::value::keyword::keyword_name(self.payload)
        } else {
            None
        }
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

    // =========================================================================
    // Heap Type Predicates
    // =========================================================================

    /// Check if this is a string (immutable heap string).
    #[inline]
    pub fn is_string(&self) -> bool {
        self.tag == TAG_STRING
    }

    /// Check if this is a cons cell.
    #[inline]
    pub fn is_cons(&self) -> bool {
        self.tag == TAG_CONS
    }

    /// Check if this is a mutable @array.
    #[inline]
    pub fn is_array_mut(&self) -> bool {
        self.tag == TAG_ARRAY_MUT
    }

    /// Check if this is a mutable @struct.
    #[inline]
    pub fn is_struct_mut(&self) -> bool {
        self.tag == TAG_STRUCT_MUT
    }

    /// Check if this is an immutable struct.
    #[inline]
    pub fn is_struct(&self) -> bool {
        self.tag == TAG_STRUCT
    }

    /// Check if this is a closure.
    #[inline]
    pub fn is_closure(&self) -> bool {
        self.tag == TAG_CLOSURE
    }

    /// Check if this is a user box (LBox).
    #[inline]
    pub fn is_lbox(&self) -> bool {
        self.tag == TAG_LBOX
    }

    /// Check if this is a compiler capture cell (CaptureCell).
    #[inline]
    pub fn is_capture_cell(&self) -> bool {
        self.tag == TAG_CAPTURE_CELL
    }

    /// Check if this is a fiber.
    #[inline]
    pub fn is_fiber(&self) -> bool {
        self.tag == TAG_FIBER
    }

    /// Check if this is an @string.
    #[inline]
    pub fn is_string_mut(&self) -> bool {
        self.tag == TAG_STRING_MUT
    }

    /// Check if this is a bytes value.
    #[inline]
    pub fn is_bytes(&self) -> bool {
        self.tag == TAG_BYTES
    }

    /// Check if this is an @bytes value.
    #[inline]
    pub fn is_bytes_mut(&self) -> bool {
        self.tag == TAG_BYTES_MUT
    }

    /// Check if this is a syntax object.
    #[inline]
    pub fn is_syntax(&self) -> bool {
        self.tag == TAG_SYNTAX
    }

    /// Check if this is a native function.
    #[inline]
    pub fn is_native_fn(&self) -> bool {
        self.tag == TAG_NATIVE_FN
    }

    /// Check if this is an immutable array.
    #[inline]
    pub fn is_array(&self) -> bool {
        self.tag == TAG_ARRAY
    }

    /// Check if this is an immutable set.
    #[inline]
    pub fn is_set(&self) -> bool {
        self.tag == TAG_SET
    }

    /// Check if this is a mutable set.
    #[inline]
    pub fn is_set_mut(&self) -> bool {
        self.tag == TAG_SET_MUT
    }

    /// Check if this is a parameter.
    #[inline]
    pub fn is_parameter(&self) -> bool {
        self.tag == TAG_PARAMETER
    }

    /// Check if this is a managed pointer.
    #[inline]
    pub fn is_managed_pointer(&self) -> bool {
        self.tag == TAG_MANAGED_PTR
    }

    /// Check if this is an external object.
    #[inline]
    pub fn is_external(&self) -> bool {
        self.tag == TAG_EXTERNAL
    }

    /// Check if this is a thread handle.
    #[inline]
    pub fn is_thread(&self) -> bool {
        self.tag == TAG_THREAD
    }

    /// Check if this is a library handle.
    #[inline]
    pub fn is_lib_handle(&self) -> bool {
        self.tag == TAG_LIB_HANDLE
    }

    /// Check if this is an FFI signature.
    #[inline]
    pub fn is_ffi_sig(&self) -> bool {
        self.tag == TAG_FFI_SIG
    }

    /// Check if this is an FFI type descriptor.
    #[inline]
    pub fn is_ffi_type(&self) -> bool {
        self.tag == TAG_FFI_TYPE
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

    /// Access string contents via closure. Works for heap strings.
    /// Returns None if this is not a string.
    #[inline]
    pub fn with_string<R>(&self, f: impl FnOnce(&str) -> R) -> Option<R> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LString { s, .. } => {
                // SAFETY: LString's bytes are always valid UTF-8 (enforced by
                // constructors). The arena outlives the borrow.
                let str_ref = unsafe { std::str::from_utf8_unchecked(s.as_slice()) };
                Some(f(str_ref))
            }
            _ => None,
        }
    }

    /// Compare two string values lexicographically.
    /// Returns None if either value is not a string.
    pub fn compare_str(&self, other: &Value) -> Option<std::cmp::Ordering> {
        self.with_string(|sa| other.with_string(|sb| sa.cmp(sb)))
            .flatten()
    }

    /// Compare two keyword values lexicographically by name.
    /// Returns None if either value is not a keyword.
    pub fn compare_keyword(&self, other: &Value) -> Option<std::cmp::Ordering> {
        match (self.as_keyword_name(), other.as_keyword_name()) {
            (Some(a), Some(b)) => Some(a.cmp(&b)),
            _ => None,
        }
    }

    /// Extract as cons if this is a cons cell.
    #[inline]
    pub fn as_cons(&self) -> Option<&crate::value::heap::Cons> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_cons() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Cons(c) => Some(c),
            _ => None,
        }
    }

    /// Extract as mutable array if this is one.
    #[inline]
    pub fn as_array_mut(&self) -> Option<&std::cell::RefCell<Vec<Value>>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_array_mut() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LArrayMut { data, .. } => Some(data),
            _ => None,
        }
    }

    /// Extract as @struct if this is an @struct.
    #[inline]
    pub fn as_struct_mut(
        &self,
    ) -> Option<&std::cell::RefCell<std::collections::BTreeMap<crate::value::heap::TableKey, Value>>>
    {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_struct_mut() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LStructMut { data, .. } => Some(data),
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
    /// Returns a borrow of the arena-resident `Closure`. If you need an
    /// owned `Rc<Closure>` (e.g. for storing in a `Fiber` or `Frame`),
    /// clone explicitly: `Rc::new(value.as_closure().unwrap().clone())`.
    /// `Closure::clone` is O(1) — every non-Copy field is `Rc`-shared.
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

    /// Extract as box (LBox) if this is a user box.
    #[inline]
    pub fn as_lbox(&self) -> Option<&std::cell::RefCell<Value>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_lbox() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LBox { cell, .. } => Some(cell),
            _ => None,
        }
    }

    /// Extract as capture cell if this is a compiler capture cell.
    #[inline]
    pub fn as_capture_cell(&self) -> Option<&std::cell::RefCell<Value>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_capture_cell() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::CaptureCell { cell, .. } => Some(cell),
            _ => None,
        }
    }

    /// Extract the RefCell from either a user box or a capture cell.
    #[inline]
    pub fn as_box_or_capture(&self) -> Option<&std::cell::RefCell<Value>> {
        self.as_lbox().or_else(|| self.as_capture_cell())
    }

    /// Extract the primitive definition if this is a native function.
    #[inline]
    pub fn as_native_def(&self) -> Option<&'static crate::primitives::def::PrimitiveDef> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_native_fn() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::NativeFn(def) => Some(*def),
            _ => None,
        }
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

    /// Extract as mutable set if this is a mutable set.
    #[inline]
    pub fn as_set_mut(&self) -> Option<&std::cell::RefCell<std::collections::BTreeSet<Value>>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_set_mut() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LSetMut { data, .. } => Some(data),
            _ => None,
        }
    }

    /// Extract as @string if this is an @string.
    #[inline]
    pub fn as_string_mut(&self) -> Option<&std::cell::RefCell<Vec<u8>>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_string_mut() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LStringMut { data, .. } => Some(data),
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

    /// Extract as @bytes if this is an @bytes value.
    #[inline]
    pub fn as_bytes_mut(&self) -> Option<&std::cell::RefCell<Vec<u8>>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_bytes_mut() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LBytesMut { data, .. } => Some(data),
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
    pub fn as_syntax(&self) -> Option<&std::rc::Rc<crate::syntax::Syntax>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_syntax() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Syntax { syntax, .. } => Some(syntax),
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
        } else if self.is_heap() {
            unsafe { deref(*self).type_name() }
        } else {
            "unknown"
        }
    }

    /// Check if this value is mutable (can be modified in-place).
    #[inline]
    pub fn is_mutable(&self) -> bool {
        self.is_array_mut()
            || self.is_string_mut()
            || self.is_bytes_mut()
            || self.is_struct_mut()
            || self.is_set_mut()
            || self.is_lbox()
            || self.is_capture_cell()
            || self.is_parameter()
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
            HeapObject::FFISignature(sig, _) => Some(sig),
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

    /// Get or prepare the cached CIF for an FFI signature.
    /// Returns None if this is not an FFI signature.
    ///
    /// The CIF is lazily prepared on first access and cached for reuse.
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
            // Syntax-wrapped nil or empty list (e.g. from letrec in macros)
            if let Some(syntax) = current.as_syntax() {
                match &syntax.kind {
                    crate::syntax::SyntaxKind::Nil => return Ok(result),
                    crate::syntax::SyntaxKind::List(items) if items.is_empty() => {
                        return Ok(result)
                    }
                    _ => {}
                }
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
