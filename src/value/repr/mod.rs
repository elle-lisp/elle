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

mod accessors;
mod constructors;
mod traits;

#[cfg(test)]
mod tests;

// =============================================================================
// Tag Constants
// =============================================================================

/// Quiet NaN base - all tagged values have this prefix in upper 13 bits
pub(crate) const QNAN: u64 = 0x7FF8_0000_0000_0000;

/// Mask to check for quiet NaN (upper 13 bits)
pub(crate) const QNAN_MASK: u64 = 0xFFF8_0000_0000_0000;

/// Nil value - uses QNAN + 4 in upper 16 bits, no payload needed
pub const TAG_NIL: u64 = 0x7FFC_0000_0000_0000;

/// False value  
pub const TAG_FALSE: u64 = 0x7FFC_0000_0000_0001;

/// True value
pub const TAG_TRUE: u64 = 0x7FFC_0000_0000_0002;

/// Empty list value - uses QNAN + 4 in upper 16 bits, no payload needed
pub const TAG_EMPTY_LIST: u64 = 0x7FFC_0000_0000_0003;

/// Undefined value - sentinel for uninitialized global slots
pub const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0004;

/// Integer tag - uses QNAN exactly (0x7FF8), payload is 48-bit signed int
pub const TAG_INT: u64 = 0x7FF8_0000_0000_0000;
pub(crate) const TAG_INT_MASK: u64 = 0xFFFF_0000_0000_0000;

/// Symbol tag - upper 16 bits = 0x7FF9
pub const TAG_SYMBOL: u64 = 0x7FF9_0000_0000_0000;
pub(crate) const TAG_SYMBOL_MASK: u64 = 0xFFFF_0000_0000_0000;

/// Keyword tag - upper 16 bits = 0x7FFA  
pub const TAG_KEYWORD: u64 = 0x7FFA_0000_0000_0000;
pub(crate) const TAG_KEYWORD_MASK: u64 = 0xFFFF_0000_0000_0000;

/// Heap pointer tag - upper 16 bits = 0x7FFB
pub const TAG_POINTER: u64 = 0x7FFB_0000_0000_0000;
pub(crate) const TAG_POINTER_MASK: u64 = 0xFFFF_0000_0000_0000;

/// NaN/Infinity tag - upper 16 bits = 0x7FFD, payload is 64-bit float bits
pub const TAG_NAN: u64 = 0x7FFD_0000_0000_0000;
pub(crate) const TAG_NAN_MASK: u64 = 0xFFFF_0000_0000_0000;

/// Mask for 48-bit payload extraction
pub(crate) const PAYLOAD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

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
pub struct Value(pub(crate) u64);

// Compile-time size assertion
const _: () = assert!(std::mem::size_of::<Value>() == 8);

impl Value {
    // =========================================================================
    // Constants
    // =========================================================================

    pub const NIL: Value = Value(TAG_NIL);
    pub const TRUE: Value = Value(TAG_TRUE);
    pub const FALSE: Value = Value(TAG_FALSE);
    pub const EMPTY_LIST: Value = Value(TAG_EMPTY_LIST);
    pub const UNDEFINED: Value = Value(TAG_UNDEFINED);

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

    /// Check if this is the undefined sentinel value.
    #[inline]
    pub fn is_undefined(&self) -> bool {
        self.0 == TAG_UNDEFINED
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
    /// UNDEFINED should never appear in user-visible evaluation - debug_assert catches leaks.
    #[inline]
    pub fn is_truthy(&self) -> bool {
        debug_assert!(
            !self.is_undefined(),
            "UNDEFINED leaked into truthiness check"
        );
        self.0 != TAG_FALSE && self.0 != TAG_NIL
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
