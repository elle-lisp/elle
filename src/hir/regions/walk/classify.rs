//! Call-classification queries: read-only lookups over `call_class` that tell
//! the walk what a callee does — returns an immediate, declares a region
//! effect, funnels a retained store, has a known return type, or embeds args
//! into its fresh result. All share the unshadowed-immutable-primitive guard.

use super::*;

impl RegionInference {
    /// Check if a call's callee is known to return an immediate value
    /// (no heap allocation). Uses the call classification data.
    pub(super) fn call_returns_immediate(&self, func: &Hir) -> bool {
        if let HirKind::Var(binding) = &func.kind {
            // Check user_immediates first (letrec-bound lambdas)
            if self.call_class.user_immediates.contains(binding) {
                return true;
            }
            let bi = self.arena().get(*binding);
            // Only trust immutable bindings (primitives, not user-shadowed)
            if !bi.is_immutable || bi.is_mutated {
                return false;
            }
            self.call_class.intrinsic_ops.contains(&bi.name)
                || self.call_class.effects.get(&bi.name)
                    == Some(&crate::primitives::def::RegionEffect::Immediate)
        } else {
            false
        }
    }

    /// The callee's declared `RegionEffect` (docs/impl/region/effects.md "Native
    /// region effects"), when the callee is an immutable, unshadowed
    /// binding naming a declared primitive. `None` for unknown callees —
    /// the caller must treat that as `Mixed` (the full arg clique).
    pub(super) fn call_effect(&self, func: &Hir) -> Option<crate::primitives::def::RegionEffect> {
        if let HirKind::Var(binding) = &func.kind {
            let bi = self.arena().get(*binding);
            if !bi.is_immutable || bi.is_mutated {
                return None;
            }
            self.call_class.effects.get(&bi.name).copied()
        } else {
            None
        }
    }

    /// Is the callee a value-RETAINING store funnel (`%put`/`%array-push`/`%add`)
    /// — a `Funnel` op whose runtime body increfs the stored value? Under the same
    /// unshadowed-immutable-primitive condition as [`call_effect`]. Distinguishes
    /// the store funnels from the removals (`%del`) and byte-copy pushes
    /// (`%string-push`/`%bytes-push`), all of which share the `Funnel` effect.
    pub(super) fn is_retaining_store(&self, func: &Hir) -> bool {
        if let HirKind::Var(binding) = &func.kind {
            let bi = self.arena().get(*binding);
            if !bi.is_immutable || bi.is_mutated {
                return false;
            }
            self.call_class.retaining_store_funnels.contains(&bi.name)
        } else {
            false
        }
    }

    /// The callee's declared [`RetType`](crate::primitives::def::RetType), under
    /// the same unshadowed-immutable-primitive condition as [`call_effect`].
    /// `None` for an unknown/shadowed callee or an empty classification. The
    /// ownership inference uses this to recognize a `Funnel` store's container
    /// argument as a mutable *retaining* container (`MutableArray`/`MutableStruct`).
    pub(super) fn call_rettype(&self, func: &Hir) -> Option<crate::primitives::def::RetType> {
        if let HirKind::Var(binding) = &func.kind {
            let bi = self.arena().get(*binding);
            if !bi.is_immutable || bi.is_mutated {
                return None;
            }
            self.call_class.ret_types.get(&bi.name).copied()
        } else {
            None
        }
    }

    /// The 0-based argument indices the callee EMBEDS into its fresh result
    /// ([`crate::primitives::def::PrimitiveDef::embeds`]), under the same
    /// unshadowed-immutable-primitive condition as [`call_effect`]. Empty for an
    /// unknown/shadowed callee or an empty classification. The walk's `Fresh` arm
    /// records a `result ⊇ arg` containment edge for each, so the ownership forest
    /// tracks an argument the fresh result keeps a reference to (`with-traits`'s trait
    /// table into its cloned result). The returned slice is `'static` (the primitive
    /// table's own data), so it borrows nothing from `self`.
    pub(super) fn call_embeds(&self, func: &Hir) -> &'static [usize] {
        if let HirKind::Var(binding) = &func.kind {
            let bi = self.arena().get(*binding);
            if !bi.is_immutable || bi.is_mutated {
                return &[];
            }
            self.call_class.embeds.get(&bi.name).copied().unwrap_or(&[])
        } else {
            &[]
        }
    }
}
