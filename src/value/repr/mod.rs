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
//! Int:       0x7FF8_XXXX_XXXX_XXXX where X = 48-bit signed integer (sign-extended)
//! Falsy:     0x7FF9 — Nil = 0x7FF9_0000_0000_0000, False = 0x7FF9_0000_0000_0001
//! EmptyList: 0x7FFA_0000_0000_0000 (no payload)
//! Pointer:   0x7FFB_XXXX_XXXX_XXXX where X = 48-bit heap pointer
//! Truthy:    0x7FFC — bit 47=0: singletons (True=0, Undefined=1), bit 47=1: symbol (32-bit ID)
//! NaN/Inf:   0x7FFD_XXXX_XXXX_XXXX where X = 64-bit float bits (NaN or Infinity)
//! PtrVal:    0x7FFE — bit 47=0: keyword (47-bit ptr), bit 47=1: cpointer (47-bit ptr)
//! SSO:       0x7FFF (reserved for short string optimization)

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

/// Integer tag - uses QNAN exactly (0x7FF8), payload is 48-bit signed int
pub const TAG_INT: u64 = 0x7FF8_0000_0000_0000;
pub(crate) const TAG_INT_MASK: u64 = 0xFFFF_0000_0000_0000;

/// Falsy tag - upper 16 bits = 0x7FF9
/// Nil = TAG_FALSY | 0, False = TAG_FALSY | 1
pub const TAG_FALSY: u64 = 0x7FF9_0000_0000_0000;
#[allow(dead_code)] // used by SSO section
pub(crate) const TAG_FALSY_MASK: u64 = 0xFFFF_0000_0000_0000;
pub const TAG_NIL: u64 = 0x7FF9_0000_0000_0000;
pub const TAG_FALSE: u64 = 0x7FF9_0000_0000_0001;

/// Empty list tag - upper 16 bits = 0x7FFA
pub const TAG_EMPTY_LIST: u64 = 0x7FFA_0000_0000_0000;
#[allow(dead_code)] // reserved for future use
pub(crate) const TAG_EMPTY_LIST_MASK: u64 = 0xFFFF_0000_0000_0000;

/// Heap pointer tag - upper 16 bits = 0x7FFB
pub const TAG_POINTER: u64 = 0x7FFB_0000_0000_0000;
pub(crate) const TAG_POINTER_MASK: u64 = 0xFFFF_0000_0000_0000;

/// Truthy + symbol tag - upper 16 bits = 0x7FFC
/// Bit 47 = 0: singleton (payload 0=true, 1=undefined)
/// Bit 47 = 1: symbol (bits 0-31 = symbol ID)
pub const TAG_TRUTHY: u64 = 0x7FFC_0000_0000_0000;
pub(crate) const TAG_TRUTHY_MASK: u64 = 0xFFFF_0000_0000_0000;
pub const TAG_TRUE: u64 = 0x7FFC_0000_0000_0000;
pub const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
pub(crate) const TRUTHY_SYMBOL_BIT: u64 = 1u64 << 47; // bit 47 = symbol sub-tag
pub(crate) const SYMBOL_ID_MASK: u64 = 0xFFFF_FFFF; // bits 0-31

/// NaN/Infinity tag - upper 16 bits = 0x7FFD
pub const TAG_NAN: u64 = 0x7FFD_0000_0000_0000;
pub(crate) const TAG_NAN_MASK: u64 = 0xFFFF_0000_0000_0000;

/// Pointer values tag - upper 16 bits = 0x7FFE
/// Bit 47 = 0: keyword (bits 0-46 = interned string pointer)
/// Bit 47 = 1: cpointer (bits 0-46 = raw C pointer address)
pub const TAG_PTRVAL: u64 = 0x7FFE_0000_0000_0000;
pub(crate) const TAG_PTRVAL_MASK: u64 = 0xFFFF_0000_0000_0000;
pub(crate) const PTRVAL_CPOINTER_BIT: u64 = 1u64 << 47; // bit 47 = cpointer sub-tag
pub(crate) const PTRVAL_PAYLOAD_MASK: u64 = (1u64 << 47) - 1; // bits 0-46

/// SSO (Short String Optimization) tag - upper 16 bits = 0x7FFF
/// Payload: up to 6 UTF-8 bytes packed into bits 0-47, zero-padded
pub const TAG_SSO: u64 = 0x7FFF_0000_0000_0000;
#[allow(dead_code)] // used by SSO section
pub(crate) const TAG_SSO_MASK: u64 = 0xFFFF_0000_0000_0000;

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
///
/// # repr(transparent) invariant
/// JIT dispatch (`jit/dispatch.rs`, `vm/call.rs`) casts between `*const Value`
/// and `*const u64` without copying. Changing this repr breaks those casts.
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
        (self.0 & TAG_TRUTHY_MASK) == TAG_TRUTHY && (self.0 & TRUTHY_SYMBOL_BIT) != 0
    }

    /// Check if this is a keyword.
    #[inline]
    pub fn is_keyword(&self) -> bool {
        (self.0 & TAG_PTRVAL_MASK) == TAG_PTRVAL && (self.0 & PTRVAL_CPOINTER_BIT) == 0
    }

    /// Check if this is a raw C pointer.
    #[inline]
    pub fn is_pointer(&self) -> bool {
        (self.0 & TAG_PTRVAL_MASK) == TAG_PTRVAL && (self.0 & PTRVAL_CPOINTER_BIT) != 0
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
        (self.0 >> 48) != 0x7FF9
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
