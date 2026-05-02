//! Runtime helper functions for JIT-compiled code
//!
//! These functions are called from JIT-compiled code to perform operations
//! that are too complex to inline, such as arithmetic with type checking.
//!
//! All functions use the C calling convention and operate on (tag, payload)
//! pairs representing 16-byte Values.
//!
//! `JitValue` with `#[repr(C)]` is FFI-compatible on all Cranelift targets:
//! a two-field struct of u64s is returned in a register pair (rax:rdx on
//! x86-64, x0:x1 on aarch64), matching Cranelift's two-I64 return convention.

use crate::jit::value::JitValue;
use crate::value::repr::TAG_INT;
use crate::value::repr::TAG_NIL;
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
pub extern "C" fn elle_jit_add(a_tag: u64, a_payload: u64, b_tag: u64, b_payload: u64) -> JitValue {
    let a = Value {
        tag: a_tag,
        payload: a_payload,
    };
    let b = Value {
        tag: b_tag,
        payload: b_payload,
    };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        match ai.checked_add(bi) {
            Some(r) => JitValue::from_value(Value::int(r)),
            None => overflow_error_jv("addition"),
        }
    } else if let (Some(af), Some(bf)) = (a.as_number(), b.as_number()) {
        JitValue::from_value(Value::float(af + bf))
    } else {
        type_error_jv("number")
    }
}

/// Integer subtraction
#[no_mangle]
pub extern "C" fn elle_jit_sub(a_tag: u64, a_payload: u64, b_tag: u64, b_payload: u64) -> JitValue {
    let a = Value {
        tag: a_tag,
        payload: a_payload,
    };
    let b = Value {
        tag: b_tag,
        payload: b_payload,
    };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        match ai.checked_sub(bi) {
            Some(r) => JitValue::from_value(Value::int(r)),
            None => overflow_error_jv("subtraction"),
        }
    } else if let (Some(af), Some(bf)) = (a.as_number(), b.as_number()) {
        JitValue::from_value(Value::float(af - bf))
    } else {
        type_error_jv("number")
    }
}

/// Integer multiplication
#[no_mangle]
pub extern "C" fn elle_jit_mul(a_tag: u64, a_payload: u64, b_tag: u64, b_payload: u64) -> JitValue {
    let a = Value {
        tag: a_tag,
        payload: a_payload,
    };
    let b = Value {
        tag: b_tag,
        payload: b_payload,
    };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        match ai.checked_mul(bi) {
            Some(r) => JitValue::from_value(Value::int(r)),
            None => overflow_error_jv("multiplication"),
        }
    } else if let (Some(af), Some(bf)) = (a.as_number(), b.as_number()) {
        JitValue::from_value(Value::float(af * bf))
    } else {
        type_error_jv("number")
    }
}

/// Integer division
#[no_mangle]
pub extern "C" fn elle_jit_div(a_tag: u64, a_payload: u64, b_tag: u64, b_payload: u64) -> JitValue {
    let a = Value {
        tag: a_tag,
        payload: a_payload,
    };
    let b = Value {
        tag: b_tag,
        payload: b_payload,
    };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        if bi == 0 {
            type_error_jv("non-zero divisor")
        } else {
            match ai.checked_div(bi) {
                Some(r) => JitValue::from_value(Value::int(r)),
                None => overflow_error_jv("division"),
            }
        }
    } else if let (Some(af), Some(bf)) = (a.as_number(), b.as_number()) {
        JitValue::from_value(Value::float(af / bf))
    } else {
        type_error_jv("number")
    }
}

/// Integer remainder
#[no_mangle]
pub extern "C" fn elle_jit_rem(a_tag: u64, a_payload: u64, b_tag: u64, b_payload: u64) -> JitValue {
    let a = Value {
        tag: a_tag,
        payload: a_payload,
    };
    let b = Value {
        tag: b_tag,
        payload: b_payload,
    };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        if bi == 0 {
            type_error_jv("non-zero divisor")
        } else {
            match ai.checked_rem(bi) {
                Some(r) => JitValue::from_value(Value::int(r)),
                None => overflow_error_jv("remainder"),
            }
        }
    } else if let (Some(af), Some(bf)) = (a.as_number(), b.as_number()) {
        JitValue::from_value(Value::float(af % bf))
    } else {
        type_error_jv("number")
    }
}

// =============================================================================
// Bitwise Operations
// =============================================================================

/// Bitwise AND
#[no_mangle]
pub extern "C" fn elle_jit_bit_and(
    a_tag: u64,
    a_payload: u64,
    b_tag: u64,
    b_payload: u64,
) -> JitValue {
    let a = Value {
        tag: a_tag,
        payload: a_payload,
    };
    let b = Value {
        tag: b_tag,
        payload: b_payload,
    };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        JitValue::from_value(Value::int(ai & bi))
    } else {
        type_error_jv("integer")
    }
}

/// Bitwise OR
#[no_mangle]
pub extern "C" fn elle_jit_bit_or(
    a_tag: u64,
    a_payload: u64,
    b_tag: u64,
    b_payload: u64,
) -> JitValue {
    let a = Value {
        tag: a_tag,
        payload: a_payload,
    };
    let b = Value {
        tag: b_tag,
        payload: b_payload,
    };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        JitValue::from_value(Value::int(ai | bi))
    } else {
        type_error_jv("integer")
    }
}

/// Bitwise XOR
#[no_mangle]
pub extern "C" fn elle_jit_bit_xor(
    a_tag: u64,
    a_payload: u64,
    b_tag: u64,
    b_payload: u64,
) -> JitValue {
    let a = Value {
        tag: a_tag,
        payload: a_payload,
    };
    let b = Value {
        tag: b_tag,
        payload: b_payload,
    };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        JitValue::from_value(Value::int(ai ^ bi))
    } else {
        type_error_jv("integer")
    }
}

/// Shift left
#[no_mangle]
pub extern "C" fn elle_jit_shl(a_tag: u64, a_payload: u64, b_tag: u64, b_payload: u64) -> JitValue {
    let a = Value {
        tag: a_tag,
        payload: a_payload,
    };
    let b = Value {
        tag: b_tag,
        payload: b_payload,
    };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        JitValue::from_value(Value::int(ai.wrapping_shl(bi as u32)))
    } else {
        type_error_jv("integer")
    }
}

/// Shift right (arithmetic)
#[no_mangle]
pub extern "C" fn elle_jit_shr(a_tag: u64, a_payload: u64, b_tag: u64, b_payload: u64) -> JitValue {
    let a = Value {
        tag: a_tag,
        payload: a_payload,
    };
    let b = Value {
        tag: b_tag,
        payload: b_payload,
    };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        JitValue::from_value(Value::int(ai.wrapping_shr(bi as u32)))
    } else {
        type_error_jv("integer")
    }
}

// =============================================================================
// Unary Operations
// =============================================================================

/// Numeric negation
#[no_mangle]
pub extern "C" fn elle_jit_neg(tag: u64, payload: u64) -> JitValue {
    let a = Value { tag, payload };
    if let Some(ai) = a.as_int() {
        match ai.checked_neg() {
            Some(r) => JitValue::from_value(Value::int(r)),
            None => overflow_error_jv("negation"),
        }
    } else if let Some(af) = a.as_float() {
        JitValue::from_value(Value::float(-af))
    } else {
        type_error_jv("number")
    }
}

/// Logical NOT
#[no_mangle]
pub extern "C" fn elle_jit_not(tag: u64, payload: u64) -> JitValue {
    let a = Value { tag, payload };
    JitValue::from_value(Value::bool(!a.is_truthy()))
}

/// Bitwise NOT
#[no_mangle]
pub extern "C" fn elle_jit_bit_not(tag: u64, payload: u64) -> JitValue {
    let a = Value { tag, payload };
    if let Some(ai) = a.as_int() {
        JitValue::from_value(Value::int(!ai))
    } else {
        type_error_jv("integer")
    }
}

// =============================================================================
// Conversion Operations
// =============================================================================

/// Convert to float: int → float, float → identity, else type error.
#[no_mangle]
pub extern "C" fn elle_jit_int_to_float(tag: u64, payload: u64) -> JitValue {
    let a = Value { tag, payload };
    if let Some(n) = a.as_int() {
        JitValue::from_value(Value::float(n as f64))
    } else if a.as_float().is_some() {
        JitValue::from_value(a)
    } else {
        type_error_jv("number")
    }
}

/// Convert to int: float → truncate to int, int → identity, else type error.
#[no_mangle]
pub extern "C" fn elle_jit_float_to_int(tag: u64, payload: u64) -> JitValue {
    let a = Value { tag, payload };
    if let Some(f) = a.as_float() {
        JitValue::from_value(Value::int(f as i64))
    } else if a.as_int().is_some() {
        JitValue::from_value(a)
    } else {
        type_error_jv("number")
    }
}

// =============================================================================
// Comparison Operations
// =============================================================================

/// Equality comparison — numeric-aware.
/// If both values are numbers, compares numerically (int 1 == float 1.0).
/// Otherwise, uses structural equality (PartialEq).
#[no_mangle]
pub extern "C" fn elle_jit_eq(a_tag: u64, a_payload: u64, b_tag: u64, b_payload: u64) -> JitValue {
    let a = Value {
        tag: a_tag,
        payload: a_payload,
    };
    let b = Value {
        tag: b_tag,
        payload: b_payload,
    };
    // Fast path: bitwise identical
    if a == b {
        return JitValue::bool_val(true);
    }
    // Numeric coercion: int 1 == float 1.0
    if a.is_number() && b.is_number() {
        if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
            return JitValue::bool_val(x == y);
        }
    }
    JitValue::bool_val(false)
}

/// Not equal comparison — numeric-aware (inverse of elle_jit_eq).
#[no_mangle]
pub extern "C" fn elle_jit_ne(a_tag: u64, a_payload: u64, b_tag: u64, b_payload: u64) -> JitValue {
    let a = Value {
        tag: a_tag,
        payload: a_payload,
    };
    let b = Value {
        tag: b_tag,
        payload: b_payload,
    };
    if a == b {
        return JitValue::bool_val(false);
    }
    if a.is_number() && b.is_number() {
        if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
            return JitValue::bool_val(x != y);
        }
    }
    JitValue::bool_val(true)
}

/// Less than comparison
#[no_mangle]
pub extern "C" fn elle_jit_lt(a_tag: u64, a_payload: u64, b_tag: u64, b_payload: u64) -> JitValue {
    let a = Value {
        tag: a_tag,
        payload: a_payload,
    };
    let b = Value {
        tag: b_tag,
        payload: b_payload,
    };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        return JitValue::bool_val(ai < bi);
    }
    if let (Some(af), Some(bf)) = (a.as_number(), b.as_number()) {
        return JitValue::bool_val(af < bf);
    }
    if let Some(ord) = a.compare_str(&b) {
        return JitValue::bool_val(ord.is_lt());
    }
    if let Some(ord) = a.compare_keyword(&b) {
        return JitValue::bool_val(ord.is_lt());
    }
    type_error_jv("number, string, or keyword")
}

/// Less than or equal comparison
#[no_mangle]
pub extern "C" fn elle_jit_le(a_tag: u64, a_payload: u64, b_tag: u64, b_payload: u64) -> JitValue {
    let a = Value {
        tag: a_tag,
        payload: a_payload,
    };
    let b = Value {
        tag: b_tag,
        payload: b_payload,
    };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        return JitValue::bool_val(ai <= bi);
    }
    if let (Some(af), Some(bf)) = (a.as_number(), b.as_number()) {
        return JitValue::bool_val(af <= bf);
    }
    if let Some(ord) = a.compare_str(&b) {
        return JitValue::bool_val(ord.is_le());
    }
    if let Some(ord) = a.compare_keyword(&b) {
        return JitValue::bool_val(ord.is_le());
    }
    type_error_jv("number, string, or keyword")
}

/// Greater than comparison
#[no_mangle]
pub extern "C" fn elle_jit_gt(a_tag: u64, a_payload: u64, b_tag: u64, b_payload: u64) -> JitValue {
    let a = Value {
        tag: a_tag,
        payload: a_payload,
    };
    let b = Value {
        tag: b_tag,
        payload: b_payload,
    };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        return JitValue::bool_val(ai > bi);
    }
    if let (Some(af), Some(bf)) = (a.as_number(), b.as_number()) {
        return JitValue::bool_val(af > bf);
    }
    if let Some(ord) = a.compare_str(&b) {
        return JitValue::bool_val(ord.is_gt());
    }
    if let Some(ord) = a.compare_keyword(&b) {
        return JitValue::bool_val(ord.is_gt());
    }
    type_error_jv("number, string, or keyword")
}

/// Greater than or equal comparison
#[no_mangle]
pub extern "C" fn elle_jit_ge(a_tag: u64, a_payload: u64, b_tag: u64, b_payload: u64) -> JitValue {
    let a = Value {
        tag: a_tag,
        payload: a_payload,
    };
    let b = Value {
        tag: b_tag,
        payload: b_payload,
    };
    if let (Some(ai), Some(bi)) = (a.as_int(), b.as_int()) {
        return JitValue::bool_val(ai >= bi);
    }
    if let (Some(af), Some(bf)) = (a.as_number(), b.as_number()) {
        return JitValue::bool_val(af >= bf);
    }
    if let Some(ord) = a.compare_str(&b) {
        return JitValue::bool_val(ord.is_ge());
    }
    if let Some(ord) = a.compare_keyword(&b) {
        return JitValue::bool_val(ord.is_ge());
    }
    type_error_jv("number, string, or keyword")
}

// =============================================================================
// Type Checking
// =============================================================================

/// Check if value is nil
#[no_mangle]
pub extern "C" fn elle_jit_is_nil(tag: u64, _payload: u64) -> JitValue {
    JitValue::bool_val(tag == TAG_NIL)
}

/// Check if value is truthy (not nil and not false)
#[no_mangle]
pub extern "C" fn elle_jit_is_truthy(tag: u64, payload: u64) -> JitValue {
    let v = Value { tag, payload };
    JitValue::bool_val(v.is_truthy())
}

/// Check if value is an integer
#[no_mangle]
pub extern "C" fn elle_jit_is_int(tag: u64, _payload: u64) -> JitValue {
    JitValue::bool_val(tag == TAG_INT)
}

/// Check if value is the empty list
#[no_mangle]
pub extern "C" fn elle_jit_is_empty(tag: u64, _payload: u64) -> JitValue {
    use crate::value::repr::TAG_EMPTY_LIST;
    JitValue::bool_val(tag == TAG_EMPTY_LIST)
}

/// Check if value is a boolean
#[no_mangle]
pub extern "C" fn elle_jit_is_bool(tag: u64, _payload: u64) -> JitValue {
    use crate::value::repr::{TAG_FALSE, TAG_TRUE};
    JitValue::bool_val(tag == TAG_TRUE || tag == TAG_FALSE)
}

/// Check if value is a float
#[no_mangle]
pub extern "C" fn elle_jit_is_float(tag: u64, _payload: u64) -> JitValue {
    use crate::value::repr::TAG_FLOAT;
    JitValue::bool_val(tag == TAG_FLOAT)
}

/// Check if value is a string (immutable or mutable)
#[no_mangle]
pub extern "C" fn elle_jit_is_string(tag: u64, _payload: u64) -> JitValue {
    use crate::value::repr::{TAG_STRING, TAG_STRING_MUT};
    JitValue::bool_val(tag == TAG_STRING || tag == TAG_STRING_MUT)
}

/// Check if value is a keyword
#[no_mangle]
pub extern "C" fn elle_jit_is_keyword(tag: u64, _payload: u64) -> JitValue {
    use crate::value::repr::TAG_KEYWORD;
    JitValue::bool_val(tag == TAG_KEYWORD)
}

/// Check if value is a symbol
#[no_mangle]
pub extern "C" fn elle_jit_is_symbol_check(tag: u64, _payload: u64) -> JitValue {
    use crate::value::repr::TAG_SYMBOL;
    JitValue::bool_val(tag == TAG_SYMBOL)
}

/// Check if value is bytes (immutable or mutable)
#[no_mangle]
pub extern "C" fn elle_jit_is_bytes(tag: u64, _payload: u64) -> JitValue {
    use crate::value::repr::{TAG_BYTES, TAG_BYTES_MUT};
    JitValue::bool_val(tag == TAG_BYTES || tag == TAG_BYTES_MUT)
}

/// Check if value is a box (lbox)
#[no_mangle]
pub extern "C" fn elle_jit_is_box(tag: u64, _payload: u64) -> JitValue {
    use crate::value::repr::TAG_LBOX;
    JitValue::bool_val(tag == TAG_LBOX)
}

/// Check if value is a closure
#[no_mangle]
pub extern "C" fn elle_jit_is_closure(tag: u64, _payload: u64) -> JitValue {
    use crate::value::repr::TAG_CLOSURE;
    JitValue::bool_val(tag == TAG_CLOSURE)
}

/// Check if value is a fiber
#[no_mangle]
pub extern "C" fn elle_jit_is_fiber(tag: u64, _payload: u64) -> JitValue {
    use crate::value::repr::TAG_FIBER;
    JitValue::bool_val(tag == TAG_FIBER)
}

/// Get type keyword for a value
#[no_mangle]
pub extern "C" fn elle_jit_type_of(tag: u64, payload: u64) -> JitValue {
    let v = Value { tag, payload };
    JitValue::from_value(Value::keyword(v.type_name()))
}

/// Polymorphic length — panics on unsupported types (intrinsic contract).
#[no_mangle]
pub extern "C" fn elle_jit_length(tag: u64, payload: u64) -> JitValue {
    let v = Value { tag, payload };
    use unicode_segmentation::UnicodeSegmentation;
    let len = if v.is_empty_list() || v.is_nil() {
        0
    } else if v.is_pair() {
        v.list_to_vec().expect("%length: improper list").len()
    } else if let Some(a) = v.as_array() {
        a.len()
    } else if let Some(a) = v.as_array_mut() {
        a.borrow().len()
    } else if let Some(s) = v.as_struct() {
        s.len()
    } else if let Some(s) = v.as_struct_mut() {
        s.borrow().len()
    } else if let Some(s) = v.as_set() {
        s.len()
    } else if let Some(s) = v.as_set_mut() {
        s.borrow().len()
    } else if let Some(b) = v.as_bytes() {
        b.len()
    } else if let Some(b) = v.as_bytes_mut() {
        b.borrow().len()
    } else if let Some(r) = v.with_string(|s| s.graphemes(true).count()) {
        r
    } else if let Some(buf) = v.as_string_mut() {
        let b = buf.borrow();
        std::str::from_utf8(&b)
            .expect("%length: @string invalid UTF-8")
            .graphemes(true)
            .count()
    } else {
        panic!("%length: unsupported type {}", v.type_name())
    };
    JitValue::from_value(Value::int(len as i64))
}

/// Polymorphic get — panics on unsupported types (intrinsic contract).
#[no_mangle]
pub extern "C" fn elle_jit_get(obj_tag: u64, obj_pay: u64, key_tag: u64, key_pay: u64) -> JitValue {
    let obj = Value {
        tag: obj_tag,
        payload: obj_pay,
    };
    let key = Value {
        tag: key_tag,
        payload: key_pay,
    };
    use crate::value::TableKey;
    let result = if let Some(elems) = obj.as_array() {
        elems[key.as_int().expect("%get: index must be int") as usize]
    } else if let Some(a) = obj.as_array_mut() {
        a.borrow()[key.as_int().expect("%get: index must be int") as usize]
    } else if let Some(pairs) = obj.as_struct() {
        let tk = TableKey::from_value(&key).expect("%get: unhashable key");
        crate::value::sorted_struct_get(pairs, &tk)
            .copied()
            .unwrap_or(Value::NIL)
    } else if let Some(t) = obj.as_struct_mut() {
        let tk = TableKey::from_value(&key).expect("%get: unhashable key");
        t.borrow().get(&tk).copied().unwrap_or(Value::NIL)
    } else {
        panic!("%get: unsupported type {}", obj.type_name())
    };
    JitValue::from_value(result)
}

/// Polymorphic put — panics on type error (intrinsic contract).
#[no_mangle]
pub extern "C" fn elle_jit_put(
    obj_tag: u64,
    obj_pay: u64,
    key_tag: u64,
    key_pay: u64,
    val_tag: u64,
    val_pay: u64,
) -> JitValue {
    let obj = Value {
        tag: obj_tag,
        payload: obj_pay,
    };
    let key = Value {
        tag: key_tag,
        payload: key_pay,
    };
    let val = Value {
        tag: val_tag,
        payload: val_pay,
    };
    let (bits, result) = crate::primitives::access::prim_put(&[obj, key, val]);
    assert!(
        !bits.contains(crate::value::SIG_ERROR),
        "%put: intrinsic contract violated"
    );
    JitValue::from_value(result)
}

/// Polymorphic del — panics on type error (intrinsic contract).
#[no_mangle]
pub extern "C" fn elle_jit_del(obj_tag: u64, obj_pay: u64, key_tag: u64, key_pay: u64) -> JitValue {
    let obj = Value {
        tag: obj_tag,
        payload: obj_pay,
    };
    let key = Value {
        tag: key_tag,
        payload: key_pay,
    };
    let (bits, result) = crate::primitives::lstruct::prim_del(&[obj, key]);
    assert!(
        !bits.contains(crate::value::SIG_ERROR),
        "%del: intrinsic contract violated"
    );
    JitValue::from_value(result)
}

/// Polymorphic has? — panics on type error (intrinsic contract).
#[no_mangle]
pub extern "C" fn elle_jit_has(obj_tag: u64, obj_pay: u64, key_tag: u64, key_pay: u64) -> JitValue {
    let obj = Value {
        tag: obj_tag,
        payload: obj_pay,
    };
    let key = Value {
        tag: key_tag,
        payload: key_pay,
    };
    let (bits, result) = crate::primitives::lstruct::prim_has_key(&[obj, key]);
    assert!(
        !bits.contains(crate::value::SIG_ERROR),
        "%has?: intrinsic contract violated"
    );
    JitValue::from_value(result)
}

/// Push — panics on type error (intrinsic contract).
#[no_mangle]
pub extern "C" fn elle_jit_push(
    arr_tag: u64,
    arr_pay: u64,
    val_tag: u64,
    val_pay: u64,
) -> JitValue {
    let arr = Value {
        tag: arr_tag,
        payload: arr_pay,
    };
    let val = Value {
        tag: val_tag,
        payload: val_pay,
    };
    if let Some(vec_ref) = arr.as_array_mut() {
        vec_ref.borrow_mut().push(val);
        JitValue::from_value(arr)
    } else if let Some(elems) = arr.as_array() {
        let mut new = elems.to_vec();
        new.push(val);
        JitValue::from_value(Value::array(new))
    } else {
        panic!("%push: unsupported type {}", arr.type_name())
    }
}

/// Pop — panics on type error or empty (intrinsic contract).
#[no_mangle]
pub extern "C" fn elle_jit_pop(tag: u64, payload: u64) -> JitValue {
    let v = Value { tag, payload };
    let arr = v.as_array_mut().expect("%pop: expected @array");
    let popped = arr.borrow_mut().pop().expect("%pop: empty @array");
    JitValue::from_value(popped)
}

/// Freeze — pass-through for already-immutable types.
#[no_mangle]
pub extern "C" fn elle_jit_freeze(tag: u64, payload: u64) -> JitValue {
    let v = Value { tag, payload };
    let result = if let Some(a) = v.as_array_mut() {
        Value::array(a.borrow().clone())
    } else if let Some(t) = v.as_struct_mut() {
        let entries: Vec<_> = t.borrow().iter().map(|(k, v)| (k.clone(), *v)).collect();
        Value::struct_from_sorted(entries)
    } else if let Some(s) = v.as_set_mut() {
        Value::set(s.borrow().clone())
    } else if let Some(buf) = v.as_string_mut() {
        let b = buf.borrow();
        Value::string(std::str::from_utf8(&b).expect("%freeze: invalid UTF-8"))
    } else if let Some(b) = v.as_bytes_mut() {
        Value::bytes(b.borrow().clone())
    } else {
        v // already immutable
    };
    JitValue::from_value(result)
}

/// Thaw — pass-through for already-mutable types.
#[no_mangle]
pub extern "C" fn elle_jit_thaw(tag: u64, payload: u64) -> JitValue {
    let v = Value { tag, payload };
    let result = if let Some(a) = v.as_array() {
        Value::array_mut(a.to_vec())
    } else if let Some(s) = v.as_struct() {
        let entries: std::collections::BTreeMap<_, _> =
            s.iter().map(|(k, v)| (k.clone(), *v)).collect();
        Value::struct_mut_from(entries)
    } else if let Some(s) = v.as_set() {
        Value::set_mut(s.iter().cloned().collect())
    } else if let Some(r) = v.with_string(|s| Value::string_mut(s.as_bytes().to_vec())) {
        r
    } else if let Some(b) = v.as_bytes() {
        Value::bytes_mut(b.to_vec())
    } else {
        v // already mutable
    };
    JitValue::from_value(result)
}

/// Bitwise identity comparison (pointer identity for heap values)
#[no_mangle]
pub extern "C" fn elle_jit_identical(a_tag: u64, a_pay: u64, b_tag: u64, b_pay: u64) -> JitValue {
    JitValue::bool_val(a_tag == b_tag && a_pay == b_pay)
}

// =============================================================================
// Error Handling
// =============================================================================

/// Type error (called from JIT code when type check fails)
#[no_mangle]
pub extern "C" fn elle_jit_type_error(expected: *const u8, expected_len: usize) -> JitValue {
    let msg = unsafe {
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(expected, expected_len))
    };
    eprintln!("JIT type error: expected {}", msg);
    JitValue::nil()
}

/// Type error helper that takes a static string
pub(super) fn type_error_jv(expected: &str) -> JitValue {
    eprintln!("JIT type error: expected {}", expected);
    JitValue::nil()
}

/// Overflow error helper for JIT arithmetic
fn overflow_error_jv(op: &str) -> JitValue {
    eprintln!("JIT overflow error: integer {} overflow", op);
    JitValue::nil()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_integers() {
        let a = Value::int(10);
        let b = Value::int(20);
        let v = elle_jit_add(a.tag, a.payload, b.tag, b.payload).to_value();
        assert_eq!(v.as_int(), Some(30));
    }

    #[test]
    fn test_sub_integers() {
        let a = Value::int(30);
        let b = Value::int(10);
        let v = elle_jit_sub(a.tag, a.payload, b.tag, b.payload).to_value();
        assert_eq!(v.as_int(), Some(20));
    }

    #[test]
    fn test_mul_integers() {
        let a = Value::int(6);
        let b = Value::int(7);
        let v = elle_jit_mul(a.tag, a.payload, b.tag, b.payload).to_value();
        assert_eq!(v.as_int(), Some(42));
    }

    #[test]
    fn test_comparison() {
        let a = Value::int(10);
        let b = Value::int(20);

        assert_eq!(
            elle_jit_lt(a.tag, a.payload, b.tag, b.payload),
            JitValue::bool_val(true)
        );
        assert_eq!(
            elle_jit_gt(a.tag, a.payload, b.tag, b.payload),
            JitValue::bool_val(false)
        );
        assert_eq!(
            elle_jit_eq(a.tag, a.payload, a.tag, a.payload),
            JitValue::bool_val(true)
        );
    }

    #[test]
    fn test_not() {
        let t = Value::TRUE;
        let f = Value::FALSE;
        let n = Value::NIL;

        assert_eq!(elle_jit_not(t.tag, t.payload), JitValue::bool_val(false));
        assert_eq!(elle_jit_not(f.tag, f.payload), JitValue::bool_val(true));
        assert_eq!(elle_jit_not(n.tag, n.payload), JitValue::bool_val(true));
    }

    #[test]
    fn test_eq_heap_values() {
        let list1 = Value::pair(Value::int(1), Value::pair(Value::int(2), Value::EMPTY_LIST));
        let list2 = Value::pair(Value::int(1), Value::pair(Value::int(2), Value::EMPTY_LIST));

        assert_eq!(
            elle_jit_eq(list1.tag, list1.payload, list2.tag, list2.payload),
            JitValue::bool_val(true),
            "equal lists must be eq"
        );
        assert_eq!(
            elle_jit_ne(list1.tag, list1.payload, list2.tag, list2.payload),
            JitValue::bool_val(false),
            "equal lists must not be ne"
        );

        let list3 = Value::pair(Value::int(1), Value::pair(Value::int(3), Value::EMPTY_LIST));
        assert_eq!(
            elle_jit_eq(list1.tag, list1.payload, list3.tag, list3.payload),
            JitValue::bool_val(false),
            "different lists must not be eq"
        );

        let s1 = Value::string("hello");
        let s2 = Value::string("hello");
        assert_eq!(
            elle_jit_eq(s1.tag, s1.payload, s2.tag, s2.payload),
            JitValue::bool_val(true),
            "equal strings must be eq"
        );

        let s3 = Value::string("world");
        assert_eq!(
            elle_jit_eq(s1.tag, s1.payload, s3.tag, s3.payload),
            JitValue::bool_val(false),
            "different strings must not be eq"
        );
    }

    #[test]
    fn test_lt_strings() {
        let a = Value::string("apple");
        let b = Value::string("banana");
        assert_eq!(
            elle_jit_lt(a.tag, a.payload, b.tag, b.payload),
            JitValue::bool_val(true)
        );
        assert_eq!(
            elle_jit_lt(b.tag, b.payload, a.tag, a.payload),
            JitValue::bool_val(false)
        );
        assert_eq!(
            elle_jit_lt(a.tag, a.payload, a.tag, a.payload),
            JitValue::bool_val(false)
        );
    }

    #[test]
    fn test_gt_strings() {
        let a = Value::string("banana");
        let b = Value::string("apple");
        assert_eq!(
            elle_jit_gt(a.tag, a.payload, b.tag, b.payload),
            JitValue::bool_val(true)
        );
        assert_eq!(
            elle_jit_gt(b.tag, b.payload, a.tag, a.payload),
            JitValue::bool_val(false)
        );
    }

    #[test]
    fn test_le_strings() {
        let a = Value::string("apple");
        let b = Value::string("banana");
        assert_eq!(
            elle_jit_le(a.tag, a.payload, b.tag, b.payload),
            JitValue::bool_val(true)
        );
        assert_eq!(
            elle_jit_le(a.tag, a.payload, a.tag, a.payload),
            JitValue::bool_val(true)
        );
        assert_eq!(
            elle_jit_le(b.tag, b.payload, a.tag, a.payload),
            JitValue::bool_val(false)
        );
    }

    #[test]
    fn test_ge_strings() {
        let a = Value::string("banana");
        let b = Value::string("apple");
        assert_eq!(
            elle_jit_ge(a.tag, a.payload, b.tag, b.payload),
            JitValue::bool_val(true)
        );
        assert_eq!(
            elle_jit_ge(a.tag, a.payload, a.tag, a.payload),
            JitValue::bool_val(true)
        );
        assert_eq!(
            elle_jit_ge(b.tag, b.payload, a.tag, a.payload),
            JitValue::bool_val(false)
        );
    }

    #[test]
    fn test_lt_keywords() {
        let a = Value::keyword("apple");
        let b = Value::keyword("banana");
        assert_eq!(
            elle_jit_lt(a.tag, a.payload, b.tag, b.payload),
            JitValue::bool_val(true)
        );
        assert_eq!(
            elle_jit_lt(b.tag, b.payload, a.tag, a.payload),
            JitValue::bool_val(false)
        );
    }
}
