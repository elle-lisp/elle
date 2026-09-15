// audited: 2026-09-14
//! Whether a value's region can be named by a static slot, or is a runtime
//! fact the emission has to read off the value itself.
//! docs/impl/region/mechanism.md

use super::*;

impl<'a> Lowerer<'a> {
    /// The static region **slot** to coalesce a value's mint onto, or `None` to
    /// stay value-resolved (docs/impl/region/mechanism.md § "Compile-time region
    /// selection (coalescing)"). Layers the lowering-time runtime-population guard
    /// over `coalescible_solver_region`'s solver-fact class logic: the region's
    /// slot must already be mapped (`region_to_table`) AND stamped by an allocation
    /// **emitted in this function** (`emitted_alloc_regions`), so the activation
    /// map populates it at runtime.
    ///
    /// The class predicate alone is not sufficient: a value whose region is
    /// statically nameable yet allocated in *another* activation — an immutable
    /// captured upvalue, or a cross-thread/fiber value in a process-shared region
    /// (e.g. a `sys/spawn-vm` thunk returning a captured string, living in a
    /// shared region) — passes the class check but has no slot stamped in *this*
    /// function. A slot-resolved `IncrefRegion` against it resolves to `None` at
    /// runtime and its cascade frees a live region (the mis-coalesce the
    /// `AssertRegionMatches` oracle catches; tests/elle/concurrency.lisp). This is
    /// the same phantom-region guard `emit_decref_region` applies on the decref
    /// side. The slot is *read*, never minted: the owning allocation already
    /// minted it in program order, so a `region_to_table` miss means "not
    /// allocated in this function" → refuse.
    pub(in crate::lir::lower) fn coalescible_region(
        &self,
        value: &Hir,
    ) -> Option<crate::hir::region::StaticRegion> {
        // Resolve through `merged_root` exactly as `static_slot` does, so the
        // lookup hits the (root-keyed) `region_to_table` for a merged region. In
        // practice a coalescible value is never a merge participant (the merge seed
        // refuses escaping/returned children, which is what `coalescible_*` accepts),
        // so this is the identity here; it keeps the two slot resolvers consistent.
        let region = self
            .region_info
            .merged_root(self.coalescible_solver_region(value)?);
        let slot = *self.region_to_table.get(&region)?;
        self.emitted_alloc_regions.contains(&slot).then_some(slot)
    }

    /// The solver region a value's mint can be coalesced onto, or `None` when the
    /// region is genuinely a runtime fact (the dynamic boundary). `Some(r)` iff
    /// the value is a fresh local allocation whose region `r` is statically
    /// nameable: its region (`alloc_region` for a direct allocation, or, for a
    /// returned binding read, the single region in `binding_source_regions`) is
    /// `live` and is **none** of the dynamic classes — a call-result placeholder
    /// (`call_result_regions`, which subsumes a returned fixed param's phantom
    /// region and an opaque `(f x)`), an env-cell release (`cell_release_regions`,
    /// a captured upvalue), a reassign-suppressed region
    /// (`suppressed_decref_regions`), a reassigned 1-slot-container value region
    /// (`mutated_binding_value_regions`, which also catches a returned `Var`
    /// aliasing a store target — escape.md divergence 2), or a value a fn-local
    /// 1-slot container holds (`cell_stored_regions`).
    ///
    /// That last class is a runtime fact even though its allocation site names a
    /// static slot: the container re-mints its content at every store and each
    /// store's producer release *unmaps* that slot, so a mint emitted later — the
    /// `Return` handing the final content out — would resolve the slot to nothing
    /// and the equivalence oracle detonates
    /// (docs/impl/region/bindings.md § "A value a 1-slot container holds is a
    /// runtime fact"; `coalescible_refuses_a_cell_stored_value`).
    ///
    /// A returned `Var` whose `binding_source_regions` names *more than one*
    /// region is a branch-dependent mix — not statically nameable — so it is
    /// refused. Pure: no emission, no `region_table` mutation.
    pub(in crate::lir::lower) fn coalescible_solver_region(
        &self,
        value: &Hir,
    ) -> Option<crate::hir::region::Region> {
        let info = &self.region_info;
        let region = match &value.kind {
            HirKind::Var(b) => match info.binding_source_regions.get(b)?.as_slice() {
                [r] => *r,
                _ => return None,
            },
            _ => *info.alloc_region.get(&value.id)?,
        };
        if !info.live_regions.contains(&region)
            || info.call_result_regions.contains(&region)
            || info.cell_release_regions.contains(&region)
            || info.suppressed_decref_regions.contains(&region)
            || info.mutated_binding_value_regions.contains(&region)
            || info.cell_stored_regions.contains(&region)
        {
            return None;
        }
        Some(region)
    }
}
