//! `vm/config` primitive for runtime configuration access.
//!
//! Provides Elle-level access to the VM's RuntimeConfig via SIG_QUERY.
//! - `(vm/config)` — returns the full config as a struct
//! - `(vm/config :trace)` — returns the trace keyword set
//! - `(vm/config :jit)` — returns the JIT policy keyword
//! - `(vm/config :wasm)` — returns the WASM policy keyword
//! - `(put (vm/config) :trace |:call :signal|)` — sets trace keywords
//! - `(put (vm/config) :jit :eager)` — sets JIT policy

use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_QUERY};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// `(vm/config)` or `(vm/config key)` — read runtime configuration.
///
/// With no args: returns the full config as a struct.
/// With a keyword arg: returns the value of that config field.
pub(crate) fn prim_vm_config(args: &[Value]) -> (SignalBits, Value) {
    match args.len() {
        0 => {
            // Return full config — SIG_QUERY "vm/config" nil
            (
                SIG_QUERY,
                Value::pair(Value::keyword("vm/config"), Value::NIL),
            )
        }
        1 => {
            // Return specific field — SIG_QUERY "vm/config" key
            (SIG_QUERY, Value::pair(Value::keyword("vm/config"), args[0]))
        }
        _ => (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("vm/config: expected 0-1 arguments, got {}", args.len()),
            ),
        ),
    }
}

/// `(vm/config-set key value)` — set a runtime configuration field.
///
/// This is the internal setter called from struct `put` dispatch.
/// The analyzer rewrites `(put (vm/config) :trace ...)` to this.
/// For now, we use SIG_QUERY for both read and write.
pub(crate) fn prim_vm_config_set(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("vm/config-set: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    // SIG_QUERY "vm/config-set" (key . value)
    (
        SIG_QUERY,
        Value::pair(
            Value::keyword("vm/config-set"),
            Value::pair(args[0], args[1]),
        ),
    )
}

/// Declarative primitive definitions for config operations.
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "vm/config",
        func: prim_vm_config,
        signal: Signal {
            bits: SIG_QUERY.union(SIG_ERROR),
            propagates: 0,
        },
        arity: Arity::Range(0, 1),
        doc: "Read runtime configuration. No args returns the full config struct. \
              Pass a keyword (:trace, :jit, :wasm, :stats) to read a specific field.",
        params: &["key?"],
        category: "meta",
        example: "(vm/config :jit)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "vm/config-set",
        func: prim_vm_config_set,
        signal: Signal {
            bits: SIG_QUERY.union(SIG_ERROR),
            propagates: 0,
        },
        arity: Arity::Exact(2),
        doc: "Set a runtime configuration field. Use (put (vm/config) :key value) instead.",
        params: &["key", "value"],
        category: "meta",
        example: "(vm/config-set :jit :eager)",
        aliases: &[],
    },
];
