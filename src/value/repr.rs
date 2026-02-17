//! NaN-boxing representation
//!
//! IEEE 754 double-precision: 1 sign + 11 exponent + 52 mantissa = 64 bits
//!
//! A quiet NaN has: exponent = all 1s (0x7FF), mantissa bit 51 = 1
//! This gives us the quiet NaN prefix: 0x7FF8 in the upper 16 bits
//!
//! Our encoding uses upper 16 bits as type tags, lower 48 bits as payload:
//!
//! Floats:    Any f64 that is NOT a quiet NaN (upper 13 bits != 0x7FF8+)
//! Nil:       0x7FFC_0000_0000_0000 (no payload)
//! False:     0x7FFC_0000_0000_0001
//! True:      0x7FFC_0000_0000_0002
//! EmptyList: 0x7FFC_0000_0000_0003 (no payload)
//! Int:       0x7FF8_XXXX_XXXX_XXXX where X = 48-bit signed integer (sign-extended)
//! Symbol:    0x7FF9_0000_XXXX_XXXX where X = 32-bit symbol ID
//! Keyword:   0x7FFA_0000_XXXX_XXXX where X = 32-bit symbol ID  
//! Pointer:   0x7FFB_XXXX_XXXX_XXXX where X = 48-bit heap pointer
//! NaN/Inf:   0x7FFD_XXXX_XXXX_XXXX where X = 64-bit float bits (NaN or Infinity)

// =============================================================================
// Tag Constants
// =============================================================================

/// Quiet NaN base - all tagged values have this prefix in upper 13 bits
const QNAN: u64 = 0x7FF8_0000_0000_0000;

/// Mask to check for quiet NaN (upper 13 bits)
const QNAN_MASK: u64 = 0xFFF8_0000_0000_0000;

/// Nil value - uses QNAN + 4 in upper 16 bits, no payload needed
pub const TAG_NIL: u64 = 0x7FFC_0000_0000_0000;

/// False value  
pub const TAG_FALSE: u64 = 0x7FFC_0000_0000_0001;

/// True value
pub const TAG_TRUE: u64 = 0x7FFC_0000_0000_0002;

/// Empty list value - uses QNAN + 4 in upper 16 bits, no payload needed
pub const TAG_EMPTY_LIST: u64 = 0x7FFC_0000_0000_0003;

/// Integer tag - uses QNAN exactly (0x7FF8), payload is 48-bit signed int
pub const TAG_INT: u64 = 0x7FF8_0000_0000_0000;
const TAG_INT_MASK: u64 = 0xFFFF_0000_0000_0000;

/// Symbol tag - upper 16 bits = 0x7FF9
pub const TAG_SYMBOL: u64 = 0x7FF9_0000_0000_0000;
const TAG_SYMBOL_MASK: u64 = 0xFFFF_0000_0000_0000;

/// Keyword tag - upper 16 bits = 0x7FFA  
pub const TAG_KEYWORD: u64 = 0x7FFA_0000_0000_0000;
const TAG_KEYWORD_MASK: u64 = 0xFFFF_0000_0000_0000;

/// Heap pointer tag - upper 16 bits = 0x7FFB
pub const TAG_POINTER: u64 = 0x7FFB_0000_0000_0000;
const TAG_POINTER_MASK: u64 = 0xFFFF_0000_0000_0000;

/// NaN/Infinity tag - upper 16 bits = 0x7FFD, payload is 64-bit float bits
pub const TAG_NAN: u64 = 0x7FFD_0000_0000_0000;
const TAG_NAN_MASK: u64 = 0xFFFF_0000_0000_0000;

/// Mask for 48-bit payload extraction
const PAYLOAD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// Maximum 48-bit signed integer (2^47 - 1)
pub const INT_MAX: i64 = 0x7FFF_FFFF_FFFF;

/// Minimum 48-bit signed integer (-2^47)
pub const INT_MIN: i64 = -0x8000_0000_0000;

// =============================================================================
// Value Struct
// =============================================================================

/// Core value type using NaN-boxing.
///
/// This is exactly 8 bytes and implements Copy.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Value(u64);

// Compile-time size assertion
const _: () = assert!(std::mem::size_of::<Value>() == 8);

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        use crate::value::heap::{deref, HeapObject};

        // For immediate values, compare bits directly
        if !self.is_heap() && !other.is_heap() {
            return self.0 == other.0;
        }

        // If one is heap and the other isn't, they're not equal
        if self.is_heap() != other.is_heap() {
            return false;
        }

        // Both are heap values - dereference and compare contents
        unsafe {
            let self_obj = deref(*self);
            let other_obj = deref(*other);

            match (self_obj, other_obj) {
                // String comparison
                (HeapObject::String(s1), HeapObject::String(s2)) => s1 == s2,

                // Cons cell comparison
                (HeapObject::Cons(c1), HeapObject::Cons(c2)) => c1 == c2,

                // Vector comparison (compare contents)
                (HeapObject::Vector(v1), HeapObject::Vector(v2)) => {
                    v1.borrow().as_slice() == v2.borrow().as_slice()
                }

                // Table comparison (compare contents)
                (HeapObject::Table(t1), HeapObject::Table(t2)) => *t1.borrow() == *t2.borrow(),

                // Struct comparison (compare contents)
                (HeapObject::Struct(s1), HeapObject::Struct(s2)) => s1 == s2,

                // Closure comparison (compare by reference)
                (HeapObject::Closure(c1), HeapObject::Closure(c2)) => std::rc::Rc::ptr_eq(c1, c2),

                // JitClosure comparison (compare by reference)
                (HeapObject::JitClosure(c1), HeapObject::JitClosure(c2)) => {
                    std::rc::Rc::ptr_eq(c1, c2)
                }

                // Condition comparison
                (HeapObject::Condition(cond1), HeapObject::Condition(cond2)) => cond1 == cond2,

                // Coroutine comparison (compare by reference)
                (HeapObject::Coroutine(co1), HeapObject::Coroutine(co2)) => {
                    // Coroutines are compared by reference since they have mutable state
                    std::ptr::eq(co1 as *const _, co2 as *const _)
                }

                // Cell comparison (compare contents)
                (HeapObject::Cell(c1, _), HeapObject::Cell(c2, _)) => *c1.borrow() == *c2.borrow(),

                // Float comparison
                (HeapObject::Float(f1), HeapObject::Float(f2)) => f1 == f2,

                // NativeFn comparison (compare by reference)
                (HeapObject::NativeFn(_), HeapObject::NativeFn(_)) => {
                    // Function pointers are compared by reference (pointer equality)
                    // Since they're stored in an Rc, we compare the Rc pointers
                    std::ptr::eq(self_obj as *const _, other_obj as *const _)
                }

                // VmAwareFn comparison (compare by reference)
                (HeapObject::VmAwareFn(_), HeapObject::VmAwareFn(_)) => {
                    // Function pointers are compared by reference (pointer equality)
                    // Since they're stored in an Rc, we compare the Rc pointers
                    std::ptr::eq(self_obj as *const _, other_obj as *const _)
                }

                // LibHandle comparison
                (HeapObject::LibHandle(h1), HeapObject::LibHandle(h2)) => h1 == h2,

                // CHandle comparison
                (HeapObject::CHandle(p1, h1), HeapObject::CHandle(p2, h2)) => p1 == p2 && h1 == h2,

                // ThreadHandle comparison (compare by reference)
                (HeapObject::ThreadHandle(_), HeapObject::ThreadHandle(_)) => {
                    std::ptr::eq(self_obj as *const _, other_obj as *const _)
                }

                // Different types are not equal
                _ => false,
            }
        }
    }
}

impl Value {
    // =========================================================================
    // Constants
    // =========================================================================

    pub const NIL: Value = Value(TAG_NIL);
    pub const TRUE: Value = Value(TAG_TRUE);
    pub const FALSE: Value = Value(TAG_FALSE);
    pub const EMPTY_LIST: Value = Value(TAG_EMPTY_LIST);

    // =========================================================================
    // Constructors
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
    // Type Predicates
    // =========================================================================

    /// Check if this is the nil value.
    #[inline]
    pub fn is_nil(&self) -> bool {
        self.0 == TAG_NIL
    }

    /// Check if this is an empty list.
    #[inline]
    pub fn is_empty_list(&self) -> bool {
        self.0 == TAG_EMPTY_LIST
    }

    /// Check if this is a boolean (true or false).
    #[inline]
    pub fn is_bool(&self) -> bool {
        self.0 == TAG_TRUE || self.0 == TAG_FALSE
    }

    /// Check if this is an integer.
    #[inline]
    pub fn is_int(&self) -> bool {
        (self.0 & TAG_INT_MASK) == TAG_INT
    }

    /// Check if this is a float (not a tagged value).
    /// This includes NaN and Infinity values.
    #[inline]
    pub fn is_float(&self) -> bool {
        // Float if NOT in the quiet NaN range, OR if it's our special NaN tag
        let tag = self.0 & QNAN_MASK;
        tag != QNAN || (self.0 & TAG_NAN_MASK) == TAG_NAN
    }

    /// Check if this is a number (int or float).
    #[inline]
    pub fn is_number(&self) -> bool {
        self.is_int() || self.is_float()
    }

    /// Check if this is a symbol.
    #[inline]
    pub fn is_symbol(&self) -> bool {
        (self.0 & TAG_SYMBOL_MASK) == TAG_SYMBOL
    }

    /// Check if this is a keyword.
    #[inline]
    pub fn is_keyword(&self) -> bool {
        (self.0 & TAG_KEYWORD_MASK) == TAG_KEYWORD
    }

    /// Check if this is a heap pointer.
    #[inline]
    pub fn is_heap(&self) -> bool {
        (self.0 & TAG_POINTER_MASK) == TAG_POINTER
    }

    /// Check if this value is truthy (everything except nil and false).
    #[inline]
    pub fn is_truthy(&self) -> bool {
        self.0 != TAG_FALSE && self.0 != TAG_NIL
    }

    // =========================================================================
    // Extractors
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
            Some((self.0 & PAYLOAD_MASK) as u32)
        } else {
            None
        }
    }

    /// Extract keyword ID if this is a keyword.
    #[inline]
    pub fn as_keyword(&self) -> Option<u32> {
        if self.is_keyword() {
            Some((self.0 & PAYLOAD_MASK) as u32)
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

    /// Get the raw bits (for debugging/serialization).
    #[inline]
    pub fn to_bits(&self) -> u64 {
        self.0
    }

    /// Create from raw bits (for deserialization).
    ///
    /// # Safety
    /// The bits must represent a valid Value encoding.
    #[inline]
    pub unsafe fn from_bits(bits: u64) -> Self {
        Value(bits)
    }

    // =============================================================================
    // Heap Value Constructors
    // =============================================================================

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

    // =============================================================================
    // Heap Type Predicates
    // =============================================================================

    /// Check if this is a string.
    #[inline]
    pub fn is_string(&self) -> bool {
        use crate::value::heap::HeapTag;
        self.heap_tag() == Some(HeapTag::String)
    }

    /// Check if this is a cons cell.
    #[inline]
    pub fn is_cons(&self) -> bool {
        use crate::value::heap::HeapTag;
        self.heap_tag() == Some(HeapTag::Cons)
    }

    /// Check if this is a vector.
    #[inline]
    pub fn is_vector(&self) -> bool {
        use crate::value::heap::HeapTag;
        self.heap_tag() == Some(HeapTag::Vector)
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

    /// Check if this is a coroutine.
    #[inline]
    pub fn is_coroutine(&self) -> bool {
        use crate::value::heap::HeapTag;
        self.heap_tag() == Some(HeapTag::Coroutine)
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

    // =============================================================================
    // Heap Value Extractors
    // =============================================================================

    /// Extract as string if this is a string.
    #[inline]
    pub fn as_string(&self) -> Option<&str> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::String(s) => Some(s),
            _ => None,
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

    /// Extract as vector if this is a vector.
    #[inline]
    pub fn as_vector(&self) -> Option<&std::cell::RefCell<Vec<Value>>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Vector(v) => Some(v),
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

    /// Extract as VM-aware function if this is a VM-aware function.
    #[inline]
    pub fn as_vm_aware_fn(&self) -> Option<&crate::value::heap::VmAwareFn> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::VmAwareFn(f) => Some(f),
            _ => None,
        }
    }

    /// Extract as JIT closure if this is a JIT closure.
    #[inline]
    pub fn as_jit_closure(&self) -> Option<&std::rc::Rc<crate::value_old::JitClosure>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::JitClosure(c) => Some(c),
            _ => None,
        }
    }

    /// Extract as condition if this is a condition.
    #[inline]
    pub fn as_condition(&self) -> Option<&crate::value_old::Condition> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Condition(c) => Some(c),
            _ => None,
        }
    }

    /// Extract as coroutine if this is a coroutine.
    #[inline]
    pub fn as_coroutine(
        &self,
    ) -> Option<&std::rc::Rc<std::cell::RefCell<crate::value_old::Coroutine>>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Coroutine(c) => Some(c),
            _ => None,
        }
    }

    /// Extract as thread handle if this is a thread handle.
    #[inline]
    pub fn as_thread_handle(&self) -> Option<&crate::value::heap::ThreadHandleData> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::ThreadHandle(h) => Some(h),
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
        } else if self.is_heap() {
            unsafe { deref(*self).type_name() }
        } else {
            "unknown"
        }
    }

    /// Convert a proper list to a Vec.
    pub fn list_to_vec(&self) -> Result<Vec<Value>, &'static str> {
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

/// Create a proper list from values.
pub fn list(values: impl IntoIterator<Item = Value>) -> Value {
    values
        .into_iter()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .fold(Value::EMPTY_LIST, |acc, v| Value::cons(v, acc))
}

/// Create a cons cell (convenience function).
#[inline]
pub fn cons(car: Value, cdr: Value) -> Value {
    Value::cons(car, cdr)
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Delegate to Display implementation
        write!(f, "{}", self)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size() {
        assert_eq!(std::mem::size_of::<Value>(), 8);
    }

    #[test]
    fn test_nil() {
        let v = Value::NIL;
        assert!(v.is_nil());
        assert!(!v.is_bool());
        assert!(!v.is_int());
        assert!(!v.is_float());
        assert!(!v.is_truthy()); // nil is falsy
    }

    #[test]
    fn test_bool() {
        assert!(Value::TRUE.is_bool());
        assert!(Value::FALSE.is_bool());
        assert_eq!(Value::TRUE.as_bool(), Some(true));
        assert_eq!(Value::FALSE.as_bool(), Some(false));
        assert!(Value::TRUE.is_truthy());
        assert!(!Value::FALSE.is_truthy());
    }

    #[test]
    fn test_int_roundtrip() {
        for &n in &[0i64, 1, -1, 100, -100, INT_MAX, INT_MIN] {
            let v = Value::int(n);
            assert!(v.is_int());
            assert!(!v.is_float());
            assert_eq!(v.as_int(), Some(n), "Failed for {}", n);
        }
    }

    #[test]
    fn test_float_roundtrip() {
        for &f in &[
            0.0f64,
            1.0,
            -1.0,
            std::f64::consts::PI,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            let v = Value::float(f);
            assert!(v.is_float());
            assert!(!v.is_int());
            assert_eq!(v.as_float(), Some(f));
        }
    }

    #[test]
    fn test_symbol() {
        let v = Value::symbol(42);
        assert!(v.is_symbol());
        assert_eq!(v.as_symbol(), Some(42));
    }

    #[test]
    fn test_keyword() {
        let v = Value::keyword(123);
        assert!(v.is_keyword());
        assert_eq!(v.as_keyword(), Some(123));
    }

    #[test]
    fn test_bool_constructor() {
        assert_eq!(Value::bool(true), Value::TRUE);
        assert_eq!(Value::bool(false), Value::FALSE);
    }

    #[test]
    fn test_string_constructor() {
        let v = Value::string("hello");
        assert!(v.is_string());
        assert_eq!(v.as_string(), Some("hello"));
    }

    #[test]
    fn test_cons_constructor() {
        let car = Value::int(1);
        let cdr = Value::int(2);
        let v = Value::cons(car, cdr);
        assert!(v.is_cons());
        if let Some(cons) = v.as_cons() {
            assert_eq!(cons.first, car);
            assert_eq!(cons.rest, cdr);
        } else {
            panic!("Expected cons cell");
        }
    }

    #[test]
    fn test_vector_constructor() {
        let elements = vec![Value::int(1), Value::int(2), Value::int(3)];
        let v = Value::vector(elements.clone());
        assert!(v.is_vector());
        if let Some(vec_ref) = v.as_vector() {
            let borrowed = vec_ref.borrow();
            assert_eq!(borrowed.len(), 3);
            assert_eq!(borrowed[0], Value::int(1));
            assert_eq!(borrowed[1], Value::int(2));
            assert_eq!(borrowed[2], Value::int(3));
        } else {
            panic!("Expected vector");
        }
    }

    #[test]
    fn test_table_constructor() {
        let v = Value::table();
        assert!(v.is_table());
        if let Some(table_ref) = v.as_table() {
            let borrowed = table_ref.borrow();
            assert_eq!(borrowed.len(), 0);
        } else {
            panic!("Expected table");
        }
    }

    #[test]
    fn test_cell_constructor() {
        let inner = Value::int(42);
        let v = Value::cell(inner);
        assert!(v.is_cell());
        if let Some(cell_ref) = v.as_cell() {
            let borrowed = cell_ref.borrow();
            assert_eq!(*borrowed, Value::int(42));
        } else {
            panic!("Expected cell");
        }
    }

    #[test]
    fn test_list_function() {
        let values = vec![Value::int(1), Value::int(2), Value::int(3)];
        let list_val = list(values);
        assert!(list_val.is_list());

        // Convert back to vec
        let result = list_val.list_to_vec().unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], Value::int(1));
        assert_eq!(result[1], Value::int(2));
        assert_eq!(result[2], Value::int(3));
    }

    #[test]
    fn test_is_list() {
        // Proper list
        let proper_list = Value::cons(Value::int(1), Value::cons(Value::int(2), Value::NIL));
        assert!(proper_list.is_list());

        // Not a list (improper list)
        let improper_list = Value::cons(Value::int(1), Value::int(2));
        assert!(!improper_list.is_list());

        // Nil is a list
        assert!(Value::NIL.is_list());
    }

    #[test]
    fn test_type_name() {
        assert_eq!(Value::NIL.type_name(), "nil");
        assert_eq!(Value::TRUE.type_name(), "boolean");
        assert_eq!(Value::int(42).type_name(), "integer");
        assert_eq!(Value::float(std::f64::consts::PI).type_name(), "float");
        assert_eq!(Value::symbol(1).type_name(), "symbol");
        assert_eq!(Value::keyword(1).type_name(), "keyword");
        assert_eq!(Value::string("test").type_name(), "string");
        assert_eq!(
            Value::cons(Value::NIL, Value::EMPTY_LIST).type_name(),
            "cons"
        );
        assert_eq!(Value::vector(vec![]).type_name(), "vector");
        assert_eq!(Value::table().type_name(), "table");
        assert_eq!(Value::cell(Value::NIL).type_name(), "cell");
    }

    #[test]
    fn test_truthiness_semantics() {
        // Only nil and #f are falsy
        assert!(!Value::NIL.is_truthy(), "nil is falsy");
        assert!(!Value::FALSE.is_truthy(), "#f is falsy");

        // #t is truthy
        assert!(Value::TRUE.is_truthy(), "#t is truthy");

        // Zero is truthy (not falsy like in C)
        assert!(Value::int(0).is_truthy(), "0 is truthy");
        assert!(Value::float(0.0).is_truthy(), "0.0 is truthy");

        // Empty string is truthy
        assert!(Value::string("").is_truthy(), "empty string is truthy");

        // Empty list is truthy (it's nil, but we test the list form)
        assert!(Value::EMPTY_LIST.is_truthy(), "empty list is truthy");

        // Empty vector is truthy
        assert!(Value::vector(vec![]).is_truthy(), "empty vector is truthy");

        // Regular values are truthy
        assert!(Value::int(1).is_truthy(), "1 is truthy");
        assert!(Value::int(-1).is_truthy(), "-1 is truthy");
        assert!(
            Value::float(std::f64::consts::PI).is_truthy(),
            "PI is truthy"
        );
        assert!(
            Value::string("hello").is_truthy(),
            "non-empty string is truthy"
        );
        assert!(Value::symbol(1).is_truthy(), "symbol is truthy");
        assert!(Value::keyword(1).is_truthy(), "keyword is truthy");

        // Non-empty list is truthy
        let non_empty_list = Value::cons(Value::int(1), Value::NIL);
        assert!(non_empty_list.is_truthy(), "non-empty list is truthy");

        // Non-empty vector is truthy
        let non_empty_vec = Value::vector(vec![Value::int(1)]);
        assert!(non_empty_vec.is_truthy(), "non-empty vector is truthy");

        // Table is truthy
        assert!(Value::table().is_truthy(), "table is truthy");

        // Cell is truthy
        assert!(Value::cell(Value::int(42)).is_truthy(), "cell is truthy");
    }
}
