// audited: 2026-09-29
//! `vm/config` and `vm/config-set` over the VM's runtime configuration, and the tier predicates.
//!
//! docs/config.md

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
/// The VM applies it (`handle_vm_config_set`) and raises what it refuses.
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
              Pass a keyword (:jit, :mlir, :trace, :stats, :max-depth, :unicode) to \
              read one field; a tier threshold reads nil when the tier is off.",
        params: &["key?"],
        category: "meta",
        example: "(vm/config :jit)",
        effect: RegionEffect::Fresh,
    }
    "vm/config-set" => prim_vm_config_set {
        signal: Signal::query_errors(),
        arity: Arity::Exact(2),
        doc: "Set a runtime configuration field: :jit or :mlir to a positive \
              threshold, :trace to a keyword set, :max-depth to a positive integer. \
              Raises on a field or a value it refuses.",
        params: &["key", "value"],
        category: "meta",
        example: "(vm/config-set :trace |:call|)",
        effect: RegionEffect::Fresh,
    }
}
