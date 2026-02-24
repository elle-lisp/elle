use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Logical NOT operation
pub fn prim_not(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("not: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    (SIG_OK, Value::bool(!args[0].is_truthy()))
}

/// Logical AND operation
/// (and) => true
/// (and x) => x
/// (and x y z) => z if all truthy, else first falsy
pub fn prim_and(args: &[Value]) -> (SignalBits, Value) {
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
pub fn prim_or(args: &[Value]) -> (SignalBits, Value) {
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
pub fn prim_xor(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() {
        return (SIG_OK, Value::bool(false));
    }

    // Count truthy values, return true if odd
    let truthy_count = args.iter().filter(|v| v.is_truthy()).count();
    (SIG_OK, Value::bool(truthy_count % 2 == 1))
}

/// Declarative primitive definitions for logic operations.
pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "not",
        func: prim_not,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Logical NOT operation",
        params: &["x"],
        category: "",
        example: "(not true)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "and",
        func: prim_and,
        effect: Effect::none(),
        arity: Arity::AtLeast(0),
        doc: "Logical AND operation",
        params: &[],
        category: "",
        example: "(and true false)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "or",
        func: prim_or,
        effect: Effect::none(),
        arity: Arity::AtLeast(0),
        doc: "Logical OR operation",
        params: &[],
        category: "",
        example: "(or false true)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "xor",
        func: prim_xor,
        effect: Effect::none(),
        arity: Arity::AtLeast(0),
        doc: "Logical XOR operation",
        params: &[],
        category: "",
        example: "(xor true false)",
        aliases: &[],
    },
];
