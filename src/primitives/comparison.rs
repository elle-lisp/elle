//! Comparison primitives
use crate::arithmetic::values_eq;
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Equality comparison — numeric-aware and chained.
/// If both values are numbers, compares numerically (int 1 == float 1.0).
/// Otherwise, uses structural equality (PartialEq).
/// Chained: (= a b c) means all pairs are equal.
pub(crate) fn prim_eq(args: &[Value]) -> (SignalBits, Value) {
    for i in 0..args.len() - 1 {
        if !values_eq(&args[i], &args[i + 1]) {
            return (SIG_OK, Value::FALSE);
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
