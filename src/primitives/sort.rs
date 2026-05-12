//! Sort primitives
use crate::primitives::def::PrimitiveDef;
use crate::primitives::seq::seq_sort;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// Sort a collection in ascending order using the built-in value ordering.
///
/// Type-preserving: @arrays mutated in place, arrays and lists return new sorted values.
/// Supports any comparable values via Value::Ord.
pub(crate) fn prim_sort(args: &[Value]) -> (SignalBits, Value) {
    match seq_sort(&args[0]) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "sort",
        func: prim_sort,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Sort a collection in ascending order using the built-in value ordering. Type-preserving: @arrays mutated in place, arrays and lists return new sorted values.",
        params: &["coll"],
        category: "collection",
        example: "(sort @[3 1 2]) #=> @[1 2 3]\n(sort [\"b\" \"a\" \"c\"]) #=> [\"a\" \"b\" \"c\"]",
        aliases: &[],
    },
];
