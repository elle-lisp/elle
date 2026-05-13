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

// xor — now implemented in Elle (src/stdlib.lisp)

/// Declarative primitive definitions for logic operations.
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "and",
        func: prim_and,
        signal: Signal::silent(),
        arity: Arity::AtLeast(0),
        doc: "Logical AND operation (non-short-circuiting function form)",
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
        doc: "Logical OR operation (non-short-circuiting function form)",
        params: &[],
        category: "logic",
        example: "(or false true)",
        aliases: &[],
    },
];
