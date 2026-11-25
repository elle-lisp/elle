// audited: 2026-11-23
//! The `jit/stats` primitive: the JIT compiles the calling VM has taken, the rejected ones apart, and their wall and CPU time.
//!
//! rig/overview.md
//! docs/impl/jit.md

use elle::primitives::ctx::NativeCtx;
use elle::primitives::def::{PrimitiveDef, RegionEffect};
use elle::runtime::Runtime;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_OK};
use elle::value::heap::TableKey;
use elle::value::types::Arity;
use elle::Value;
use std::collections::BTreeMap;

/// The calling VM's counts as `[compiles rejected wall-us cpu-us]`, once it
/// has taken every compile its worker finished. A build without the JIT has
/// taken none.
#[cfg(feature = "jit")]
fn counts(ctx: &mut NativeCtx<'_>) -> [i64; 4] {
    let s = ctx.vm().poll_jit_stats();
    [
        s.compiles() as i64,
        s.rejected() as i64,
        s.wall().as_micros() as i64,
        s.cpu().as_micros() as i64,
    ]
}

#[cfg(not(feature = "jit"))]
fn counts(_ctx: &mut NativeCtx<'_>) -> [i64; 4] {
    [0; 4]
}

fn prim_jit_stats(ctx: &mut NativeCtx<'_>, _args: &[Value]) -> (SignalBits, Value) {
    let counts = counts(ctx);
    let mut fields = BTreeMap::new();
    for (name, n) in ["compiles", "rejected", "wall-us", "cpu-us"]
        .into_iter()
        .zip(counts)
    {
        // `keyword` records the spelling, which the user build's vocabulary
        // does not hold, so the struct prints its keys by name.
        let key = TableKey::from_value(&ctx.keyword(name)).expect("a keyword is a table key");
        fields.insert(key, Value::int(n));
    }
    (SIG_OK, ctx.struct_from(fields))
}

static JIT_STATS: PrimitiveDef = PrimitiveDef {
    name: "jit/stats",
    func: prim_jit_stats,
    signal: Signal::ambient(),
    arity: Arity::Exact(0),
    doc: "Return the JIT compiles this VM has taken as {:compiles :rejected :wall-us :cpu-us}: how many, how many produced no code, and their wall and CPU time in microseconds. It takes every compile the worker finished and waits for none (rig/overview.md).",
    params: &[],
    category: "elle",
    example: "(jit/stats)",
    effect: RegionEffect::Fresh,
    ..PrimitiveDef::DEFAULT
};

/// Register `jit/stats` on `rt`, for compiled code and `eval` alike.
pub fn register(rt: &mut Runtime) {
    rt.register_primitive(&JIT_STATS);
}
