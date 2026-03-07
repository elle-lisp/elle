use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::heap::TableKey;
use crate::value::types::Arity;
use crate::value::{error_val, Value};
use std::collections::BTreeMap;

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

/// Assert that a value is truthy
/// (assert value) => value if truthy, else signal error
/// (assert value message) => value if truthy, else signal error with message
pub fn prim_assert(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("assert: expected 1-2 arguments, got {}", args.len()),
            ),
        );
    }

    let value = args[0];
    let message = if args.len() == 2 { args[1] } else { Value::NIL };

    if value.is_truthy() {
        // Pass through the value
        (SIG_OK, value)
    } else {
        // Signal error with {:error :failed-assertion :message msg}
        let mut fields = BTreeMap::new();
        fields.insert(
            TableKey::Keyword("error".into()),
            Value::keyword("failed-assertion"),
        );
        fields.insert(TableKey::Keyword("message".into()), message);
        (SIG_ERROR, Value::struct_from(fields))
    }
}

/// Declarative primitive definitions for logic operations.
pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "not",
        func: prim_not,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Logical NOT operation",
        params: &["x"],
        category: "logic",
        example: "(not true)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "and",
        func: prim_and,
        effect: Effect::inert(),
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
        effect: Effect::inert(),
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
        effect: Effect::inert(),
        arity: Arity::AtLeast(0),
        doc: "Logical XOR operation",
        params: &[],
        category: "logic",
        example: "(xor true false)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "assert",
        func: prim_assert,
        effect: Effect::errors(),
        arity: Arity::Range(1, 2),
        doc: "Assert that value is truthy. Signals {:error :failed-assertion :message msg} if not. Returns value if truthy.",
        params: &["value", "message?"],
        category: "control",
        example: "(assert true)\n(assert (> x 0) \"x must be positive\")",
        aliases: &[],
    },
];
