//! Comparison primitives
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;
use std::cmp::Ordering;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Equality comparison — numeric-aware and chained.
/// If both values are numbers, compares numerically (int 1 == float 1.0).
/// Otherwise, uses structural equality (PartialEq).
/// Chained: (= a b c) means all pairs are equal.
pub(crate) fn prim_eq(args: &[Value]) -> (SignalBits, Value) {
    for i in 0..args.len() - 1 {
        // Fast path: bitwise identical (covers same-type immediates)
        if args[i] == args[i + 1] {
            continue;
        }
        // Numeric coercion: int-int stays exact, mixed promotes to f64
        if args[i].is_number() && args[i + 1].is_number() {
            if let (Some(a), Some(b)) = (args[i].as_int(), args[i + 1].as_int()) {
                if a == b {
                    continue;
                } else {
                    return (SIG_OK, Value::FALSE);
                }
            }
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

/// Inequality comparison — negation of `=`.
/// Numeric-aware: (not= 1 1.0) is false. Accepts exactly 2 arguments.
pub(crate) fn prim_not_eq(args: &[Value]) -> (SignalBits, Value) {
    // Fast path: bitwise identical
    if args[0] == args[1] {
        return (SIG_OK, Value::FALSE);
    }
    // Numeric coercion: if both are numbers, compare as f64
    if args[0].is_number() && args[1].is_number() {
        if let (Some(a), Some(b)) = (args[0].as_number(), args[1].as_number()) {
            if a == b {
                return (SIG_OK, Value::FALSE);
            }
        }
    }
    (SIG_OK, Value::TRUE)
}

/// Strict identity comparison — bitwise/structural equality with no coercion.
/// This is what `=` used to be: (identical? 1 1.0) is false.
pub(crate) fn prim_identical(args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_OK,
        if args[0] == args[1] {
            Value::TRUE
        } else {
            Value::FALSE
        },
    )
}

/// Three-way comparison using the total Value ordering.
/// Returns -1 if a < b, 0 if a == b, 1 if a > b.
/// Uses the same ordering as `sort` (Value::Ord).
///
/// Signal is errors() even though Value::Ord is currently total.
/// This is intentional: if the type system ever introduces incomparable
/// values, callers that assumed compare is pure would silently misbehave.
/// Declaring errors() keeps the contract honest.
pub(crate) fn prim_compare(args: &[Value]) -> (SignalBits, Value) {
    let result: i64 = match args[0].cmp(&args[1]) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    };
    (SIG_OK, Value::int(result))
}

/// Hash any value to an integer using DefaultHasher.
pub(crate) fn prim_hash(args: &[Value]) -> (SignalBits, Value) {
    let mut hasher = DefaultHasher::new();
    args[0].hash(&mut hasher);
    (SIG_OK, Value::int(hasher.finish() as i64))
}

/// Declarative primitive definitions for comparison functions.
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "=",
        func: prim_eq,
        signal: Signal::errors(),
        arity: Arity::AtLeast(2),
        doc: "Test equality of values. Numeric-aware: (= 1 1.0) is true. Chained: (= a b c) means all are equal.",
        params: &["a", "b"],
        category: "comparison",
        example: "(= 1 1) #=> true\n(= 1 1.0) #=> true\n(= 1 2 1) #=> false",
        aliases: &["eq?"],
    },
    PrimitiveDef {
        name: "not=",
        func: prim_not_eq,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Test inequality of values. Numeric-aware: (not= 1 1.0) is false. Returns true if the two values are not equal.",
        params: &["a", "b"],
        category: "comparison",
        example: "(not= 1 2) #=> true\n(not= 1 1) #=> false\n(not= 1 1.0) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "identical?",
        func: prim_identical,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Test strict identity. No numeric coercion: (identical? 1 1.0) is false.",
        params: &["a", "b"],
        category: "comparison",
        example: "(identical? 1 1) #=> true\n(identical? 1 1.0) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "compare",
        func: prim_compare,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Three-way comparison. Returns -1 if a < b, 0 if a = b, 1 if a > b. Uses the same total ordering as sort. Useful for writing comparators: (sort-with (fn (a b) (compare b a)) coll) sorts descending.",
        params: &["a", "b"],
        category: "comparison",
        example: "(compare 1 2) #=> -1\n(compare 2 2) #=> 0\n(compare 3 2) #=> 1\n(compare \"a\" \"b\") #=> -1",
        aliases: &[],
    },
    PrimitiveDef {
        name: "hash",
        func: prim_hash,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Hash any value to an integer. Equal values produce equal hashes. Uses the same structural hashing as hash-map/hash-set internals.",
        params: &["value"],
        category: "comparison",
        example: "(hash 42) #=> <integer>\n(= (hash :foo) (hash :foo)) #=> true",
        aliases: &[],
    },
];
