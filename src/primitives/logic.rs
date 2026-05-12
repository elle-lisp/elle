use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// Logical AND operation
/// (and) => true
/// (and x) => x
/// (and x y z) => z if all truthy, else first falsy
pub(crate) fn prim_and(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() {
        return (SIG_OK, Value::bool(true));
    }

    // Short-circuit truthiness AND
    for arg in &args[..args.len() - 1] {
        if !arg.is_truthy() {
            return (SIG_OK, *arg);
        }
    }

    (SIG_OK, args[args.len() - 1])
}

/// Logical OR operation
/// (or) => false
/// (or x) => x
/// (or x y z) => x if truthy, else next truthy or z
pub(crate) fn prim_or(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() {
        return (SIG_OK, Value::bool(false));
    }

    // Short-circuit truthiness OR
    for arg in &args[..args.len() - 1] {
        if arg.is_truthy() {
            return (SIG_OK, *arg);
        }
    }

    (SIG_OK, args[args.len() - 1])
}

/// Logical XOR operation
/// (xor) => false
/// (xor x) => x as bool
/// (xor x y z) => true if odd number of truthy values, else false
pub(crate) fn prim_xor(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() {
        return (SIG_OK, Value::bool(false));
    }

    // Count truthy values, return true if odd
    let truthy_count = args.iter().filter(|v| v.is_truthy()).count();
    (SIG_OK, Value::bool(truthy_count % 2 == 1))
}

/// Declarative primitive definitions for logic operations.
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "and",
        func: prim_and,
        signal: Signal::silent(),
        arity: Arity::AtLeast(0),
        doc: "Logical AND operation",
        params: &[],
        category: "logic",
        example: "(and true false)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "or",
        func: prim_or,
        signal: Signal::silent(),
        arity: Arity::AtLeast(0),
        doc: "Logical OR operation",
        params: &[],
        category: "logic",
        example: "(or false true)",
        aliases: &[],
    },

    PrimitiveDef {
        name: "xor",
        func: prim_xor,
        signal: Signal::silent(),
        arity: Arity::AtLeast(0),
        doc: "Logical XOR operation",
        params: &[],
        category: "logic",
        example: "(xor true false)",
        aliases: &[],
    },
];
