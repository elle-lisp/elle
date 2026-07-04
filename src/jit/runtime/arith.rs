use super::*;

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
