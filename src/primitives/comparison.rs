//! Comparison primitives
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};
use std::cmp::Ordering;

/// Chained comparison helper. Compares adjacent pairs with short-circuit.
/// Supports numbers, strings, and keywords.
fn chain_cmp(
    name: &str,
    args: &[Value],
    cmp_int: fn(i64, i64) -> bool,
    cmp_float: fn(f64, f64) -> bool,
    cmp_ord: fn(Ordering) -> bool,
) -> (SignalBits, Value) {
    if args.len() < 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "{}: expected at least 2 arguments, got {}",
                    name,
                    args.len()
                ),
            ),
        );
    }
    for i in 0..args.len() - 1 {
        let result = match (args[i].as_int(), args[i + 1].as_int()) {
            (Some(a), Some(b)) => cmp_int(a, b),
            _ => match (args[i].as_number(), args[i + 1].as_number()) {
                (Some(a), Some(b)) => cmp_float(a, b),
                _ => {
                    if let Some(ord) = args[i].compare_str(&args[i + 1]) {
                        cmp_ord(ord)
                    } else if let Some(ord) = args[i].compare_keyword(&args[i + 1]) {
                        cmp_ord(ord)
                    } else {
                        return (
                            SIG_ERROR,
                            error_val(
                                "type-error",
                                format!(
                                    "{}: expected number, string, or keyword, got {} and {}",
                                    name,
                                    args[i].type_name(),
                                    args[i + 1].type_name()
                                ),
                            ),
                        );
                    }
                }
            },
        };
        if !result {
            return (SIG_OK, Value::FALSE);
        }
    }
    (SIG_OK, Value::TRUE)
}

/// Equality comparison — numeric-aware and chained.
/// If both values are numbers, compares numerically (int 1 == float 1.0).
/// Otherwise, uses structural equality (PartialEq).
/// Chained: (= a b c) means all pairs are equal.
pub(crate) fn prim_eq(args: &[Value]) -> (SignalBits, Value) {
    if args.len() < 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("=: expected at least 2 arguments, got {}", args.len()),
            ),
        );
    }
    for i in 0..args.len() - 1 {
        // Fast path: bitwise identical (covers same-type immediates)
        if args[i] == args[i + 1] {
            continue;
        }
        // Numeric coercion: if both are numbers, compare as f64
        if args[i].is_number() && args[i + 1].is_number() {
            if let (Some(a), Some(b)) = (args[i].as_number(), args[i + 1].as_number()) {
                if a == b {
                    continue;
                } else {
                    return (SIG_OK, Value::FALSE);
                }
            }
        }
        return (SIG_OK, Value::FALSE);
    }
    (SIG_OK, Value::TRUE)
}

/// Strict identity comparison — bitwise/structural equality with no coercion.
/// This is what `=` used to be: (identical? 1 1.0) is false.
pub(crate) fn prim_identical(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("identical?: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    (
        SIG_OK,
        if args[0] == args[1] {
            Value::TRUE
        } else {
            Value::FALSE
        },
    )
}

/// Less than comparison (chained)
pub(crate) fn prim_lt(args: &[Value]) -> (SignalBits, Value) {
    chain_cmp("<", args, |a, b| a < b, |a, b| a < b, |ord| ord.is_lt())
}

/// Greater than comparison (chained)
pub(crate) fn prim_gt(args: &[Value]) -> (SignalBits, Value) {
    chain_cmp(">", args, |a, b| a > b, |a, b| a > b, |ord| ord.is_gt())
}

/// Less than or equal comparison (chained)
pub(crate) fn prim_le(args: &[Value]) -> (SignalBits, Value) {
    chain_cmp("<=", args, |a, b| a <= b, |a, b| a <= b, |ord| ord.is_le())
}

/// Greater than or equal comparison (chained)
pub(crate) fn prim_ge(args: &[Value]) -> (SignalBits, Value) {
    chain_cmp(">=", args, |a, b| a >= b, |a, b| a >= b, |ord| ord.is_ge())
}

/// Declarative primitive definitions for comparison functions.
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "=",
        func: prim_eq,
        effect: Signal::inert(),
        arity: Arity::AtLeast(2),
        doc: "Test equality of values. Numeric-aware: (= 1 1.0) is true. Chained: (= a b c) means all are equal.",
        params: &["a", "b"],
        category: "comparison",
        example: "(= 1 1) #=> true\n(= 1 1.0) #=> true\n(= 1 2 1) #=> false",
        aliases: &["eq?"],
    },
    PrimitiveDef {
        name: "identical?",
        func: prim_identical,
        effect: Signal::inert(),
        arity: Arity::Exact(2),
        doc: "Test strict identity. No numeric coercion: (identical? 1 1.0) is false.",
        params: &["a", "b"],
        category: "comparison",
        example: "(identical? 1 1) #=> true\n(identical? 1 1.0) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "<",
        func: prim_lt,
        effect: Signal::inert(),
        arity: Arity::AtLeast(2),
        doc: "Test strictly ascending order. Chained: (< a b c) means a < b and b < c. Works on numbers, strings, and keywords.",
        params: &["a", "b"],
        category: "comparison",
        example: "(< 1 2 3) #=> true\n(< \"a\" \"b\" \"c\") #=> true\n(< :apple :banana :cherry) #=> true",
        aliases: &[],
    },
    PrimitiveDef {
        name: ">",
        func: prim_gt,
        effect: Signal::inert(),
        arity: Arity::AtLeast(2),
        doc: "Test strictly descending order. Chained: (> c b a) means c > b and b > a. Works on numbers, strings, and keywords.",
        params: &["a", "b"],
        category: "comparison",
        example: "(> 3 2 1) #=> true\n(> \"c\" \"b\" \"a\") #=> true\n(> :cherry :banana :apple) #=> true",
        aliases: &[],
    },
    PrimitiveDef {
        name: "<=",
        func: prim_le,
        effect: Signal::inert(),
        arity: Arity::AtLeast(2),
        doc: "Test non-descending order. Chained: (<= a b c) means a <= b and b <= c. Works on numbers, strings, and keywords.",
        params: &["a", "b"],
        category: "comparison",
        example: "(<= 1 2 2 3) #=> true\n(<= \"a\" \"b\" \"b\" \"c\") #=> true",
        aliases: &[],
    },
    PrimitiveDef {
        name: ">=",
        func: prim_ge,
        effect: Signal::inert(),
        arity: Arity::AtLeast(2),
        doc: "Test non-ascending order. Chained: (>= c b a) means c >= b and b >= a. Works on numbers, strings, and keywords.",
        params: &["a", "b"],
        category: "comparison",
        example: "(>= 3 2 2 1) #=> true\n(>= \"c\" \"b\" \"b\" \"a\") #=> true",
        aliases: &[],
    },
];
