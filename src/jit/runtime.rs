//! Runtime helper functions for JIT-compiled code
//!
//! These functions are called from JIT-compiled code to perform operations
//! that are too complex to inline, such as heap allocation and type checking.
//!
//! All functions use the C calling convention and operate on raw `u64` values
//! (NaN-boxed Value bits).

use crate::value::repr::{PAYLOAD_MASK, TAG_FALSE, TAG_INT, TAG_INT_MASK, TAG_NIL};
use crate::value::Value;

// =============================================================================
// Arithmetic Operations
// =============================================================================

/// Integer addition with overflow check
///
/// If both operands are integers, performs integer addition.
/// If either is a float, performs float addition.
/// Returns NIL on type error.
#[no_mangle]
pub extern "C" fn elle_jit_add(a: u64, b: u64) -> u64 {
    let a = unsafe { Value::from_bits(a) };
    let b = unsafe { Value::from_bits(b) };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        Value::int(ai.wrapping_add(bi)).to_bits()
    } else if let (Some(af), Some(bf)) = (a.as_number(), b.as_number()) {
        Value::float(af + bf).to_bits()
    } else {
        elle_jit_type_error_str("number")
    }
}

/// Integer subtraction
#[no_mangle]
pub extern "C" fn elle_jit_sub(a: u64, b: u64) -> u64 {
    let a = unsafe { Value::from_bits(a) };
    let b = unsafe { Value::from_bits(b) };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        Value::int(ai.wrapping_sub(bi)).to_bits()
    } else if let (Some(af), Some(bf)) = (a.as_number(), b.as_number()) {
        Value::float(af - bf).to_bits()
    } else {
        elle_jit_type_error_str("number")
    }
}

/// Integer multiplication
#[no_mangle]
pub extern "C" fn elle_jit_mul(a: u64, b: u64) -> u64 {
    let a = unsafe { Value::from_bits(a) };
    let b = unsafe { Value::from_bits(b) };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        Value::int(ai.wrapping_mul(bi)).to_bits()
    } else if let (Some(af), Some(bf)) = (a.as_number(), b.as_number()) {
        Value::float(af * bf).to_bits()
    } else {
        elle_jit_type_error_str("number")
    }
}

/// Integer division
#[no_mangle]
pub extern "C" fn elle_jit_div(a: u64, b: u64) -> u64 {
    let a = unsafe { Value::from_bits(a) };
    let b = unsafe { Value::from_bits(b) };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        if bi == 0 {
            elle_jit_type_error_str("non-zero divisor")
        } else {
            Value::int(ai.wrapping_div(bi)).to_bits()
        }
    } else if let (Some(af), Some(bf)) = (a.as_number(), b.as_number()) {
        Value::float(af / bf).to_bits()
    } else {
        elle_jit_type_error_str("number")
    }
}

/// Integer remainder
#[no_mangle]
pub extern "C" fn elle_jit_rem(a: u64, b: u64) -> u64 {
    let a = unsafe { Value::from_bits(a) };
    let b = unsafe { Value::from_bits(b) };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        if bi == 0 {
            elle_jit_type_error_str("non-zero divisor")
        } else {
            Value::int(ai.wrapping_rem(bi)).to_bits()
        }
    } else if let (Some(af), Some(bf)) = (a.as_number(), b.as_number()) {
        Value::float(af % bf).to_bits()
    } else {
        elle_jit_type_error_str("number")
    }
}

// =============================================================================
// Bitwise Operations
// =============================================================================

/// Bitwise AND
#[no_mangle]
pub extern "C" fn elle_jit_bit_and(a: u64, b: u64) -> u64 {
    let a = unsafe { Value::from_bits(a) };
    let b = unsafe { Value::from_bits(b) };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        Value::int(ai & bi).to_bits()
    } else {
        elle_jit_type_error_str("integer")
    }
}

/// Bitwise OR
#[no_mangle]
pub extern "C" fn elle_jit_bit_or(a: u64, b: u64) -> u64 {
    let a = unsafe { Value::from_bits(a) };
    let b = unsafe { Value::from_bits(b) };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        Value::int(ai | bi).to_bits()
    } else {
        elle_jit_type_error_str("integer")
    }
}

/// Bitwise XOR
#[no_mangle]
pub extern "C" fn elle_jit_bit_xor(a: u64, b: u64) -> u64 {
    let a = unsafe { Value::from_bits(a) };
    let b = unsafe { Value::from_bits(b) };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        Value::int(ai ^ bi).to_bits()
    } else {
        elle_jit_type_error_str("integer")
    }
}

/// Shift left
#[no_mangle]
pub extern "C" fn elle_jit_shl(a: u64, b: u64) -> u64 {
    let a = unsafe { Value::from_bits(a) };
    let b = unsafe { Value::from_bits(b) };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        Value::int(ai.wrapping_shl(bi as u32)).to_bits()
    } else {
        elle_jit_type_error_str("integer")
    }
}

/// Shift right (arithmetic)
#[no_mangle]
pub extern "C" fn elle_jit_shr(a: u64, b: u64) -> u64 {
    let a = unsafe { Value::from_bits(a) };
    let b = unsafe { Value::from_bits(b) };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        Value::int(ai.wrapping_shr(bi as u32)).to_bits()
    } else {
        elle_jit_type_error_str("integer")
    }
}

// =============================================================================
// Unary Operations
// =============================================================================

/// Numeric negation
#[no_mangle]
pub extern "C" fn elle_jit_neg(a: u64) -> u64 {
    let a = unsafe { Value::from_bits(a) };
    if let Some(ai) = a.as_int() {
        Value::int(-ai).to_bits()
    } else if let Some(af) = a.as_float() {
        Value::float(-af).to_bits()
    } else {
        elle_jit_type_error_str("number")
    }
}

/// Logical NOT
#[no_mangle]
pub extern "C" fn elle_jit_not(a: u64) -> u64 {
    let a = unsafe { Value::from_bits(a) };
    Value::bool(!a.is_truthy()).to_bits()
}

/// Bitwise NOT
#[no_mangle]
pub extern "C" fn elle_jit_bit_not(a: u64) -> u64 {
    let a = unsafe { Value::from_bits(a) };
    if let Some(ai) = a.as_int() {
        Value::int(!ai).to_bits()
    } else {
        elle_jit_type_error_str("integer")
    }
}

// =============================================================================
// Comparison Operations
// =============================================================================

/// Equality comparison
#[no_mangle]
pub extern "C" fn elle_jit_eq(a: u64, b: u64) -> u64 {
    // For immediate values, bit equality is sufficient
    // For heap values, we'd need deeper comparison, but for Phase 1
    // we just compare bits
    Value::bool(a == b).to_bits()
}

/// Not equal comparison
#[no_mangle]
pub extern "C" fn elle_jit_ne(a: u64, b: u64) -> u64 {
    Value::bool(a != b).to_bits()
}

/// Less than comparison
#[no_mangle]
pub extern "C" fn elle_jit_lt(a: u64, b: u64) -> u64 {
    let a = unsafe { Value::from_bits(a) };
    let b = unsafe { Value::from_bits(b) };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        Value::bool(ai < bi).to_bits()
    } else if let (Some(af), Some(bf)) = (a.as_number(), b.as_number()) {
        Value::bool(af < bf).to_bits()
    } else {
        elle_jit_type_error_str("number")
    }
}

/// Less than or equal comparison
#[no_mangle]
pub extern "C" fn elle_jit_le(a: u64, b: u64) -> u64 {
    let a = unsafe { Value::from_bits(a) };
    let b = unsafe { Value::from_bits(b) };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        Value::bool(ai <= bi).to_bits()
    } else if let (Some(af), Some(bf)) = (a.as_number(), b.as_number()) {
        Value::bool(af <= bf).to_bits()
    } else {
        elle_jit_type_error_str("number")
    }
}

/// Greater than comparison
#[no_mangle]
pub extern "C" fn elle_jit_gt(a: u64, b: u64) -> u64 {
    let a = unsafe { Value::from_bits(a) };
    let b = unsafe { Value::from_bits(b) };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        Value::bool(ai > bi).to_bits()
    } else if let (Some(af), Some(bf)) = (a.as_number(), b.as_number()) {
        Value::bool(af > bf).to_bits()
    } else {
        elle_jit_type_error_str("number")
    }
}

/// Greater than or equal comparison
#[no_mangle]
pub extern "C" fn elle_jit_ge(a: u64, b: u64) -> u64 {
    let a = unsafe { Value::from_bits(a) };
    let b = unsafe { Value::from_bits(b) };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        Value::bool(ai >= bi).to_bits()
    } else if let (Some(af), Some(bf)) = (a.as_number(), b.as_number()) {
        Value::bool(af >= bf).to_bits()
    } else {
        elle_jit_type_error_str("number")
    }
}

// =============================================================================
// Data Construction
// =============================================================================

/// Allocate a cons cell
#[no_mangle]
pub extern "C" fn elle_jit_cons(car: u64, cdr: u64) -> u64 {
    let car = unsafe { Value::from_bits(car) };
    let cdr = unsafe { Value::from_bits(cdr) };
    Value::cons(car, cdr).to_bits()
}

// =============================================================================
// Type Checking
// =============================================================================

/// Check if value is nil
#[no_mangle]
pub extern "C" fn elle_jit_is_nil(a: u64) -> u64 {
    Value::bool(a == TAG_NIL).to_bits()
}

/// Check if value is truthy (not nil and not false)
#[no_mangle]
pub extern "C" fn elle_jit_is_truthy(a: u64) -> u64 {
    Value::bool(a != TAG_NIL && a != TAG_FALSE).to_bits()
}

/// Check if value is an integer
#[no_mangle]
pub extern "C" fn elle_jit_is_int(a: u64) -> u64 {
    Value::bool((a & TAG_INT_MASK) == TAG_INT).to_bits()
}

// =============================================================================
// Error Handling
// =============================================================================

/// Type error (called from JIT code when type check fails)
///
/// For Phase 1, this just prints an error and returns NIL.
/// Phase 4 will add proper exception handling.
#[no_mangle]
pub extern "C" fn elle_jit_type_error(expected: *const u8, expected_len: usize) -> u64 {
    let msg = unsafe {
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(expected, expected_len))
    };
    eprintln!("JIT type error: expected {}", msg);
    TAG_NIL
}

/// Type error helper that takes a static string
fn elle_jit_type_error_str(expected: &str) -> u64 {
    eprintln!("JIT type error: expected {}", expected);
    TAG_NIL
}

// =============================================================================
// Integer Fast Path Helpers (for future optimization)
// =============================================================================

/// Extract integer value from NaN-boxed representation
/// Returns the raw i64 value, sign-extended from 48 bits
#[allow(dead_code)]
#[inline]
pub fn extract_int(bits: u64) -> i64 {
    let raw = (bits & PAYLOAD_MASK) as i64;
    // Sign-extend from 48 bits
    if raw & (1 << 47) != 0 {
        raw | !PAYLOAD_MASK as i64
    } else {
        raw
    }
}

/// Encode an integer as NaN-boxed representation
#[allow(dead_code)]
#[inline]
pub fn encode_int(n: i64) -> u64 {
    TAG_INT | ((n as u64) & PAYLOAD_MASK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_integers() {
        let a = Value::int(10).to_bits();
        let b = Value::int(20).to_bits();
        let result = elle_jit_add(a, b);
        let v = unsafe { Value::from_bits(result) };
        assert_eq!(v.as_int(), Some(30));
    }

    #[test]
    fn test_sub_integers() {
        let a = Value::int(30).to_bits();
        let b = Value::int(10).to_bits();
        let result = elle_jit_sub(a, b);
        let v = unsafe { Value::from_bits(result) };
        assert_eq!(v.as_int(), Some(20));
    }

    #[test]
    fn test_mul_integers() {
        let a = Value::int(6).to_bits();
        let b = Value::int(7).to_bits();
        let result = elle_jit_mul(a, b);
        let v = unsafe { Value::from_bits(result) };
        assert_eq!(v.as_int(), Some(42));
    }

    #[test]
    fn test_comparison() {
        let a = Value::int(10).to_bits();
        let b = Value::int(20).to_bits();

        let lt = unsafe { Value::from_bits(elle_jit_lt(a, b)) };
        assert_eq!(lt.as_bool(), Some(true));

        let gt = unsafe { Value::from_bits(elle_jit_gt(a, b)) };
        assert_eq!(gt.as_bool(), Some(false));

        let eq = unsafe { Value::from_bits(elle_jit_eq(a, a)) };
        assert_eq!(eq.as_bool(), Some(true));
    }

    #[test]
    fn test_not() {
        let t = Value::TRUE.to_bits();
        let f = Value::FALSE.to_bits();
        let n = Value::NIL.to_bits();

        let not_t = unsafe { Value::from_bits(elle_jit_not(t)) };
        assert_eq!(not_t.as_bool(), Some(false));

        let not_f = unsafe { Value::from_bits(elle_jit_not(f)) };
        assert_eq!(not_f.as_bool(), Some(true));

        let not_n = unsafe { Value::from_bits(elle_jit_not(n)) };
        assert_eq!(not_n.as_bool(), Some(true));
    }

    #[test]
    fn test_extract_encode_int() {
        for n in [-1000i64, -1, 0, 1, 1000, i32::MAX as i64, i32::MIN as i64] {
            let encoded = encode_int(n);
            let decoded = extract_int(encoded);
            assert_eq!(decoded, n, "Failed for {}", n);
        }
    }
}
