//! `vm/config` primitive for runtime configuration access.
//!
//! Provides Elle-level access to the VM's RuntimeConfig via SIG_QUERY.
//! - `(vm/config)` — returns the full config as a struct
//! - `(vm/config :trace)` — returns the trace keyword set
//! - `(vm/config :jit)` — returns the JIT policy keyword
//! - `(vm/config :wasm)` — returns the WASM policy keyword
//! - `(put (vm/config) :trace |:call :signal|)` — sets trace keywords
//! - `(put (vm/config) :jit :eager)` — sets JIT policy

use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_OK, SIG_QUERY};
use crate::value::types::Arity;
use crate::value::Value;

/// `(vm/config)` or `(vm/config key)` — read runtime configuration.
///
/// With no args: returns the full config as a struct.
/// With a keyword arg: returns the value of that config field.
pub(crate) fn prim_vm_config(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args.is_empty() {
        // Return full config — SIG_QUERY "vm/config" nil
        (SIG_QUERY, ctx.pair(Value::keyword("vm/config"), Value::NIL))
    } else {
        // Return specific field — SIG_QUERY "vm/config" key
        (SIG_QUERY, ctx.pair(Value::keyword("vm/config"), args[0]))
    }
}

/// `(vm/config-set key value)` — set a runtime configuration field.
///
/// This is the internal setter called from struct `put` dispatch.
/// The analyzer rewrites `(put (vm/config) :trace ...)` to this.
/// For now, we use SIG_QUERY for both read and write.
pub(crate) fn prim_vm_config_set(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // SIG_QUERY "vm/config-set" (key . value)
    let inner = ctx.pair(args[0], args[1]);
    (SIG_QUERY, ctx.pair(Value::keyword("vm/config-set"), inner))
}

/// `(vm/tier)` — the backend tier currently executing.
///
/// Returns a keyword (`:bytecode`, `:jit`, `:wasm`, `:mlir-cpu`). Under
/// `compile/run-on` it reflects the forced tier the driving VM recorded;
/// otherwise it is `:bytecode`. Read from `ctx.vm().active_tier`, so a closure
/// compiled once and dispatched to several tiers learns which one it is running
/// on *at runtime*.
pub(crate) fn prim_vm_tier(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, Value::keyword(ctx.vm().active_tier))
}

/// `(backend? :tier)` — true iff `:tier` is the currently executing tier.
///
/// Runtime predicate (not compile-time): the same closure runs on every
/// tier via `compile/run-on`, so the answer depends on the active tier,
/// which is only known at runtime. A non-keyword argument yields false.
/// Drives the test runner's `gate!` and cross-tier divergence fixtures.
pub(crate) fn prim_backend_q(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let active = ctx.vm().active_tier;
    let matches = args[0].is_keyword_named(active);
    (SIG_OK, Value::bool(matches))
}

// Declarative primitive definitions for config operations.
primitive! {
    "vm/tier" => prim_vm_tier {
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "The backend tier currently executing, as a keyword \
              (:bytecode, :jit, :wasm, :mlir-cpu). Reflects the tier forced \
              by compile/run-on; :bytecode otherwise.",
        params: &[],
        category: "meta",
        example: "(vm/tier) #=> :bytecode",
        effect: RegionEffect::Immediate,
    }
    "backend?" => prim_backend_q {
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "True iff the given tier keyword is the currently executing tier. \
              Runtime predicate, since one closure runs on every tier under \
              compile/run-on. Used by the test runner's gate! and divergence fixtures.",
        params: &["tier"],
        category: "meta",
        example: "(backend? :jit)",
        effect: RegionEffect::Immediate,
    }
    "vm/config" => prim_vm_config {
        signal: Signal::query_errors(),
        arity: Arity::Range(0, 1),
        doc: "Read runtime configuration. No args returns the full config struct. \
              Pass a keyword (:trace, :jit, :wasm, :stats) to read a specific field.",
        params: &["key?"],
        category: "meta",
        example: "(vm/config :jit)",
        effect: RegionEffect::Fresh,
    }
    "vm/config-set" => prim_vm_config_set {
        signal: Signal::query_errors(),
        arity: Arity::Exact(2),
        doc: "Set a runtime configuration field. Use (put (vm/config) :key value) instead.",
        params: &["key", "value"],
        category: "meta",
        example: "(vm/config-set :jit :eager)",
        effect: RegionEffect::Fresh,
    }
}
