// audited: 2026-11-18
//! The `sort` primitive: a list, array or @array put into the one order of all values.
//!
//! docs/types.md

use crate::primitives::def::RegionEffect;
use crate::primitives::seq::seq_sort;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// Sort a collection into the order `compare` reports.
///
/// Type-preserving: @arrays mutated in place, arrays and lists return new sorted values.
pub(crate) fn prim_sort(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match seq_sort(&args[0], ctx) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

primitive! {
    "sort" => prim_sort {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Sort a collection in ascending order, the order compare reports. Type-preserving: @arrays mutated in place, arrays and lists return new sorted values.",
        params: &["coll"],
        category: "collection",
        example: "(sort @[3 1 2]) #=> @[1 2 3]\n(sort [[1 3] [1 2]]) #=> [[1 2] [1 3]]",
        effect: RegionEffect::Funnel,
    }
}
