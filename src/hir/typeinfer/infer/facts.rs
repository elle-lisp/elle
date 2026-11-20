// audited: 2026-11-20
//! The declared floor, and the guard facts in force: how a branching form applies them, and what ends them.
//!
//! docs/impl/typeinfer.md
//! docs/impl/typeinfer/facts.md
//! docs/impl/dissolution/inline.md

use super::super::*;
use crate::hir::arena::BindingScope;

/// Refine a binding's accumulated type with a fact proven about the binding —
/// a type guard's narrowing, or a `(numeric!)` declaration's floor.
///
/// A fact meeting the ascent's start IS the fact. `meet(⊥, fact)` is ⊥, so
/// meeting there would erase a proof that owes nothing to a call site. A guard
/// holds in the branch it governs, and a declaration holds through the body,
/// whether or not this unit calls the enclosing function.
///
/// A Bottom the meet PRODUCES is the opposite reading: the accumulated type
/// and the fact are disjoint, so no value reaches the site the fact governs.
/// It is left alone, because it proves nothing either.
///
/// The two are one `TyId`, so a Bottom an enclosing disjoint fact produced
/// reads here as the start and the fact re-proves the binding. That costs
/// precision and nothing else: reaching it takes two mutually exclusive guards
/// around the site, so the code the fact proves is code no value reaches.
fn refine(interner: &TypeInterner, accumulated: TyId, fact: TyId) -> TyId {
    if accumulated == TypeInterner::BOTTOM {
        return fact;
    }
    interner.meet(accumulated, fact)
}

/// Apply a binding's `(numeric!)` declaration to a type it is being bound with.
/// The declaration floors the binding at Number. Callers may refine it to Int
/// or Float, never widen it past Number, and the floor discharges a
/// `%`-intrinsic's operand contract in the declaring function's body.
///
/// The floor applies wherever a declared binding is bound: at lambda entry, at
/// the call-site parameter join, and at a `let` init. The `let` is the form a
/// spliced kernel parameter takes once HOF fusion has dissolved its lambda,
/// and the form a grafted parameter takes. The floor does not apply at an
/// `assign`: a mutated binding has flow the per-pass recomputation cannot see,
/// so it never receives proofs.
///
/// The declaration types the binding whatever it receives. A type disjoint from
/// Number, such as a string operand, meets the floor at Bottom, and the floor
/// gives Number there instead. Lambda entry reaches the same answer, because it
/// reads that Bottom as the Kleene start.
pub(crate) fn declared_floor(
    binding: Binding,
    ty: TyId,
    arena: &BindingArena,
    interner: &TypeInterner,
) -> TyId {
    if !arena.get(binding).declared_numeric {
        return ty;
    }
    match refine(interner, ty, TypeInterner::NUMBER) {
        TypeInterner::BOTTOM => TypeInterner::NUMBER,
        floored => floored,
    }
}

/// Whether code other than the running code can write `binding` while a call
/// or an `emit` runs: a closure may write it, or the running lambda captures it
/// and the unit writes it. `captured` is what the innermost lambda captures,
/// empty outside every lambda. The contract walk asks this of a nonzero fact.
pub(crate) fn is_shared(
    binding: Binding,
    arena: &BindingArena,
    captured: &rustc_hash::FxHashSet<Binding>,
) -> bool {
    let inner = arena.get(binding);
    inner.closure_may_write() || (inner.is_mutated && captured.contains(&binding))
}

/// Whether a call to `func` lets no other code run: the callee is the native
/// the runtime supplies under its name, and that native declares it runs no
/// Elle code. A parameter or a local that shares the name is an unknown
/// callee. A call that answers true ends no fact.
pub(crate) fn runs_no_elle(func: &Hir, arena: &BindingArena) -> bool {
    let Some(callee) = unwrap_callee_binding(func) else {
        return false;
    };
    let inner = arena.get(callee);
    inner.is_primitive
        && inner.is_native_fn
        && inner.is_immutable
        && !inner.is_mutated
        && crate::primitives::registration::def_by_symbol(inner.name)
            .is_some_and(|def| def.runs_no_elle)
}

/// One binding a fact narrowed: what it held before, and what the fact gave
/// it, so the end of the fact's scope can tell whether the scope wrote it.
pub(crate) struct Narrowed {
    binding: Binding,
    prior: Option<TyId>,
    narrowed: TyId,
}

impl Infer<'_> {
    /// How many facts are in force. A scope that applies facts hands this back
    /// to `end_facts` when it closes.
    pub(super) fn facts_in_force(&self) -> usize {
        self.narrowings.len()
    }

    /// Give `binding` the type `ty` until the scope that called this ends.
    pub(super) fn narrow(&mut self, binding: Binding, ty: TyId) {
        let prior = self.binding_types.insert(binding, ty);
        self.narrowings.push(Narrowed {
            binding,
            prior,
            narrowed: ty,
        });
    }

    /// Apply the `TypeIs` facts until the scope that called this ends.
    /// `Nonzero` facts are the contract walk's flow, not a type, so they are
    /// skipped here.
    pub(super) fn apply_facts(&mut self, facts: &[super::super::guard::Fact]) {
        for fact in facts {
            let super::super::guard::Fact::TypeIs(binding, narrow_ty) = fact else {
                continue;
            };
            let old = self
                .binding_types
                .get(binding)
                .copied()
                .unwrap_or(TypeInterner::TOP);
            let refined = refine(&self.interner, old, *narrow_ty);
            self.narrow(*binding, refined);
        }
    }

    /// End every fact applied since `mark`, innermost first, so a
    /// twice-narrowed binding ends at its original entry.
    ///
    /// A binding the scope did not write takes back its prior type. A binding it
    /// wrote holds something the fact never governed, and the write outlives the
    /// scope, so it takes the join of the prior type and what the write left.
    pub(super) fn end_facts(&mut self, mark: usize) {
        while self.narrowings.len() > mark {
            let n = self.narrowings.pop().expect("a fact above the mark");
            let current = self.binding_types.get(&n.binding).copied();
            if current == Some(n.narrowed) {
                match n.prior {
                    Some(ty) => self.binding_types.insert(n.binding, ty),
                    None => self.binding_types.remove(&n.binding),
                };
                continue;
            }
            let written = current.unwrap_or(TypeInterner::TOP);
            let prior = n.prior.unwrap_or(TypeInterner::TOP);
            let joined = self.interner.join(prior, written);
            self.binding_types.insert(n.binding, joined);
        }
    }

    /// The type `binding` may hold where no fact in force governs it: its type
    /// now, joined with every type those facts hid.
    fn unnarrowed(&self, binding: Binding) -> TyId {
        self.narrowings
            .iter()
            .filter(|n| n.binding == binding)
            .fold(self.read(binding), |ty, n| {
                self.interner.join(ty, n.prior.unwrap_or(TypeInterner::TOP))
            })
    }

    /// The type a read of `binding` sees now. An entry the pass holds no type
    /// for reads as unknown.
    fn read(&self, binding: Binding) -> TyId {
        self.binding_types
            .get(&binding)
            .copied()
            .unwrap_or(TypeInterner::TOP)
    }

    /// The type a write to `binding` joins with when the pass holds none for
    /// it. A local the walk has not bound yet starts at Bottom, and its binder
    /// joins its initializer later. A parameter entered with a value no call
    /// site proved, which is unknown.
    pub(super) fn unwritten(&self, binding: Binding) -> TyId {
        match self.arena.get(binding).scope {
            BindingScope::Parameter => TypeInterner::TOP,
            BindingScope::Local => TypeInterner::BOTTOM,
        }
    }

    /// A call or an `emit` let other code run, and it may have written a
    /// shared binding. Every fact in force about one ends here: the binding
    /// takes back every type its facts hid, for the rest of their scope.
    pub(super) fn end_shared_facts(&mut self) {
        let shared: Vec<Binding> = self
            .narrowings
            .iter()
            .map(|n| n.binding)
            .filter(|b| is_shared(*b, self.arena, &self.captured))
            .collect();
        for b in shared {
            let ty = self.unnarrowed(b);
            self.binding_types.insert(b, ty);
        }
    }

    /// Walk a lambda's body, which runs when the lambda is called rather than
    /// where it stands. A captured binding the unit writes reads with none of
    /// the facts in force here. A binding the body writes keeps the join of its
    /// type here and what the body left.
    pub(super) fn infer_lambda_body(&mut self, captures: &[Binding], body: &Hir) -> TyId {
        let narrowed: Vec<Binding> = captures
            .iter()
            .copied()
            .filter(|b| self.arena.get(*b).is_mutated)
            .filter(|b| self.narrowings.iter().any(|n| n.binding == *b))
            .collect();
        let mut lifted = Vec::with_capacity(narrowed.len());
        for b in narrowed {
            let here = self.read(b);
            let view = self.unnarrowed(b);
            self.binding_types.insert(b, view);
            lifted.push((b, here, view));
        }
        let outer = std::mem::replace(&mut self.captured, captures.iter().copied().collect());
        let body_ty = self.infer(body);
        self.hir_types.insert(body.id, body_ty);
        self.captured = outer;
        for (b, here, view) in lifted {
            let left = self.read(b);
            let ty = if left == view {
                here
            } else {
                self.interner.join(here, left)
            };
            self.binding_types.insert(b, ty);
        }
        body_ty
    }
}
