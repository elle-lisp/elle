//! Force-dispatch a closure on a specific compilation tier.
//!
//! Powers the `compile/run-on` primitive used by `lib/differential.lisp`
//! to verify that the same closure produces the same result on every
//! tier that accepts it.
//!
//! Tiers:
//! - `:bytecode` — pure interpreter (this closure's code is interpreted;
//!   nested calls still go through normal tier dispatch)
//! - `:jit` — force-compiles via Cranelift, then dispatches to native code
//! - `:mlir-cpu` — force-compiles via MLIR + LLVM, dispatches via the
//!   `MlirCache` (only available with `--features mlir`)
//!
//! Each entry point returns `(SignalBits, Value)`. Tier ineligibility
//! surfaces as a structured `:tier-rejected` error so callers can skip
//! the tier rather than failing.
//!
//! One `impl VM` method per tier lives in its own submodule (`bytecode`,
//! `jit`, `wasm`, `mlir`); the wasm/mlir tiers are compiled only under their
//! respective features. Callers reach these inherent methods by method-call
//! syntax, so no re-export is needed — only the shared `rejected` helper,
//! which stays here and is visible to the tier submodules as `super::rejected`.

use crate::value::Value;

use super::core::VM;

mod bytecode;
mod jit;
#[cfg(feature = "mlir")]
mod mlir;
#[cfg(feature = "wasm")]
mod wasm;

/// Build a structured `:tier-rejected` error, born in a fresh region of its own
/// ([`VM::error_extra`]) — `vm` owns the heap that mints it.
fn rejected(vm: &mut VM, tier: &str, msg: impl Into<String>) -> Value {
    vm.error_extra(
        "tier-rejected",
        msg,
        &[
            // The two field names are keyword spellings; `vocab` fails the
            // build if the vocabulary stops carrying one, the same guard
            // `rich_error!` applies to the names it stringifies.
            (
                const { crate::value::keyword::vocab("tier") },
                Value::keyword(tier),
            ),
            (
                const { crate::value::keyword::vocab("reason") },
                Value::keyword("ineligible"),
            ),
        ],
    )
}
