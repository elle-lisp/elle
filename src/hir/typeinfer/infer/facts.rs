// audited: 2026-10-06
//! The declared floor, and the guard facts a branching form applies to the binding types and then restores.
//!
//! docs/impl/typeinfer.md
//! docs/intrinsics.md
//! docs/impl/dissolution/inline.md

use super::super::*;

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

/// One binding a fact narrowed: what it held before, and what the fact gave
/// it, so the end of the fact's scope can tell whether the scope wrote it.
pub(crate) struct Narrowed {
    binding: Binding,
    prior: Option<TyId>,
    narrowed: TyId,
}

/// Give `binding` the type `ty` for the scope a fact governs, saving what
/// `restore_type_facts` needs to end that scope.
pub(crate) fn narrow_type(
    binding: Binding,
    ty: TyId,
    binding_types: &mut HashMap<Binding, TyId>,
) -> Narrowed {
    let prior = binding_types.insert(binding, ty);
    Narrowed {
        binding,
        prior,
        narrowed: ty,
    }
}

/// Apply the `TypeIs` facts to the binding environment, returning what
/// `restore_type_facts` needs to end their scope. `Nonzero` facts are the
/// contract checker's flow, not a type — skipped here.
pub(crate) fn apply_type_facts(
    facts: &[super::super::guard::Fact],
    binding_types: &mut HashMap<Binding, TyId>,
    interner: &TypeInterner,
) -> Vec<Narrowed> {
    let mut saved = Vec::new();
    for fact in facts {
        let super::super::guard::Fact::TypeIs(binding, narrow_ty) = fact else {
            continue;
        };
        let old = binding_types
            .get(binding)
            .copied()
            .unwrap_or(TypeInterner::TOP);
        saved.push(narrow_type(
            *binding,
            refine(interner, old, *narrow_ty),
            binding_types,
        ));
    }
    saved
}

/// End the scope of the facts `saved` records, in reverse so a twice-narrowed
/// binding ends at its original entry.
///
/// A binding the scope did not write takes back its prior type. A binding it
/// wrote holds something the fact never governed, and the write outlives the
/// scope, so it takes the join of the prior type and what the write left.
pub(crate) fn restore_type_facts(
    saved: Vec<Narrowed>,
    binding_types: &mut HashMap<Binding, TyId>,
    interner: &TypeInterner,
) {
    for n in saved.into_iter().rev() {
        let current = binding_types.get(&n.binding).copied();
        if current == Some(n.narrowed) {
            match n.prior {
                Some(ty) => binding_types.insert(n.binding, ty),
                None => binding_types.remove(&n.binding),
            };
            continue;
        }
        let written = current.unwrap_or(TypeInterner::TOP);
        let prior = n.prior.unwrap_or(TypeInterner::TOP);
        binding_types.insert(n.binding, interner.join(prior, written));
    }
}
