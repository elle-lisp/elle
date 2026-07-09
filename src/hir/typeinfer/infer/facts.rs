use super::super::*;

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
