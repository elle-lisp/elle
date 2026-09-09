// audited: 2026-09-09
// src/hir/AGENTS.md
// docs/impl/typeinfer.md
//! The declared floor, and the guard-derived narrowing facts a branching form
//! applies to the binding environment and then restores.

use super::super::*;

/// Refine a binding's accumulated type with a fact proven about the binding —
/// a type guard's narrowing, or a `(numeric!)` declaration's floor.
///
/// A fact meeting the ascent's start IS the fact. `meet(⊥, fact)` is ⊥, so
/// meeting there would erase a proof that owes nothing to a call site: a guard
/// holds in the branch it governs, and a declaration holds through the body,
/// whether or not this unit calls the enclosing function
/// (docs/impl/typeinfer.md § "Bottom is not a proof").
///
/// A Bottom the meet PRODUCES is the opposite reading — the accumulated type
/// and the fact are disjoint, so no value reaches the site the fact governs —
/// and it is left alone, because it proves nothing either.
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
/// The declaration floors the binding at Number — callers may refine it to
/// Int/Float, never widen past the declared contract — which is what discharges a
/// `%`-intrinsic's operand contract in the declaring function's body
/// (docs/intrinsics.md § "What counts as proof").
///
/// The floor is applied wherever a declared binding is BOUND: at lambda entry, at
/// the call-site parameter join, and at a `let` init — the last being the form a
/// spliced kernel parameter takes once HOF fusion has dissolved its lambda
/// (docs/impl/dissolution.md § "Raw `%`-intrinsic bodies"). It is deliberately NOT
/// applied at an `assign`: a mutated binding has flow the per-pass recomputation
/// cannot see, so it never receives proofs.
pub(crate) fn declared_floor(
    binding: Binding,
    ty: TyId,
    arena: &BindingArena,
    interner: &TypeInterner,
) -> TyId {
    if arena.get(binding).declared_numeric {
        refine(interner, ty, TypeInterner::NUMBER)
    } else {
        ty
    }
}

/// Apply the `TypeIs` facts to the binding environment, returning the saved
/// prior entries for `restore_type_facts`. `Nonzero` facts are the contract
/// checker's flow, not a type — skipped here.
pub(crate) fn apply_type_facts(
    facts: &[super::super::guard::Fact],
    binding_types: &mut HashMap<Binding, TyId>,
    interner: &TypeInterner,
) -> Vec<(Binding, Option<TyId>)> {
    let mut saved = Vec::new();
    for fact in facts {
        let super::super::guard::Fact::TypeIs(binding, narrow_ty) = fact else {
            continue;
        };
        saved.push((*binding, binding_types.get(binding).copied()));
        let old = binding_types
            .get(binding)
            .copied()
            .unwrap_or(TypeInterner::TOP);
        binding_types.insert(*binding, refine(interner, old, *narrow_ty));
    }
    saved
}

/// Undo `apply_type_facts`, in reverse so a twice-narrowed binding restores
/// its original entry.
pub(crate) fn restore_type_facts(
    saved: Vec<(Binding, Option<TyId>)>,
    binding_types: &mut HashMap<Binding, TyId>,
) {
    for (binding, prior) in saved.into_iter().rev() {
        match prior {
            Some(ty) => {
                binding_types.insert(binding, ty);
            }
            None => {
                binding_types.remove(&binding);
            }
        }
    }
}
