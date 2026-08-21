use super::super::*;

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
        interner.meet(ty, TypeInterner::NUMBER)
    } else {
        ty
    }
}

/// Apply the `TypeIs` facts to the binding environment (meet with the
/// accumulated type), returning the saved prior entries for `restore_type_facts`.
/// `Nonzero` facts are the contract checker's flow, not a type — skipped here.
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
        binding_types.insert(*binding, interner.meet(old, *narrow_ty));
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
