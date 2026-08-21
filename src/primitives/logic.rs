use crate::primitives::def::RegionEffect;
use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// Logical AND operation
/// (and) => true
/// (and x) => x
/// (and x y z) => z if all truthy, else first falsy
pub(crate) fn prim_and(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
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
pub(crate) fn prim_or(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
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

primitive! {
    "and" => prim_and {
        arity: Arity::AtLeast(0),
        doc: "Logical AND operation (non-short-circuiting function form)",
        category: "logic", example: "(and true false)",
        effect: RegionEffect::PassThrough,
    }
    "or" => prim_or {
        arity: Arity::AtLeast(0),
        doc: "Logical OR operation (non-short-circuiting function form)",
        category: "logic", example: "(or false true)",
        effect: RegionEffect::PassThrough,
    }
}
