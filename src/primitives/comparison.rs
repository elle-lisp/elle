// audited: 2026-09-23
//! The equality primitives: numeric-aware `=`, coercion-free `identical?`, and `hash`.
//!
//! docs/types.md

use crate::arithmetic::values_eq;
use crate::primitives::def::RegionEffect;
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
pub(crate) fn prim_eq(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    for i in 0..args.len() - 1 {
        if !values_eq(&args[i], &args[i + 1]) {
            return (SIG_OK, Value::FALSE);
        }
    }
    (SIG_OK, Value::TRUE)
}

/// Structural equality with no coercion: `Value`'s `PartialEq`, so two
/// separately built collections with equal contents compare true, and floats
/// compare by bit pattern. (identical? 1 1.0) is false. The `%identical?`
/// intrinsic is the pointer-identity test.
pub(crate) fn prim_identical(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
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
pub(crate) fn prim_hash(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let mut hasher = DefaultHasher::new();
    args[0].hash(&mut hasher);
    (SIG_OK, Value::int(hasher.finish() as i64))
}

// Declarative primitive definitions for comparison functions.
primitive! {
    "=" => prim_eq {
        signal: Signal::errors(),
        arity: Arity::AtLeast(2),
        doc: "Test equality of values. Numeric-aware at every depth: (= 1 1.0) and (= [1] [1.0]) are true; IEEE NaN is never equal, even inside collections. Chained: (= a b c) means all are equal.",
        params: &["a", "b"],
        category: "comparison",
        example: "(= 1 1) #=> true\n(= 1 1.0) #=> true\n(= 1 2 1) #=> false",
        aliases: &["eq?"],
        effect: RegionEffect::Immediate,
    }
    "identical?" => prim_identical {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Test equality with no coercion. Values of the same type with equal contents are identical, even two separate allocations; floats compare by bit pattern, so NaN is identical to NaN. (identical? 1 1.0) is false. For pointer identity use %identical?.",
        params: &["a", "b"],
        category: "comparison",
        example: "(identical? 1 1) #=> true\n(identical? 1 1.0) #=> false\n(identical? @[1] @[1]) #=> true",
        effect: RegionEffect::Immediate,
    }
    "hash" => prim_hash {
        arity: Arity::Exact(1),
        doc: "Hash any value to an integer. Equal values produce equal hashes.",
        params: &["value"],
        category: "comparison",
        example: "(hash 42) #=> <integer>\n(= (hash :foo) (hash :foo)) #=> true",
        effect: RegionEffect::Immediate,
    }
}
