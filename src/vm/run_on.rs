// audited: 2026-09-29
//! `compile/run-on`: force a closure onto one execution tier, and answer a
//! structured `:tier-rejected` error when that tier refuses it.
//!
//! docs/impl/differential.md
//!
//! One `impl VM` method per tier lives in its own submodule (`bytecode`,
//! `jit`, `wasm`, `mlir`); the jit, wasm and mlir tiers compile only under
//! their features, and a build without one answers `:feature-disabled` here.
//! Callers reach those inherent methods by method-call syntax, so
//! nothing is re-exported. The shared `rejected` helper stays here, where the
//! tier submodules see it as `super::rejected`.

use crate::value::{SignalBits, Value, SIG_ERROR};

use super::core::VM;

mod bytecode;
#[cfg(feature = "jit")]
mod jit;
#[cfg(feature = "mlir")]
mod mlir;
#[cfg(test)]
mod tests;
#[cfg(feature = "wasm")]
mod wasm;

impl VM {
    /// `(compile/run-on tier closure & args)`: route `arg`, the list
    /// `(tier closure arg1 arg2 ...)`, to the tier's `invoke_closure_*`.
    pub(in crate::vm) fn dispatch_compile_run_on(
        &mut self,
        ctx: &mut crate::primitives::ctx::Alloc,
        arg: Value,
    ) -> (SignalBits, Value) {
        let parts = match arg.list_to_vec_in(ctx.heap_mut()) {
            Ok(v) => v,
            Err(e) => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!("compile/run-on: malformed args list ({})", e),
                    ),
                )
            }
        };

        if parts.len() < 2 {
            return (
                SIG_ERROR,
                ctx.error(
                    "arity-error",
                    "compile/run-on: expected (tier closure & args), got fewer than 2 parts",
                ),
            );
        }

        let tier_kw = match self.keyword_spelling(parts[0]) {
            Some(k) => k,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "compile/run-on: tier must be a keyword, got {}",
                            parts[0].type_name()
                        ),
                    ),
                )
            }
        };

        let closure_val = parts[1];
        let closure = match closure_val.as_closure() {
            Some(c) => c.clone(),
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "compile/run-on: target must be a closure, got {}",
                            closure_val.type_name()
                        ),
                    ),
                )
            }
        };

        let call_args: Vec<Value> = parts[2..].to_vec();

        match tier_kw.as_str() {
            "bytecode" => self.on_tier("bytecode", |vm| {
                vm.invoke_closure_bytecode(closure_val, &closure, &call_args)
            }),
            #[cfg(feature = "jit")]
            "jit" => self.on_tier("jit", |vm| {
                vm.invoke_closure_jit(closure_val, &closure, &call_args)
            }),
            #[cfg(not(feature = "jit"))]
            "jit" => crate::rich_error!(
                ctx,
                "tier-rejected",
                "compile/run-on :jit requires --features jit",
                tier = Value::keyword("jit"),
                reason = Value::keyword("feature-disabled"),
            ),
            #[cfg(feature = "wasm")]
            "wasm" => self.on_tier("wasm", |vm| {
                vm.invoke_closure_wasm(closure_val, &closure, &call_args)
            }),
            #[cfg(not(feature = "wasm"))]
            "wasm" => crate::rich_error!(
                ctx,
                "tier-rejected",
                "compile/run-on :wasm requires --features wasm",
                tier = Value::keyword("wasm"),
                reason = Value::keyword("feature-disabled"),
            ),
            #[cfg(feature = "mlir")]
            "mlir-cpu" => self.on_tier("mlir-cpu", |vm| {
                vm.invoke_closure_mlir_cpu(closure_val, &closure, &call_args)
            }),
            #[cfg(not(feature = "mlir"))]
            "mlir-cpu" => crate::rich_error!(
                ctx,
                "tier-rejected",
                "compile/run-on :mlir-cpu requires --features mlir",
                tier = Value::keyword("mlir-cpu"),
                reason = Value::keyword("feature-disabled"),
            ),
            other => crate::rich_error!(
                ctx,
                "tier-rejected",
                format!("compile/run-on: unknown tier :{}", other),
                tier = parts[0],
                reason = Value::keyword("unknown-tier"),
            ),
        }
    }

    /// Run `f` with `tier` as the VM's active tier, and restore the previous
    /// one after, so a nested `compile/run-on` and the surrounding default
    /// both hold.
    fn on_tier<R>(&mut self, tier: &'static str, f: impl FnOnce(&mut VM) -> R) -> R {
        let prev = std::mem::replace(&mut self.active_tier, tier);
        let r = f(self);
        self.active_tier = prev;
        r
    }
}

/// Build a structured `:tier-rejected` error, born in a fresh region of its own
/// ([`VM::error_extra`]) — `vm` owns the heap that mints it. Only an optional
/// tier refuses a closure, so a build with none of them has no caller.
#[cfg(any(feature = "jit", feature = "wasm", feature = "mlir"))]
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
