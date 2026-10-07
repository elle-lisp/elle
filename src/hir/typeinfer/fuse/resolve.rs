// audited: 2026-10-06
//! Finds the body a HOF's function argument names: a lambda literal, this unit's function, or a function value's fragment.
//!
//! docs/impl/dissolution/inline.md
//! docs/impl/dissolution/stages.md

use super::*;
use crate::hir::fragment::{ArmBody, LambdaFragment};
use crate::hir::typeinfer::graft::{fragment_of_value, UnitFns};
use crate::value::Value;
use std::rc::Rc;

/// The resolution context for a HOF's function argument: this unit's functions
/// by binding, and another unit's through the value its binding holds, whose
/// template carries the fragment.
pub(super) struct FnResolver<'a> {
    pub(super) unit: &'a UnitFns,
    pub(super) values: &'a HashMap<Binding, Value>,
    /// This unit's primitives by name, which every fragment's free globals
    /// resolve through.
    pub(super) prim_by_name: &'a FxHashMap<SymbolId, Binding>,
}

impl FnResolver<'_> {
    /// The function a `Var` argument names, when its one body may be spliced
    /// here: this unit's own, or — for an `is_primitive` binding — the one on
    /// the template of the closure it holds. A user redefinition shadows the
    /// export with a binding that is not `is_primitive`, so it never reaches
    /// the export's body.
    ///
    /// The function takes one or two fixed parameters and no rest list. A body
    /// holding a call-position `%`-intrinsic qualifies only under the
    /// function's `(numeric!)` declaration, and every free global must resolve
    /// in this unit, which is what makes [`Self::take_parts`]'s graft total.
    fn fragment(&self, b: Binding, arena: &BindingArena) -> Option<Rc<LambdaFragment>> {
        let fragment = match self.unit.get(&b) {
            Some(f) => Rc::clone(&f.fragment),
            None => {
                let inner = arena.get(b);
                if !inner.is_primitive || !inner.is_immutable || inner.is_mutated {
                    return None;
                }
                fragment_of_value(self.values.get(&b)?)?
            }
        };
        let body = fragment.whole()?;
        let admitted = (1..=2).contains(&fragment.fixed())
            && (fragment.numeric() || !body.fragment.holds_intrinsic())
            && body.fragment.globals_resolve(self.prim_by_name);
        admitted.then_some(fragment)
    }

    /// The body signal of a HOF's function argument at the given arity, for
    /// the reorder gate, or `None` if the argument does not qualify.
    pub(super) fn body_signal(
        &self,
        lam: &Hir,
        arena: &BindingArena,
        arity: usize,
    ) -> Option<Signal> {
        match &lam.kind {
            HirKind::Lambda(_) => qualifies_lambda(lam, arena, arity).map(|(_, body)| body.signal),
            HirKind::Var(b) => {
                let f = self.fragment(*b, arena)?;
                (f.fixed() == arity).then(|| whole(&f).fragment.body_signal())
            }
            _ => None,
        }
    }

    /// Does this function argument's body prove it returns an **array**? Asked
    /// only of a `mapcat`, the one op that reads its function's result as a
    /// collection and walks it: the fused inner walk is an indexed one, linear
    /// over an array and quadratic over a list.
    ///
    /// A call-site literal is read here, against this unit's own init-keyword
    /// proof. A fragment answers from the fact it recorded when it closed, which
    /// is the same proof run in the unit that could see the body's bindings.
    pub(super) fn result_is_array(
        &self,
        lam: &Hir,
        arena: &BindingArena,
        bases: &FxHashMap<Binding, &'static str>,
    ) -> bool {
        match &lam.kind {
            HirKind::Lambda(lambda) => classify_base(&lambda.body, arena, bases).is_some(),
            HirKind::Var(b) => self.fragment(*b, arena).is_some_and(|f| f.returns_array()),
            _ => false,
        }
    }

    /// Resolve a HOF's function argument to owned `(params, body)`, ready to
    /// splice. A **lambda literal** is *moved* out. A `Var` grafts its fragment:
    /// it mints fresh parameters and `let` bindings in this arena and resolves
    /// its free globals by name. `body_signal` proved one path holds at the
    /// required arity and that the globals resolve, so the resolution is total.
    ///
    /// Every minted binding is re-kinded as a compiler temporary, because the
    /// splice places it somewhere the defining unit did not. A parameter becomes
    /// a `let`-bound loop local, bound once per element by the emitted loop and
    /// not by a call. The body's own `let` bindings are re-minted per call site.
    /// So neither answers to a name the user wrote any more. The facts
    /// about the binding itself rather than its place, its mutability and its
    /// `(numeric!)` floor, travel in the fragment and stand.
    pub(super) fn take_parts(&self, lam: Hir, arena: &mut BindingArena) -> (Vec<Binding>, Hir) {
        match lam.kind {
            HirKind::Lambda(lambda) => (lambda.params, lambda.body),
            HirKind::Var(b) => {
                let f = self
                    .fragment(b, arena)
                    .expect("validate_chain proved a graftable fragment");
                let body = whole(&f);
                let (map, spliced) = body
                    .fragment
                    .graft(arena, self.prim_by_name)
                    .expect("validate_chain proved the free globals resolve");
                for i in body.fragment.local_indices() {
                    let inner = arena.get_mut(map[i as usize]);
                    inner.name = SymbolId::SYNTHETIC;
                    inner.scope = BindingScope::Local;
                    inner.is_synthetic = true;
                }
                let params = body.params.iter().map(|&i| map[i as usize]).collect();
                (params, spliced)
            }
            _ => unreachable!("validate_chain proved a lambda or a fragment Var"),
        }
    }
}

/// The one body a fixed-arity fragment has; `fragment` admitted only those.
fn whole(f: &LambdaFragment) -> &ArmBody {
    f.whole()
        .expect("FnResolver::fragment admits only a fixed-arity body")
}
