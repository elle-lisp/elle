// audited: 2026-10-05
//! The build this rig is: its key, read from the rig's own features and the platform, and the `elle/build` primitive that answers it.
//!
//! rig/overview.md
//! docs/ratchet.md

use elle::primitives::ctx::NativeCtx;
use elle::primitives::def::{PrimitiveDef, RegionEffect};
use elle::runtime::Runtime;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_OK};
use elle::value::types::Arity;
use elle::Value;

/// The tier this rig carries, by the precedence the features take: the
/// WebAssembly backend, then the MLIR tier, then the JIT, and the interpreter
/// alone where the rig has none.
fn tier() -> &'static str {
    if cfg!(feature = "wasm") {
        "wasm"
    } else if cfg!(feature = "mlir") {
        "mlir"
    } else if cfg!(feature = "jit") {
        "jit"
    } else {
        "interp"
    }
}

/// The I/O backend this rig runs: io_uring where the feature is on and the
/// platform is Linux, and the thread pool everywhere else.
fn io() -> &'static str {
    if cfg!(all(feature = "uring", target_os = "linux")) {
        "uring"
    } else {
        "pool"
    }
}

/// The build's key: tier, I/O backend, operating system and architecture,
/// joined by `-`.
pub fn key() -> String {
    format!(
        "{}-{}-{}-{}",
        tier(),
        io(),
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

fn prim_build(ctx: &mut NativeCtx<'_>, _args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, ctx.string(key()))
}

static BUILD: PrimitiveDef = PrimitiveDef {
    name: "elle/build",
    func: prim_build,
    signal: Signal::silent(),
    arity: Arity::Exact(0),
    doc: "Return the key of the build this rig is: its tier, I/O backend, operating system and architecture, joined by `-`. A ledger row belongs to one build (docs/ratchet.md).",
    params: &[],
    category: "elle",
    example: "(elle/build)",
    effect: RegionEffect::Fresh,
    ..PrimitiveDef::DEFAULT
};

/// Register `elle/build` on `rt`, for compiled code and `eval` alike.
pub fn register(rt: &mut Runtime) {
    rt.register_primitive(&BUILD);
}
