// audited: 2026-10-12
//! Code a compile runs on a VM: a macro transformer call, and the body of a
//! `begin-for-syntax` definition, each under an expansion fuel budget.
//!
//! docs/macros.md
//! docs/runtime.md
//! docs/impl/region/park.md
//!
//! Both run on the fiber the expansion runs on: the macro VM's fresh fiber, or
//! the fiber whose `eval` or `compile/analyze` started the compile. The budget
//! replaces that fiber's fuel for the call, and the fiber's own fuel comes back
//! unspent. A compile cannot hold a park, so a body that suspends has its park
//! refused, as `eval` refuses one, and the call answers a fault.

use super::core::VM;
use crate::signals::registry::format_bits;
use crate::value::fiber::Fuel;
use crate::value::heap::TableKey;
use crate::value::CodeUnit;
use crate::value::{SignalBits, Value, SIG_ERROR, SIG_FUEL, SIG_HALT};
use std::rc::Rc;

/// The fuel one transformer call, or one `begin-for-syntax` definition, may
/// spend, whatever fuel its fiber holds.
pub const EXPANSION_FUEL: u64 = 1 << 24;

/// Why code a compile ran answered no value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileTimeFault {
    /// The body called `primitive`, which needs `bits` that the fiber the
    /// compile runs for withholds.
    Denied { primitive: String, bits: SignalBits },
    /// The body spent the whole expansion budget.
    Fuel,
    /// The body raised this error, or halted with it.
    Raised(String),
    /// The body suspended with these bits.
    Parked(SignalBits),
}

impl CompileTimeFault {
    /// The fault in words. A compile error puts the macro or the definition
    /// that faulted in front of it.
    pub fn describe(&self) -> String {
        match self {
            Self::Denied { primitive, bits } => format!(
                "{primitive} needs {}, which the compiling fiber withholds",
                format_bits(*bits)
            ),
            Self::Fuel => {
                format!("exhausted its expansion fuel budget of {EXPANSION_FUEL} units")
            }
            Self::Raised(msg) => msg.clone(),
            Self::Parked(bits) => format!(
                "signaled {}, and an expansion cannot suspend",
                format_bits(*bits)
            ),
        }
    }
}

impl VM {
    /// Call the macro transformer `closure` with `args` under the expansion
    /// budget. The closure is handed to the body as its executing-closure
    /// register, so a self-recursive transformer resolves its self-reference.
    pub(crate) fn call_transformer(
        &mut self,
        closure: Value,
        args: &[Value],
    ) -> Result<Value, CompileTimeFault> {
        let Some(transformer) = closure.as_closure() else {
            return Err(CompileTimeFault::Raised(format!(
                "transformer is not a closure: {}",
                closure.type_name()
            )));
        };
        if !self.check_arity(&transformer.template.arity(), args.len()) {
            return Err(self.raised_now());
        }
        let Some(env) = self.build_closure_env(transformer, args) else {
            return Err(self.raised_now());
        };
        let code = transformer.template.code();
        self.at_expansion(|vm| {
            vm.pending_entry_closure = closure;
            vm.run_thunk_to_completion(&code, &env)
        })
    }

    /// Run `unit`, compiled from the body of a `begin-for-syntax` definition or
    /// a transformer's `fn` form, under the expansion budget. The unit is held
    /// until the run ends, because the entry's `Code` takes no reference of its
    /// own.
    pub(crate) fn run_at_expansion(&mut self, unit: &CodeUnit) -> Result<Value, CompileTimeFault> {
        let unit = unit.on_heap(self.heap());
        let code = unit.entry().code();
        let env = Rc::new(vec![]);
        self.at_expansion(|vm| vm.run_thunk_to_completion(&code, &env))
    }

    /// Run `body` with the fiber's fuel replaced by the expansion budget, then
    /// give the fiber its own fuel back and read what the body answered.
    fn at_expansion(
        &mut self,
        body: impl FnOnce(&mut VM) -> SignalBits,
    ) -> Result<Value, CompileTimeFault> {
        let depth = self.fiber.param_depth();
        let budget = Fuel::budget(EXPANSION_FUEL).expect("the expansion budget is an integer");
        let fuel = std::mem::replace(&mut self.fiber.fuel, budget);
        let bits = body(self);
        self.fiber.fuel = fuel;
        if bits.is_empty() {
            let (_, value) = self.fiber.signal.take().unwrap_or((bits, Value::NIL));
            return Ok(value);
        }
        if bits == SIG_HALT {
            let (_, value) = self.fiber.signal.take().unwrap_or((bits, Value::NIL));
            return if value.is_nil() {
                Ok(value)
            } else {
                Err(CompileTimeFault::Raised(
                    self.format_error_with_location(value),
                ))
            };
        }
        if bits.intersects(SIG_ERROR) {
            // The error parked the body's chain, which an expansion cannot
            // hold, so it ends here before the compile reports it.
            self.discard_hosted_error(bits, depth);
            return Err(self.raised_now());
        }
        // The denial payload is read before the refusal, which may release it.
        let fault = if bits == SIG_FUEL {
            CompileTimeFault::Fuel
        } else if let Some(primitive) = self.denied_primitive() {
            CompileTimeFault::Denied { primitive, bits }
        } else {
            CompileTimeFault::Parked(bits)
        };
        self.refuse_hosted_park(bits, depth);
        Err(fault)
    }

    /// The error in the fiber's signal slot, taken and formatted.
    fn raised_now(&mut self) -> CompileTimeFault {
        let (_, err) = self.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
        CompileTimeFault::Raised(self.format_error_with_location(err))
    }

    /// The primitive a capability denial in the fiber's signal slot names, or
    /// `None` when the slot holds some other park.
    fn denied_primitive(&self) -> Option<String> {
        let (_, payload) = self.fiber.signal.as_ref()?;
        let fields = payload.as_struct()?;
        let field = |name: &str| crate::value::sorted_struct_get(fields, &TableKey::keyword(name));
        if !field("error").is_some_and(|v| v.is_keyword_named("capability-denied")) {
            return None;
        }
        field("primitive")?.with_string(|s| s.to_string())
    }
}

#[cfg(test)]
mod tests;
