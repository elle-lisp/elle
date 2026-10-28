// audited: 2026-10-28
//! The graft: a call to a known function becomes the body its operand count selects, unless that body recurses.
//!
//! docs/impl/graft.md
//! docs/impl/graft/operands.md
//!
//! A same-unit callee's body is the fragment its lambda carries; another
//! unit's is the fragment on the code payload of the closure its binding holds.

use super::fuse::primitives_by_name;
use crate::hir::arena::BindingArena;
use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirId, HirKind};
use crate::hir::fragment::LambdaFragment;
use crate::value::closure::ClosureTemplate;
use crate::value::{SymbolId, Value};
use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

mod close;
mod live;
mod operand;
mod record;
mod scope;
mod site;

pub(crate) use close::{close_lambdas, UnitFn, UnitFns};
pub(crate) use record::{Contribution, GraftRecord, Operand, Readings};

use scope::Scope;

/// Graft every call that qualifies, and record what each graft of a
/// same-unit function bound to that function's parameters.
///
/// `values` maps a binding the runtime supplies to its compile-time value; a
/// closure's code payload carries the fragment another unit's call grafts, and
/// `memo` decodes each payload's once.
pub(crate) fn graft_calls(
    hir: &mut Hir,
    arena: &mut BindingArena,
    unit: &UnitFns,
    values: &HashMap<Binding, Value>,
    memo: &FragmentMemo,
) -> GraftRecord {
    let prim_by_name = primitives_by_name(arena);
    let mut grafter = Grafter {
        arena,
        unit,
        values,
        memo,
        prim_by_name,
        scope: Scope::new(),
        live: live::live_lambdas(hir),
        unreached: 0,
        grafting: Vec::new(),
        record: GraftRecord::default(),
    };
    grafter.walk(hir);
    grafter.record
}

/// The fragments one compile has decoded off code payloads, by payload: a unit
/// that calls `+` a hundred times decodes `+` once (docs/impl/graft.md). A
/// memo lives for one compile, and a payload whose bytes are no body is
/// remembered as none.
pub(crate) struct FragmentMemo {
    decoded: RefCell<FxHashMap<usize, Option<Rc<LambdaFragment>>>>,
}

impl FragmentMemo {
    pub(crate) fn new() -> Self {
        FragmentMemo {
            decoded: RefCell::new(FxHashMap::default()),
        }
    }

    /// The fragment the closure `value`'s code object carries, or `None` for a
    /// value that is not a closure or a payload whose bytes are no body.
    pub(crate) fn fragment_of(&self, value: &Value) -> Option<Rc<LambdaFragment>> {
        let closure = value.as_closure()?;
        self.of_template(&closure.template)
    }

    /// The fragment `t`'s payload carries, decoded once per memo. Bytes that
    /// do not decode are a body the payload does not carry, and so is an
    /// empty slice: the call stays a call.
    pub(crate) fn of_template(&self, t: &ClosureTemplate) -> Option<Rc<LambdaFragment>> {
        let bytes = t.fragment();
        if bytes.is_empty() {
            return None;
        }
        let key = t.payload_backing() as usize;
        if let Some(hit) = self.decoded.borrow().get(&key) {
            return hit.clone();
        }
        let decoded = LambdaFragment::decode(bytes).map(Rc::new);
        self.decoded.borrow_mut().insert(key, decoded.clone());
        decoded
    }
}

struct Grafter<'a> {
    arena: &'a mut BindingArena,
    unit: &'a UnitFns,
    values: &'a HashMap<Binding, Value>,
    /// The compile's decoded fragments, which another unit's body is read
    /// through.
    memo: &'a FragmentMemo,
    /// This unit's primitives by name, which another unit's globals resolve to.
    prim_by_name: FxHashMap<SymbolId, Binding>,
    scope: Scope,
    /// The lambdas live code reaches. Inside any other lambda, only a body
    /// with no op to prove grafts: the lambda's parameters prove nothing a
    /// grafted op could use.
    live: FxHashSet<HirId>,
    /// How many lambdas nothing live reaches the walk is inside.
    unreached: usize,
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
                let reached = self.live.contains(&h.id);
                if !reached {
                    self.unreached += 1;
                }
                let mut introduced = lambda.params.clone();
                introduced.extend(lambda.rest_param.iter().copied());
                self.scope.enter_lambda(&lambda.captures, &introduced);
                self.walk(&mut lambda.body);
                self.scope.leave_lambda();
                if !reached {
                    self.unreached -= 1;
                }
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
        Some((self.memo.fragment_of(self.values.get(b)?)?, None))
    }
}
