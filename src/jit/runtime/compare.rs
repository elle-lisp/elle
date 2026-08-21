use super::*;

/// Equality comparison — the language `=` via `values_eq`: numeric
/// coercion and IEEE 754 floats at every depth, compositional through
/// collections. One source of truth with the interpreter's Eq
/// instruction and the `=` primitive — any local fast path here would
/// let the JIT tier disagree with the VM (the old copy returned true
/// for bitwise-identical NaN and coerced int-int through f64).
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
    JitValue::bool_val(crate::arithmetic::values_eq(&a, &b))
}

/// Not equal comparison (inverse of elle_jit_eq).
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
    JitValue::bool_val(!crate::arithmetic::values_eq(&a, &b))
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
