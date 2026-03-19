//! 16-byte tagged-union Value representation.
//!
//! Every value is exactly 16 bytes:
//!   tag:     u64 — type discriminant (TAG_* constants below)
//!   payload: u64 — type-specific data:
//!                  integers: i64 reinterpreted as u64
//!                  floats:   f64::to_bits()
//!                  symbols:  u32 symbol ID
//!                  keywords: u64 hash from intern_keyword
//!                  cpointer: usize address
//!                  heap:     *const () pointer to HeapObject

mod accessors;
mod constructors;
mod traits;

#[cfg(test)]
mod tests;

// =============================================================================
// Tag Constants
// =============================================================================

pub const TAG_INT: u64 = 0;
pub const TAG_FLOAT: u64 = 1;
pub const TAG_NIL: u64 = 2;
pub const TAG_TRUE: u64 = 3;
pub const TAG_FALSE: u64 = 4;
pub const TAG_EMPTY_LIST: u64 = 5;
pub const TAG_SYMBOL: u64 = 6;
pub const TAG_KEYWORD: u64 = 7;
pub const TAG_UNDEFINED: u64 = 8;
pub const TAG_CPOINTER: u64 = 9;

// Heap types (tag >= TAG_HEAP_START means is_heap() is true)
pub const TAG_HEAP_START: u64 = 10;
pub const TAG_STRING: u64 = 10;
pub const TAG_STRING_MUT: u64 = 11;
pub const TAG_ARRAY: u64 = 12;
pub const TAG_ARRAY_MUT: u64 = 13;
pub const TAG_STRUCT: u64 = 14;
pub const TAG_STRUCT_MUT: u64 = 15;
pub const TAG_CONS: u64 = 16;
pub const TAG_CLOSURE: u64 = 17;
pub const TAG_BYTES: u64 = 18;
pub const TAG_BYTES_MUT: u64 = 19;
pub const TAG_SET: u64 = 20;
pub const TAG_SET_MUT: u64 = 21;
pub const TAG_LBOX: u64 = 22;
pub const TAG_FIBER: u64 = 23;
pub const TAG_SYNTAX: u64 = 24;
pub const TAG_NATIVE_FN: u64 = 26;
pub const TAG_FFI_SIG: u64 = 27;
pub const TAG_FFI_TYPE: u64 = 28;
pub const TAG_LIB_HANDLE: u64 = 29;
pub const TAG_MANAGED_PTR: u64 = 30;
pub const TAG_EXTERNAL: u64 = 31;
pub const TAG_PARAMETER: u64 = 32;
pub const TAG_THREAD: u64 = 33;

// =============================================================================
// Value Struct
// =============================================================================

/// Core value type using a 16-byte tagged union.
///
/// This is exactly 16 bytes and implements Copy.
///
/// `tag` is one of the TAG_* constants above.
/// `payload` interpretation depends on `tag` — see module-level docs.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Value {
    pub(crate) tag: u64,
    pub(crate) payload: u64,
}

// Compile-time size assertion
const _: () = assert!(std::mem::size_of::<Value>() == 16);

impl Value {
    // =========================================================================
    // Constants
    // =========================================================================

    pub const NIL: Value = Value {
        tag: TAG_NIL,
        payload: 0,
    };
    pub const TRUE: Value = Value {
        tag: TAG_TRUE,
        payload: 0,
    };
    pub const FALSE: Value = Value {
        tag: TAG_FALSE,
        payload: 0,
    };
    pub const EMPTY_LIST: Value = Value {
        tag: TAG_EMPTY_LIST,
        payload: 0,
    };
    pub const UNDEFINED: Value = Value {
        tag: TAG_UNDEFINED,
        payload: 0,
    };

    // =========================================================================
    // Type Predicates (non-heap immediates)
    // =========================================================================

    /// Check if this is the nil value.
    #[inline]
    pub fn is_nil(&self) -> bool {
        self.tag == TAG_NIL
    }

    /// Check if this is an empty list.
    #[inline]
    pub fn is_empty_list(&self) -> bool {
        self.tag == TAG_EMPTY_LIST
    }

    /// Check if this is the undefined sentinel value.
    #[inline]
    pub fn is_undefined(&self) -> bool {
        self.tag == TAG_UNDEFINED
    }

    /// Check if this is a boolean (true or false).
    #[inline]
    pub fn is_bool(&self) -> bool {
        self.tag == TAG_TRUE || self.tag == TAG_FALSE
    }

    /// Check if this is an integer.
    #[inline]
    pub fn is_int(&self) -> bool {
        self.tag == TAG_INT
    }

    /// Check if this is a float.
    #[inline]
    pub fn is_float(&self) -> bool {
        self.tag == TAG_FLOAT
    }

    /// Check if this is a number (int or float).
    #[inline]
    pub fn is_number(&self) -> bool {
        self.is_int() || self.is_float()
    }

    /// Check if this is a symbol.
    #[inline]
    pub fn is_symbol(&self) -> bool {
        self.tag == TAG_SYMBOL
    }

    /// Check if this is a keyword.
    #[inline]
    pub fn is_keyword(&self) -> bool {
        self.tag == TAG_KEYWORD
    }

    /// Check if this is a raw C pointer.
    #[inline]
    pub fn is_pointer(&self) -> bool {
        self.tag == TAG_CPOINTER
    }

    /// Check if this is a heap pointer.
    #[inline]
    pub fn is_heap(&self) -> bool {
        self.tag >= TAG_HEAP_START
    }

    /// Check if this value is truthy (everything except nil and false).
    /// UNDEFINED should never appear in user-visible evaluation - debug_assert catches leaks.
    #[inline]
    pub fn is_truthy(&self) -> bool {
        debug_assert!(
            !self.is_undefined(),
            "UNDEFINED leaked into truthiness check"
        );
        self.tag != TAG_NIL && self.tag != TAG_FALSE
    }

    /// Create a heap pointer value from a raw pointer and an explicit tag.
    ///
    /// # Safety
    /// The pointer must be valid, properly aligned, and point to a HeapObject
    /// of the type indicated by `tag`. The caller is responsible for ensuring
    /// the pointed-to memory remains valid.
    #[inline]
    pub fn from_heap_ptr(ptr: *const (), tag: u64) -> Self {
        Value {
            tag,
            payload: ptr as u64,
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
