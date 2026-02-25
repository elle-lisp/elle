//! Value constructors for immediate and heap-allocated types.

use super::{
    Value, INT_MAX, INT_MIN, PAYLOAD_MASK, QNAN, QNAN_MASK, TAG_CPOINTER, TAG_INT, TAG_KEYWORD,
    TAG_NAN, TAG_POINTER, TAG_SYMBOL,
};

impl Value {
    // =========================================================================
    // Immediate Value Constructors
    // =========================================================================

    /// Create an integer value.
    ///
    /// # Panics
    /// Panics if the integer is outside the 48-bit signed range.
    #[inline]
    pub fn int(n: i64) -> Self {
        debug_assert!(
            (INT_MIN..=INT_MAX).contains(&n),
            "Integer {} out of 48-bit range [{}, {}]",
            n,
            INT_MIN,
            INT_MAX
        );
        // Store as sign-extended 48 bits
        Value(TAG_INT | ((n as u64) & PAYLOAD_MASK))
    }

    /// Create a float value.
    ///
    /// NaN and Infinity values are stored with a special tag to avoid
    /// colliding with the quiet NaN tagging scheme.
    #[inline]
    pub fn float(f: f64) -> Self {
        let bits = f.to_bits();
        // Check if it's a quiet NaN or Infinity (would collide with our tags)
        if (bits & QNAN_MASK) == QNAN {
            // Store NaN/Infinity with special tag in upper 16 bits
            // For NaN/Infinity, the lower 48 bits are always zero, so we can
            // store the upper 16 bits in the payload
            let upper_16 = bits >> 48;
            Value(TAG_NAN | upper_16)
        } else {
            Value(bits)
        }
    }

    /// Create a symbol value from a SymbolId.
    #[inline]
    pub fn symbol(id: u32) -> Self {
        Value(TAG_SYMBOL | (id as u64))
    }

    /// Create a keyword value from a name string.
    /// The name is interned for O(1) equality and display.
    #[inline]
    pub fn keyword(name: &str) -> Self {
        use crate::value::intern::intern_string;
        let ptr = intern_string(name) as *const ();
        let addr = ptr as u64;
        debug_assert!(
            addr & !PAYLOAD_MASK == 0,
            "Keyword pointer exceeds 48-bit address space"
        );
        Value(TAG_KEYWORD | addr)
    }

    /// Create a boolean value.
    #[inline]
    pub fn bool(b: bool) -> Self {
        if b {
            Self::TRUE
        } else {
            Self::FALSE
        }
    }

    /// Create a raw C pointer value.
    ///
    /// NULL pointers (address 0) are represented as `Value::NIL`.
    /// This is an immediate value, not heap-allocated.
    #[inline]
    pub fn pointer(addr: usize) -> Self {
        if addr == 0 {
            return Self::NIL;
        }
        let addr_u64 = addr as u64;
        debug_assert!(
            addr_u64 & !PAYLOAD_MASK == 0,
            "C pointer exceeds 48-bit address space"
        );
        Value(TAG_CPOINTER | (addr_u64 & PAYLOAD_MASK))
    }

    /// Create an empty list value.
    #[inline]
    pub fn empty_list() -> Self {
        Self::EMPTY_LIST
    }

    /// Create a heap pointer value.
    ///
    /// # Safety
    /// The pointer must be valid and properly aligned. The caller is
    /// responsible for ensuring the pointed-to memory remains valid.
    #[inline]
    pub fn from_heap_ptr(ptr: *const ()) -> Self {
        let addr = ptr as u64;
        debug_assert!(
            addr & !PAYLOAD_MASK == 0,
            "Heap pointer exceeds 48-bit address space"
        );
        Value(TAG_POINTER | addr)
    }

    // =========================================================================
    // Heap Value Constructors
    // =========================================================================

    /// Create a string value.
    #[inline]
    pub fn string(s: impl Into<Box<str>>) -> Self {
        use crate::value::intern::intern_string;
        let boxed: Box<str> = s.into();
        let ptr = intern_string(&boxed) as *const ();
        Self::from_heap_ptr(ptr)
    }

    /// Create a cons cell.
    #[inline]
    pub fn cons(car: Value, cdr: Value) -> Self {
        use crate::value::heap::{alloc, Cons, HeapObject};
        alloc(HeapObject::Cons(Cons {
            first: car,
            rest: cdr,
        }))
    }

    /// Create an array.
    #[inline]
    pub fn array(elements: Vec<Value>) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        alloc(HeapObject::Array(RefCell::new(elements)))
    }

    /// Create an empty mutable table.
    #[inline]
    pub fn table() -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        use std::collections::BTreeMap;
        alloc(HeapObject::Table(RefCell::new(BTreeMap::new())))
    }

    /// Create a table with initial entries.
    #[inline]
    pub fn table_from(
        entries: std::collections::BTreeMap<crate::value::heap::TableKey, Value>,
    ) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        alloc(HeapObject::Table(RefCell::new(entries)))
    }

    /// Create an immutable struct.
    #[inline]
    pub fn struct_from(
        fields: std::collections::BTreeMap<crate::value::heap::TableKey, Value>,
    ) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        alloc(HeapObject::Struct(fields))
    }

    /// Create a closure.
    #[inline]
    pub fn closure(c: crate::value::heap::Closure) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::rc::Rc;
        alloc(HeapObject::Closure(Rc::new(c)))
    }

    /// Create a user cell (mutable box) — NOT auto-unwrapped by LoadUpvalue.
    #[inline]
    pub fn cell(value: Value) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        alloc(HeapObject::Cell(RefCell::new(value), false))
    }

    /// Create a compiler local cell — auto-unwrapped by LoadUpvalue.
    /// Used for mutable captured variables.
    #[inline]
    pub fn local_cell(value: Value) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        alloc(HeapObject::Cell(RefCell::new(value), true))
    }

    /// Create a native function value.
    #[inline]
    pub fn native_fn(f: crate::value::heap::NativeFn) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        alloc(HeapObject::NativeFn(f))
    }

    /// Create an immutable tuple value.
    #[inline]
    pub fn tuple(elements: Vec<Value>) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        alloc(HeapObject::Tuple(elements))
    }

    /// Create a fiber value.
    #[inline]
    pub fn fiber(f: crate::value::fiber::Fiber) -> Self {
        use crate::value::fiber::FiberHandle;
        use crate::value::heap::{alloc, HeapObject};
        alloc(HeapObject::Fiber(FiberHandle::new(f)))
    }

    /// Create a fiber value from an existing FiberHandle.
    #[inline]
    pub fn fiber_from_handle(handle: crate::value::fiber::FiberHandle) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        alloc(HeapObject::Fiber(handle))
    }

    /// Create a syntax object value.
    /// Preserves scope sets through the Value round-trip during macro expansion.
    #[inline]
    pub fn syntax(s: crate::syntax::Syntax) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::rc::Rc;
        alloc(HeapObject::Syntax(Rc::new(s)))
    }

    /// Create an FFI signature value.
    #[inline]
    pub fn ffi_signature(sig: crate::ffi::types::Signature) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        alloc(HeapObject::FFISignature(sig, RefCell::new(None)))
    }

    /// Create an FFI compound type descriptor value.
    ///
    /// Only for compound types (Struct, Array). Primitive types use keywords.
    #[inline]
    pub fn ffi_type(desc: crate::ffi::types::TypeDesc) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        debug_assert!(
            matches!(
                desc,
                crate::ffi::types::TypeDesc::Struct(_) | crate::ffi::types::TypeDesc::Array(_, _)
            ),
            "FFIType should only wrap compound types"
        );
        alloc(HeapObject::FFIType(desc))
    }

    /// Create a library handle value.
    #[inline]
    pub fn lib_handle(id: u32) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        alloc(HeapObject::LibHandle(id))
    }

    /// Create a binding value (compile-time only).
    #[inline]
    pub fn binding(
        name: crate::value::types::SymbolId,
        scope: crate::value::heap::BindingScope,
    ) -> Self {
        use crate::value::heap::{alloc, BindingInner, HeapObject};
        use std::cell::RefCell;
        alloc(HeapObject::Binding(RefCell::new(BindingInner {
            name,
            scope,
            is_mutated: false,
            is_captured: false,
            is_immutable: false,
        })))
    }
}
