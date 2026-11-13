// audited: 2026-11-12
//! The traffic gauges: how often a heap's counts moved, one integer per primitive.
//!
//! docs/impl/region/diagnostics.md

use crate::primitives::ctx::NativeCtx;
use crate::primitives::def::{RegionEffect, RetType};
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::Value;

/// (arena/increfs) — every increment the heap's store ran.
pub(crate) fn prim_increfs(ctx: &mut NativeCtx<'_>, _args: &[Value]) -> (SignalBits, Value) {
    let n = ctx.heap_mut().traffic().increfs;
    (SIG_OK, Value::int(n as i64))
}

/// (arena/decrefs) — every release the program ran, the free cascade's left out.
pub(crate) fn prim_decrefs(ctx: &mut NativeCtx<'_>, _args: &[Value]) -> (SignalBits, Value) {
    let n = ctx.heap_mut().traffic().decrefs;
    (SIG_OK, Value::int(n as i64))
}

primitive! {
    "debug/arena-increfs" => prim_increfs {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return the reference-count increments the heap ran, inert ones on owned regions included (monotonic).",
        category: "debug",
        example: "(debug/arena-increfs)",
        aliases: &["arena/increfs"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-decrefs" => prim_decrefs {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return the releases the program ran, the free cascade's left out (monotonic).",
        category: "debug",
        example: "(debug/arena-decrefs)",
        aliases: &["arena/decrefs"],
        effect: RegionEffect::Immediate,
    }
}
