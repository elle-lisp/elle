// audited: 2026-10-21
//! Code a compile runs on a VM: a macro transformer call, and the body of a
//! `begin-for-syntax` definition, each under the expansion budget.
//!
//! docs/macros.md
//! docs/runtime.md
//! docs/impl/region/park.md
//!
//! Each runs with the unit it expands lent to the VM, where `gensym` names its
//! symbols after it. Both run on the fiber the expansion runs on: the macro
//! VM's fresh fiber, or the fiber whose `eval` or `compile/analyze` started
//! the compile. The budget lives on the VM, so every fiber the expansion
//! resumes spends it, and an expansion nested inside another spends the outer
//! one's. The fiber's own fuel is put aside for the call and comes back
//! unspent. A compile cannot hold a park, so a body that suspends has its park
//! refused, as `eval` refuses one, and the call answers a fault.
//!
//! The fiber also withholds the effect capabilities for the call, and gets its
//! own set back after it. A call to an ambient primitive halts the expansion
//! with `:terminal`, which no fiber mask catches.

mod budget;

pub(crate) use budget::ExpansionBudget;
pub use budget::EXPANSION_FUEL;
use budget::SPENT_KIND;

use super::core::VM;
use crate::signals::registry::format_bits;
use crate::signals::EFFECT_CAPS;
use crate::syntax::Unit;
use crate::value::heap::TableKey;
use crate::value::CodeUnit;
use crate::value::{SignalBits, Value, SIG_ERROR, SIG_HALT, SIG_TERMINAL};
use std::rc::Rc;

/// The error kind of the halt that refuses an ambient primitive.
const AMBIENT_KIND: &str = "ambient-refused";

/// The bits of that halt. `:terminal` carries it past every fiber mask, as the
/// spent budget's halt passes them by halting again at the next charge.
const REFUSAL: SignalBits = SIG_HALT.union(SIG_TERMINAL);

/// Why code a compile ran answered no value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileTimeFault {
    /// The body called `primitive`, which needs `bits` that the expansion
    /// withholds: an effect capability, or one the compiling fiber withholds.
    Denied { primitive: String, bits: SignalBits },
    /// The body called `primitive`, which reaches state outside the unit
    /// through no capability bit.
    Ambient { primitive: String },
    /// The expansion budget ran out while the body ran.
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
                "{primitive} needs {}, which the expansion withholds",
                format_bits(*bits)
            ),
            Self::Ambient { primitive } => format!(
                "{primitive} reads or changes state outside the unit, which an expansion may not"
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
    /// budget, for the expansion of `unit`. The closure is handed to the body
    /// as its executing-closure register, so a self-recursive transformer
    /// resolves its self-reference.
    pub(crate) fn call_transformer(
        &mut self,
        closure: Value,
        args: &[Value],
        unit: &mut Unit,
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
        self.at_expansion(unit, |vm| {
            vm.pending_entry_closure = closure;
            vm.run_thunk_to_completion(&code, &env)
        })
    }

    /// Run `code`, compiled from the body of a `begin-for-syntax` definition or
    /// a transformer's `fn` form, under the expansion budget, for the expansion
    /// of `unit`. The code unit is held until the run ends, because the entry's
    /// `Code` takes no reference of its own.
    pub(crate) fn run_at_expansion(
        &mut self,
        code: &CodeUnit,
        unit: &mut Unit,
    ) -> Result<Value, CompileTimeFault> {
        let code = code.on_heap(self.heap());
        let entry = code.entry().code();
        let env = Rc::new(vec![]);
        self.at_expansion(unit, |vm| vm.run_thunk_to_completion(&entry, &env))
    }

    /// Run `body` under the expansion budget, with the fiber's own fuel put
    /// aside and the effect capabilities withheld, then give the fiber its fuel
    /// and its withheld set back and read what the body answered. An expansion
    /// nested inside another spends the outer budget.
    ///
    /// The body runs with a copy of `unit` lent to the VM, so a `gensym` it
    /// calls names its symbol after the unit, and `unit` keeps the count the
    /// body ran up. A nested expansion lends its own unit and restores this one.
    fn at_expansion(
        &mut self,
        unit: &mut Unit,
        body: impl FnOnce(&mut VM) -> SignalBits,
    ) -> Result<Value, CompileTimeFault> {
        let depth = self.fiber.param_depth();
        let opened = self.expansion_budget.open();
        // A fiber's default fuel is no budget at all.
        let fuel = std::mem::take(&mut self.fiber.fuel);
        let withheld = self.fiber.withheld;
        self.fiber.withheld = withheld.union(EFFECT_CAPS);
        let outer = self.expansion_unit.replace(*unit);
        let bits = body(self);
        let lent = std::mem::replace(&mut self.expansion_unit, outer);
        unit.take_back(lent.expect("the unit lent to an expansion comes back"));
        self.fiber.withheld = withheld;
        self.fiber.fuel = fuel;
        if opened {
            self.expansion_budget.close();
        }
        if bits.is_empty() {
            let (_, value) = self.fiber.signal.take().unwrap_or((bits, Value::NIL));
            return Ok(value);
        }
        if bits.intersects(SIG_HALT) {
            let (_, value) = self.fiber.signal.take().unwrap_or((bits, Value::NIL));
            return if value.is_nil() {
                Ok(value)
            } else if error_kind_is(value, SPENT_KIND) {
                Err(CompileTimeFault::Fuel)
            } else if let Some(primitive) = refused_primitive(value) {
                Err(CompileTimeFault::Ambient { primitive })
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
        let fault = match self.denied_primitive() {
            Some(primitive) => CompileTimeFault::Denied { primitive, bits },
            None => CompileTimeFault::Parked(bits),
        };
        self.refuse_hosted_park(bits, depth);
        Err(fault)
    }

    /// Refuse a call to `def`, an ambient primitive, inside the running
    /// expansion. The primitive does not run. The halt carries `:terminal`, so
    /// no fiber mask, `protect` or silent boundary stops it on its way to
    /// `at_expansion`, and its payload names the primitive.
    pub(crate) fn refuse_ambient(
        &mut self,
        def: &'static crate::primitives::def::PrimitiveDef,
    ) -> (SignalBits, Value) {
        let err = self.error_extra(
            AMBIENT_KIND,
            format!("{} reads or changes state outside the unit", def.name),
            &[("func", Value::native_fn(def))],
        );
        (REFUSAL, err)
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

/// The name of the primitive `value` refuses, when it is the payload
/// `VM::refuse_ambient` builds: its kind, and the primitive in `:func`.
fn refused_primitive(value: Value) -> Option<String> {
    if !error_kind_is(value, AMBIENT_KIND) {
        return None;
    }
    let fields = value.as_struct()?;
    let func = crate::value::sorted_struct_get(fields, &TableKey::keyword("func"))?;
    Some(func.as_native_def()?.name.to_string())
}

/// Whether `value` is an error struct whose `:error` field is the keyword
/// `kind`.
fn error_kind_is(value: Value, kind: &str) -> bool {
    value.as_struct().is_some_and(|fields| {
        crate::value::sorted_struct_get(fields, &TableKey::keyword("error"))
            .is_some_and(|v| v.is_keyword_named(kind))
    })
}

#[cfg(test)]
mod tests;
