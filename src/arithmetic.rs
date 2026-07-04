//! Unified arithmetic operations for both VM and primitives
//!
//! This module provides a single source of truth for arithmetic operations
//! (add, subtract, multiply, divide, etc.) to avoid duplication between
//! the VM's binary stack operations and the primitives' variadic functions.
//!
//! On error these functions return an error *description* `(&'static str,
//! String)` (kind, message) — NOT a pre-built error `Value`. The caller owns a
//! region (the VM via `set_error`, a native via its `ctx`) and builds the error
//! there, so these pure functions allocate only through the region source the
//! caller supplies — they mint nothing on their own (Rule 3;
//! docs/impl/region-ctx.md). The VM's unchecked intrinsic handlers
//! discard the description entirely (wrong types → garbage sentinel).
//!
//! Integer arithmetic is 64-bit two's-complement and WRAPS on overflow
//! (docs/intrinsics.md § Integer overflow): the compiler specializes `+`
//! to the signal-free `%add` instruction whenever both operands are proven
//! ints, and a type proof cannot exclude overflow — so any non-wrapping
//! semantics here would make the checked and specialized paths disagree.

use crate::value::Value;

/// Add two numeric values, promoting to float when either operand is float.
pub(crate) fn add_values(a: &Value, b: &Value) -> Result<Value, (&'static str, String)> {
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        return Ok(Value::int(x.wrapping_add(y)));
    }
    match (a.as_number(), b.as_number()) {
        (Some(x), Some(y)) => Ok(Value::float(x + y)),
        _ => Err((
            "type-error",
            format!(
                "+: expected number, got {} and {}",
                a.type_name(),
                b.type_name()
            ),
        )),
    }
}

/// Subtract two numeric values, promoting to float when either operand is float.
pub(crate) fn sub_values(a: &Value, b: &Value) -> Result<Value, (&'static str, String)> {
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        return Ok(Value::int(x.wrapping_sub(y)));
    }
    match (a.as_number(), b.as_number()) {
        (Some(x), Some(y)) => Ok(Value::float(x - y)),
        _ => Err((
            "type-error",
            format!(
                "-: expected number, got {} and {}",
                a.type_name(),
                b.type_name()
            ),
        )),
    }
}

/// Multiply two numeric values, promoting to float when either operand is float.
pub(crate) fn mul_values(a: &Value, b: &Value) -> Result<Value, (&'static str, String)> {
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        return Ok(Value::int(x.wrapping_mul(y)));
    }
    match (a.as_number(), b.as_number()) {
        (Some(x), Some(y)) => Ok(Value::float(x * y)),
        _ => Err((
            "type-error",
            format!(
                "*: expected number, got {} and {}",
                a.type_name(),
                b.type_name()
            ),
        )),
    }
}

/// Divide two numeric values. Integer division truncates; mixed/float
/// division follows IEEE 754 (including Inf on divide-by-zero).
pub(crate) fn div_values(a: &Value, b: &Value) -> Result<Value, (&'static str, String)> {
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        if y == 0 {
            return Err(("division-by-zero", "/: division by zero".to_string()));
        }
        // wrapping_div: i64::MIN / -1 wraps to i64::MIN (bare `/` panics).
        return Ok(Value::int(x.wrapping_div(y)));
    }
    match (a.as_number(), b.as_number()) {
        (Some(x), Some(y)) => Ok(Value::float(x / y)),
        _ => Err((
            "type-error",
            format!(
                "/: expected number, got {} and {}",
                a.type_name(),
                b.type_name()
            ),
        )),
    }
}

/// Remainder operation (truncated division - result has same sign as dividend)
pub(crate) fn remainder_values(a: &Value, b: &Value) -> Result<Value, (&'static str, String)> {
    match (a.as_int(), a.as_float(), b.as_int(), b.as_float()) {
        (Some(x), _, Some(y), _) => {
            if y == 0 {
                return Err(("division-by-zero", "rem: division by zero".to_string()));
            }
            // wrapping_rem: i64::MIN % -1 is 0 (bare `%` panics on overflow).
            Ok(Value::int(x.wrapping_rem(y)))
        }
        (Some(x), _, _, Some(y)) => Ok(Value::float((x as f64) % y)),
        (_, Some(x), Some(y), _) => Ok(Value::float(x % (y as f64))),
        (_, Some(x), _, Some(y)) => Ok(Value::float(x % y)),
        _ => Err((
            "type-error",
            format!(
                "rem: expected number, got {} and {}",
                a.type_name(),
                b.type_name()
            ),
        )),
    }
}

/// The language `=` (docs/types.md § Equality): structural equality
/// with numeric coercion and IEEE 754 float semantics at every depth.
/// Compositional: (= [a] [b]) ⇔ (= a b). Used by the VM's Eq
/// instruction, the `=` primitive, and the JIT's eq/ne slow paths.
#[inline]
pub(crate) fn values_eq(a: &Value, b: &Value) -> bool {
    use crate::value::repr::eq::{eq_with, Relation};
    eq_with(a, b, Relation::Numeric)
}

#[cfg(test)]
mod tests;
