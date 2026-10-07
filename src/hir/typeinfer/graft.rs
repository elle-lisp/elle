// audited: 2026-10-06
//! The graft: a call to a known function becomes the body its operand count selects, unless that body recurses.
//!
//! docs/impl/graft.md
//!
//! A same-unit callee's body is the fragment its lambda carries; another
//! unit's is the fragment on the template of the closure its binding holds.

use super::fuse::primitives_by_name;
use crate::hir::arena::BindingArena;
use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirId, HirKind};
use crate::hir::fragment::LambdaFragment;
use crate::value::{SymbolId, Value};
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::HashMap;
use std::rc::Rc;

mod close;
mod live;
mod record;
mod scope;
mod site;

pub(crate) use close::{close_lambdas, UnitFn, UnitFns};
pub(crate) use record::{Contribution, GraftRecord, Operand};

use scope::Scope;

/// Graft every call that qualifies, and record what each graft of a
/// same-unit function bound to that function's parameters.
///
/// `values` maps a binding the runtime supplies to its compile-time value; a
/// closure's template carries the fragment another unit's call grafts.
pub(crate) fn graft_calls(
    hir: &mut Hir,
    arena: &mut BindingArena,
    unit: &UnitFns,
    values: &HashMap<Binding, Value>,
) -> GraftRecord {
    let prim_by_name = primitives_by_name(arena);
    let mut grafter = Grafter {
        arena,
        unit,
        values,
        prim_by_name,
        scope: Scope::new(),
        live: live::live_lambdas(hir),
        grafting: Vec::new(),
        record: GraftRecord::default(),
    };
    grafter.walk(hir);
    grafter.record
}

/// The fragment a closure value's code object carries. A code payload holds
/// no fragment yet, so a call into another unit stays a call
/// (docs/impl/graft.md).
pub(crate) fn fragment_of_value(value: &Value) -> Option<Rc<LambdaFragment>> {
    let _ = value.as_closure()?;
    None
}

struct Grafter<'a> {
    arena: &'a mut BindingArena,
    unit: &'a UnitFns,
    values: &'a HashMap<Binding, Value>,
    /// This unit's primitives by name, which another unit's globals resolve to.
    prim_by_name: FxHashMap<SymbolId, Binding>,
    scope: Scope,
    /// The lambdas live code reaches. A call inside any other lambda stays a
    /// call: its parameters prove nothing a grafted op could use.
    live: FxHashSet<HirId>,
    /// The functions whose grafted bodies the walk is inside. None of them
    /// grafts again there, which bounds a cycle that runs through several
    /// units.
    grafting: Vec<*const LambdaFragment>,
    record: GraftRecord,
}

impl<'a> Grafter<'a> {
    fn walk(&mut self, h: &mut Hir) {
        match &mut h.kind {
            HirKind::Lambda(lambda) => {
                if !self.live.contains(&h.id) {
                    return;
                }
                let mut introduced = lambda.params.clone();
                introduced.extend(lambda.rest_param.iter().copied());
                self.scope.enter_lambda(&lambda.captures, &introduced);
                self.walk(&mut lambda.body);
                self.scope.leave_lambda();
            }
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                for (b, init) in bindings.iter_mut() {
                    self.walk(init);
                    self.scope.bind(*b);
                }
                self.walk(body);
                for (b, _) in bindings.iter() {
                    self.scope.unbind(*b);
                }
            }
            // A `define` binds for the rest of the enclosing body. The scope
            // is asked only about a module binding, which no `define`
            // introduces, so the binding is never unbound.
            HirKind::Define { binding, value } => {
                self.walk(value);
                self.scope.bind(*binding);
            }
            HirKind::Call { .. } => {
                h.for_each_child_mut(|c| self.walk(c));
                if let Some(grafted) = self.graft(h) {
                    *h = grafted;
                }
            }
            _ => h.for_each_child_mut(|c| self.walk(c)),
        }
    }

    /// The function a call names, when its body may replace the call: its
    /// fragment, and its record when this unit defines it.
    fn callee(&self, func: &Hir) -> Option<(Rc<LambdaFragment>, Option<&'a UnitFn>)> {
        let unit: &'a UnitFns = self.unit;
        let HirKind::Var(b) = &func.kind else {
            return None;
        };
        let inner = self.arena.get(*b);
        if !inner.is_immutable || inner.is_mutated {
            return None;
        }
        if let Some(f) = unit.get(b) {
            return Some((Rc::clone(&f.fragment), Some(f)));
        }
        if !inner.is_primitive {
            return None;
        }
        Some((fragment_of_value(self.values.get(b)?)?, None))
    }
}
