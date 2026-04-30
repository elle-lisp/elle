//! %-intrinsic NativeFn primitives.
//!
//! Each %-intrinsic is registered as a real `NativeFn` with `Signal::silent()`.
//! When `--checked-intrinsics` is active, the compiler routes `%add` etc. through
//! these functions instead of inlining to unchecked BinOp/CmpOp instructions.
//! Each validates types and returns `(SIG_ERROR, error_val(...))` on mismatch.

use crate::arithmetic;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

// ── Helpers ─────────────────────────────────────────────────────────

fn type_err(name: &str, expected: &str, got: &Value) -> (SignalBits, Value) {
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!("{}: expected {}, got {}", name, expected, got.type_name()),
        ),
    )
}

fn type_err2(name: &str, expected: &str, a: &Value, b: &Value) -> (SignalBits, Value) {
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "{}: expected {}, got {} and {}",
                name,
                expected,
                a.type_name(),
                b.type_name()
            ),
        ),
    )
}

// ── Arithmetic ──────────────────────────────────────────────────────

fn prim_add(args: &[Value]) -> (SignalBits, Value) {
    match arithmetic::add_values(&args[0], &args[1]) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

fn prim_sub(args: &[Value]) -> (SignalBits, Value) {
    if args.len() == 1 {
        // Unary negation
        if let Some(n) = args[0].as_int() {
            return (SIG_OK, Value::int(-n));
        }
        if let Some(f) = args[0].as_float() {
            return (SIG_OK, Value::float(-f));
        }
        return type_err("%sub", "number", &args[0]);
    }
    match arithmetic::sub_values(&args[0], &args[1]) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

fn prim_mul(args: &[Value]) -> (SignalBits, Value) {
    match arithmetic::mul_values(&args[0], &args[1]) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

fn prim_div(args: &[Value]) -> (SignalBits, Value) {
    match arithmetic::div_values(&args[0], &args[1]) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

fn prim_rem(args: &[Value]) -> (SignalBits, Value) {
    match arithmetic::remainder_values(&args[0], &args[1]) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

fn prim_mod(args: &[Value]) -> (SignalBits, Value) {
    // Floored modulus: ((a % b) + b) % b
    match arithmetic::remainder_values(&args[0], &args[1]) {
        Ok(rem) => {
            // Add divisor to remainder
            match arithmetic::add_values(&rem, &args[1]) {
                Ok(sum) => match arithmetic::remainder_values(&sum, &args[1]) {
                    Ok(v) => (SIG_OK, v),
                    Err(e) => (SIG_ERROR, e),
                },
                Err(e) => (SIG_ERROR, e),
            }
        }
        Err(e) => (SIG_ERROR, e),
    }
}

// ── Comparison ──────────────────────────────────────────────────────

fn prim_eq(args: &[Value]) -> (SignalBits, Value) {
    let a = &args[0];
    let b = &args[1];
    if *a == *b {
        return (SIG_OK, Value::TRUE);
    }
    if a.is_number() && b.is_number() {
        if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
            return (SIG_OK, Value::bool(x == y));
        }
        if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
            return (SIG_OK, Value::bool(x == y));
        }
    }
    (SIG_OK, Value::FALSE)
}

fn prim_ne(args: &[Value]) -> (SignalBits, Value) {
    let (_, eq_result) = prim_eq(args);
    if eq_result == Value::TRUE {
        (SIG_OK, Value::FALSE)
    } else {
        (SIG_OK, Value::TRUE)
    }
}

fn prim_lt(args: &[Value]) -> (SignalBits, Value) {
    let a = &args[0];
    let b = &args[1];
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        return (SIG_OK, Value::bool(x < y));
    }
    if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
        return (SIG_OK, Value::bool(x < y));
    }
    if let Some(ord) = a.compare_str(b) {
        return (SIG_OK, Value::bool(ord.is_lt()));
    }
    if let Some(ord) = a.compare_keyword(b) {
        return (SIG_OK, Value::bool(ord.is_lt()));
    }
    type_err2("%lt", "number, string, or keyword", a, b)
}

fn prim_gt(args: &[Value]) -> (SignalBits, Value) {
    let a = &args[0];
    let b = &args[1];
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        return (SIG_OK, Value::bool(x > y));
    }
    if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
        return (SIG_OK, Value::bool(x > y));
    }
    if let Some(ord) = a.compare_str(b) {
        return (SIG_OK, Value::bool(ord.is_gt()));
    }
    if let Some(ord) = a.compare_keyword(b) {
        return (SIG_OK, Value::bool(ord.is_gt()));
    }
    type_err2("%gt", "number, string, or keyword", a, b)
}

fn prim_le(args: &[Value]) -> (SignalBits, Value) {
    let a = &args[0];
    let b = &args[1];
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        return (SIG_OK, Value::bool(x <= y));
    }
    if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
        return (SIG_OK, Value::bool(x <= y));
    }
    if let Some(ord) = a.compare_str(b) {
        return (SIG_OK, Value::bool(ord.is_le()));
    }
    if let Some(ord) = a.compare_keyword(b) {
        return (SIG_OK, Value::bool(ord.is_le()));
    }
    type_err2("%le", "number, string, or keyword", a, b)
}

fn prim_ge(args: &[Value]) -> (SignalBits, Value) {
    let a = &args[0];
    let b = &args[1];
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        return (SIG_OK, Value::bool(x >= y));
    }
    if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
        return (SIG_OK, Value::bool(x >= y));
    }
    if let Some(ord) = a.compare_str(b) {
        return (SIG_OK, Value::bool(ord.is_ge()));
    }
    if let Some(ord) = a.compare_keyword(b) {
        return (SIG_OK, Value::bool(ord.is_ge()));
    }
    type_err2("%ge", "number, string, or keyword", a, b)
}

// ── Logical ─────────────────────────────────────────────────────────

fn prim_not(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::bool(!args[0].is_truthy()))
}

// ── Conversion ──────────────────────────────────────────────────────

fn prim_int(args: &[Value]) -> (SignalBits, Value) {
    if let Some(f) = args[0].as_float() {
        return (SIG_OK, Value::int(f as i64));
    }
    if args[0].as_int().is_some() {
        return (SIG_OK, args[0]);
    }
    type_err("%int", "number", &args[0])
}

fn prim_float(args: &[Value]) -> (SignalBits, Value) {
    if let Some(n) = args[0].as_int() {
        return (SIG_OK, Value::float(n as f64));
    }
    if args[0].as_float().is_some() {
        return (SIG_OK, args[0]);
    }
    type_err("%float", "number", &args[0])
}

// ── Data ────────────────────────────────────────────────────────────

fn prim_pair(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::pair(args[0], args[1]))
}

fn prim_first(args: &[Value]) -> (SignalBits, Value) {
    if let Some(p) = args[0].as_pair() {
        return (SIG_OK, p.first);
    }
    type_err("%first", "pair", &args[0])
}

fn prim_rest(args: &[Value]) -> (SignalBits, Value) {
    if let Some(p) = args[0].as_pair() {
        return (SIG_OK, p.rest);
    }
    type_err("%rest", "pair", &args[0])
}

// ── Bitwise ─────────────────────────────────────────────────────────

fn prim_bit_and(args: &[Value]) -> (SignalBits, Value) {
    let a = args[0].as_int().ok_or(()).map_err(|_| ());
    let b = args[1].as_int().ok_or(()).map_err(|_| ());
    match (a, b) {
        (Ok(x), Ok(y)) => (SIG_OK, Value::int(x & y)),
        _ => type_err2("%bit-and", "integer", &args[0], &args[1]),
    }
}

fn prim_bit_or(args: &[Value]) -> (SignalBits, Value) {
    match (args[0].as_int(), args[1].as_int()) {
        (Some(x), Some(y)) => (SIG_OK, Value::int(x | y)),
        _ => type_err2("%bit-or", "integer", &args[0], &args[1]),
    }
}

fn prim_bit_xor(args: &[Value]) -> (SignalBits, Value) {
    match (args[0].as_int(), args[1].as_int()) {
        (Some(x), Some(y)) => (SIG_OK, Value::int(x ^ y)),
        _ => type_err2("%bit-xor", "integer", &args[0], &args[1]),
    }
}

fn prim_bit_not(args: &[Value]) -> (SignalBits, Value) {
    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::int(!n)),
        None => type_err("%bit-not", "integer", &args[0]),
    }
}

fn prim_shl(args: &[Value]) -> (SignalBits, Value) {
    match (args[0].as_int(), args[1].as_int()) {
        (Some(a), Some(b)) => {
            let shift = b.clamp(0, 63) as u32;
            (SIG_OK, Value::int(a << shift))
        }
        _ => type_err2("%shl", "integer", &args[0], &args[1]),
    }
}

fn prim_shr(args: &[Value]) -> (SignalBits, Value) {
    match (args[0].as_int(), args[1].as_int()) {
        (Some(a), Some(b)) => {
            let shift = b.clamp(0, 63) as u32;
            (SIG_OK, Value::int(a >> shift))
        }
        _ => type_err2("%shr", "integer", &args[0], &args[1]),
    }
}

// ── Type predicates ─────────────────────────────────────────────────

fn prim_nil_q(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::bool(args[0].is_nil()))
}

fn prim_empty_q(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::bool(args[0].is_empty_list()))
}

fn prim_bool_q(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::bool(args[0].is_bool()))
}

fn prim_int_q(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::bool(args[0].is_int()))
}

fn prim_float_q(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::bool(args[0].is_float()))
}

fn prim_string_q(args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_OK,
        Value::bool(args[0].is_string() || args[0].is_string_mut()),
    )
}

fn prim_keyword_q(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::bool(args[0].is_keyword()))
}

fn prim_symbol_q(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::bool(args[0].is_symbol()))
}

fn prim_pair_q(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::bool(args[0].is_pair()))
}

fn prim_array_q(args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_OK,
        Value::bool(args[0].is_array() || args[0].is_array_mut()),
    )
}

fn prim_struct_q(args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_OK,
        Value::bool(args[0].is_struct() || args[0].is_struct_mut()),
    )
}

fn prim_set_q(args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_OK,
        Value::bool(args[0].is_set() || args[0].is_set_mut()),
    )
}

fn prim_bytes_q(args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_OK,
        Value::bool(args[0].is_bytes() || args[0].is_bytes_mut()),
    )
}

fn prim_box_q(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::bool(args[0].is_lbox()))
}

fn prim_closure_q(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::bool(args[0].is_closure()))
}

fn prim_fiber_q(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::bool(args[0].is_fiber()))
}

fn prim_type_of(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::keyword(args[0].type_name()))
}

// ── Data access ─────────────────────────────────────────────────────

fn prim_length(args: &[Value]) -> (SignalBits, Value) {
    use unicode_segmentation::UnicodeSegmentation;
    let val = &args[0];
    let len = if val.is_empty_list() || val.is_nil() {
        0
    } else if val.is_pair() {
        match val.list_to_vec() {
            Ok(v) => v.len(),
            Err(_) => return (SIG_ERROR, error_val("type-error", "%length: improper list")),
        }
    } else if let Some(a) = val.as_array() {
        a.len()
    } else if let Some(a) = val.as_array_mut() {
        a.borrow().len()
    } else if let Some(s) = val.as_struct() {
        s.len()
    } else if let Some(s) = val.as_struct_mut() {
        s.borrow().len()
    } else if let Some(s) = val.as_set() {
        s.len()
    } else if let Some(s) = val.as_set_mut() {
        s.borrow().len()
    } else if let Some(b) = val.as_bytes() {
        b.len()
    } else if let Some(b) = val.as_bytes_mut() {
        b.borrow().len()
    } else if let Some(r) = val.with_string(|s| s.graphemes(true).count()) {
        r
    } else if let Some(buf) = val.as_string_mut() {
        let b = buf.borrow();
        match std::str::from_utf8(&b) {
            Ok(s) => s.graphemes(true).count(),
            Err(_) => {
                return (
                    SIG_ERROR,
                    error_val("type-error", "%length: @string invalid UTF-8"),
                )
            }
        }
    } else {
        return type_err("%length", "collection or string", val);
    };
    (SIG_OK, Value::int(len as i64))
}

fn prim_get(args: &[Value]) -> (SignalBits, Value) {
    crate::primitives::access::prim_get(args)
}

fn prim_put(args: &[Value]) -> (SignalBits, Value) {
    crate::primitives::access::prim_put(args)
}

fn prim_del(args: &[Value]) -> (SignalBits, Value) {
    crate::primitives::lstruct::prim_del(args)
}

fn prim_has(args: &[Value]) -> (SignalBits, Value) {
    crate::primitives::lstruct::prim_has_key(args)
}

fn prim_push(args: &[Value]) -> (SignalBits, Value) {
    let collection = &args[0];
    let value = args[1];
    if let Some(vec_ref) = collection.as_array_mut() {
        vec_ref.borrow_mut().push(value);
        (SIG_OK, *collection)
    } else if let Some(elems) = collection.as_array() {
        let mut new = elems.to_vec();
        new.push(value);
        (SIG_OK, Value::array(new))
    } else {
        type_err("%push", "array", collection)
    }
}

fn prim_pop(args: &[Value]) -> (SignalBits, Value) {
    match args[0].as_array_mut() {
        Some(arr) => match arr.borrow_mut().pop() {
            Some(v) => (SIG_OK, v),
            None => (SIG_ERROR, error_val("type-error", "%pop: empty @array")),
        },
        None => type_err("%pop", "@array", &args[0]),
    }
}

// ── Mutability ──────────────────────────────────────────────────────

fn prim_freeze(args: &[Value]) -> (SignalBits, Value) {
    let val = &args[0];
    let result = if let Some(a) = val.as_array_mut() {
        Value::array(a.borrow().clone())
    } else if let Some(t) = val.as_struct_mut() {
        let entries: Vec<_> = t.borrow().iter().map(|(k, v)| (k.clone(), *v)).collect();
        Value::struct_from_sorted(entries)
    } else if let Some(s) = val.as_set_mut() {
        Value::set(s.borrow().clone())
    } else if let Some(buf) = val.as_string_mut() {
        let b = buf.borrow();
        match std::str::from_utf8(&b) {
            Ok(s) => Value::string(s),
            Err(_) => {
                return (
                    SIG_ERROR,
                    error_val("type-error", "%freeze: @string invalid UTF-8"),
                )
            }
        }
    } else if let Some(b) = val.as_bytes_mut() {
        Value::bytes(b.borrow().clone())
    } else {
        *val // already immutable
    };
    (SIG_OK, result)
}

fn prim_thaw(args: &[Value]) -> (SignalBits, Value) {
    let val = &args[0];
    let result = if let Some(a) = val.as_array() {
        Value::array_mut(a.to_vec())
    } else if let Some(s) = val.as_struct() {
        let entries: std::collections::BTreeMap<_, _> =
            s.iter().map(|(k, v)| (k.clone(), *v)).collect();
        Value::struct_mut_from(entries)
    } else if let Some(s) = val.as_set() {
        Value::set_mut(s.iter().cloned().collect())
    } else if let Some(r) = val.with_string(|s| Value::string_mut(s.as_bytes().to_vec())) {
        r
    } else if let Some(b) = val.as_bytes() {
        Value::bytes_mut(b.to_vec())
    } else {
        *val // already mutable
    };
    (SIG_OK, result)
}

// ── Identity ────────────────────────────────────────────────────────

fn prim_identical(args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_OK,
        Value::bool(args[0].tag == args[1].tag && args[0].payload == args[1].payload),
    )
}

// ── Registration table ──────────────────────────────────────────────

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    // Arithmetic
    PrimitiveDef {
        name: "%add",
        func: prim_add,
        arity: Arity::Exact(2),
        doc: "Add two numbers",
        params: &["a", "b"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%sub",
        func: prim_sub,
        arity: Arity::Range(1, 2),
        doc: "Subtract or negate",
        params: &["a", "b?"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%mul",
        func: prim_mul,
        arity: Arity::Exact(2),
        doc: "Multiply two numbers",
        params: &["a", "b"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%div",
        func: prim_div,
        arity: Arity::Exact(2),
        doc: "Divide two numbers",
        params: &["a", "b"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%rem",
        func: prim_rem,
        arity: Arity::Exact(2),
        doc: "Remainder (sign follows dividend)",
        params: &["a", "b"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%mod",
        func: prim_mod,
        arity: Arity::Exact(2),
        doc: "Floored modulus (sign follows divisor)",
        params: &["a", "b"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    // Comparison
    PrimitiveDef {
        name: "%eq",
        func: prim_eq,
        arity: Arity::Exact(2),
        doc: "Equality",
        params: &["a", "b"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%ne",
        func: prim_ne,
        arity: Arity::Exact(2),
        doc: "Not equal",
        params: &["a", "b"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%lt",
        func: prim_lt,
        arity: Arity::Exact(2),
        doc: "Less than",
        params: &["a", "b"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%gt",
        func: prim_gt,
        arity: Arity::Exact(2),
        doc: "Greater than",
        params: &["a", "b"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%le",
        func: prim_le,
        arity: Arity::Exact(2),
        doc: "Less than or equal",
        params: &["a", "b"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%ge",
        func: prim_ge,
        arity: Arity::Exact(2),
        doc: "Greater than or equal",
        params: &["a", "b"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    // Logical
    PrimitiveDef {
        name: "%not",
        func: prim_not,
        arity: Arity::Exact(1),
        doc: "Logical not",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    // Conversion
    PrimitiveDef {
        name: "%int",
        func: prim_int,
        arity: Arity::Exact(1),
        doc: "Convert to integer",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%float",
        func: prim_float,
        arity: Arity::Exact(1),
        doc: "Convert to float",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    // Data
    PrimitiveDef {
        name: "%pair",
        func: prim_pair,
        arity: Arity::Exact(2),
        doc: "Construct a pair",
        params: &["a", "b"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%first",
        func: prim_first,
        arity: Arity::Exact(1),
        doc: "First of pair",
        params: &["p"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%rest",
        func: prim_rest,
        arity: Arity::Exact(1),
        doc: "Rest of pair",
        params: &["p"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    // Bitwise
    PrimitiveDef {
        name: "%bit-and",
        func: prim_bit_and,
        arity: Arity::Exact(2),
        doc: "Bitwise AND",
        params: &["a", "b"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%bit-or",
        func: prim_bit_or,
        arity: Arity::Exact(2),
        doc: "Bitwise OR",
        params: &["a", "b"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%bit-xor",
        func: prim_bit_xor,
        arity: Arity::Exact(2),
        doc: "Bitwise XOR",
        params: &["a", "b"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%bit-not",
        func: prim_bit_not,
        arity: Arity::Exact(1),
        doc: "Bitwise complement",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%shl",
        func: prim_shl,
        arity: Arity::Exact(2),
        doc: "Shift left",
        params: &["a", "b"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%shr",
        func: prim_shr,
        arity: Arity::Exact(2),
        doc: "Shift right",
        params: &["a", "b"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    // Type predicates
    PrimitiveDef {
        name: "%nil?",
        func: prim_nil_q,
        arity: Arity::Exact(1),
        doc: "Is nil?",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%empty?",
        func: prim_empty_q,
        arity: Arity::Exact(1),
        doc: "Is empty list?",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%bool?",
        func: prim_bool_q,
        arity: Arity::Exact(1),
        doc: "Is boolean?",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%int?",
        func: prim_int_q,
        arity: Arity::Exact(1),
        doc: "Is integer?",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%float?",
        func: prim_float_q,
        arity: Arity::Exact(1),
        doc: "Is float?",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%string?",
        func: prim_string_q,
        arity: Arity::Exact(1),
        doc: "Is string?",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%keyword?",
        func: prim_keyword_q,
        arity: Arity::Exact(1),
        doc: "Is keyword?",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%symbol?",
        func: prim_symbol_q,
        arity: Arity::Exact(1),
        doc: "Is symbol?",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%pair?",
        func: prim_pair_q,
        arity: Arity::Exact(1),
        doc: "Is pair?",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%array?",
        func: prim_array_q,
        arity: Arity::Exact(1),
        doc: "Is array?",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%struct?",
        func: prim_struct_q,
        arity: Arity::Exact(1),
        doc: "Is struct?",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%set?",
        func: prim_set_q,
        arity: Arity::Exact(1),
        doc: "Is set?",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%bytes?",
        func: prim_bytes_q,
        arity: Arity::Exact(1),
        doc: "Is bytes?",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%box?",
        func: prim_box_q,
        arity: Arity::Exact(1),
        doc: "Is box?",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%closure?",
        func: prim_closure_q,
        arity: Arity::Exact(1),
        doc: "Is closure?",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%fiber?",
        func: prim_fiber_q,
        arity: Arity::Exact(1),
        doc: "Is fiber?",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%type-of",
        func: prim_type_of,
        arity: Arity::Exact(1),
        doc: "Type as keyword",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    // Data access
    PrimitiveDef {
        name: "%length",
        func: prim_length,
        arity: Arity::Exact(1),
        doc: "Polymorphic length",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%get",
        func: prim_get,
        arity: Arity::Range(2, 3),
        doc: "Indexed/keyed access",
        params: &["coll", "key"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%put",
        func: prim_put,
        arity: Arity::Range(2, 3),
        doc: "Assoc/set element",
        params: &["coll", "key", "val"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%del",
        func: prim_del,
        arity: Arity::Exact(2),
        doc: "Dissoc/delete key",
        params: &["coll", "key"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%has?",
        func: prim_has,
        arity: Arity::Exact(2),
        doc: "Key/element exists?",
        params: &["coll", "key"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%push",
        func: prim_push,
        arity: Arity::Exact(2),
        doc: "Append element",
        params: &["arr", "val"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%pop",
        func: prim_pop,
        arity: Arity::Exact(1),
        doc: "Remove/return last element",
        params: &["arr"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    // Mutability
    PrimitiveDef {
        name: "%freeze",
        func: prim_freeze,
        arity: Arity::Exact(1),
        doc: "Mutable to immutable",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "%thaw",
        func: prim_thaw,
        arity: Arity::Exact(1),
        doc: "Immutable to mutable",
        params: &["x"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
    // Identity
    PrimitiveDef {
        name: "%identical?",
        func: prim_identical,
        arity: Arity::Exact(2),
        doc: "Pointer identity",
        params: &["a", "b"],
        category: "intrinsic",
        ..PrimitiveDef::DEFAULT
    },
];
