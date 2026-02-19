//! Value constructors for immediate and heap-allocated types.

use super::{
    Value, INT_MAX, INT_MIN, PAYLOAD_MASK, QNAN, QNAN_MASK, TAG_INT, TAG_KEYWORD, TAG_NAN,
    TAG_POINTER, TAG_SYMBOL,
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

    /// Create a keyword value from a SymbolId.
    #[inline]
    pub fn keyword(id: u32) -> Self {
        Value(TAG_KEYWORD | (id as u64))
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

    /// Create a vector.
    #[inline]
    pub fn vector(elements: Vec<Value>) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        alloc(HeapObject::Vector(RefCell::new(elements)))
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

    /// Create a coroutine value.
    #[inline]
    pub fn coroutine(c: crate::value::heap::Coroutine) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::cell::RefCell;
        use std::rc::Rc;
        alloc(HeapObject::Coroutine(Rc::new(RefCell::new(c))))
    }

    /// Create a native function value.
    #[inline]
    pub fn native_fn(f: crate::value::heap::NativeFn) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        alloc(HeapObject::NativeFn(f))
    }

    /// Create a VM-aware native function value.
    #[inline]
    pub fn vm_aware_fn(f: crate::value::heap::VmAwareFn) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        alloc(HeapObject::VmAwareFn(f))
    }

    /// Create a continuation value.
    #[inline]
    pub fn continuation(c: crate::value::continuation::ContinuationData) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        use std::rc::Rc;
        alloc(HeapObject::Continuation(Rc::new(c)))
    }
}
