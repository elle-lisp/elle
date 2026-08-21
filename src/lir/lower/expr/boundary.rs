//! Ownership / dynamic-scope boundary lowering: `Return` and `Parameterize`.
//!
//! Grouped because both wrap a body evaluation in boundary bookkeeping —
//! `lower_return` mints the caller's owning reference to the result region,
//! `lower_parameterize` brackets the body in a Push/Pop param frame — rather
//! than lowering a plain sub-expression.

use super::*;

impl<'a> Lowerer<'a> {
    /// Lower a `Return` ownership boundary: evaluate the value, then
    /// incref its result region so the caller receives one owning reference.
    /// Region-transparent — returns the value's own register. The mint is
    /// emitted here, before the node's own `emit_decrefs_for` (which the
    /// regions pass arranges to fire at this Return node, after the retain —
    /// see the `return_sites` decref_point extension), so a freshly-allocated
    /// result region survives its callee-side release.
    ///
    /// Two encodings, chosen by `coalescible_region` (the staticness predicate,
    /// docs/impl/region/mechanism.md § "Compile-time region selection (coalescing)"):
    ///
    /// - **slot-resolved** when the returned value is a fresh local allocation
    ///   whose region is a known static slot — emit the equivalence oracle
    ///   `AssertRegionMatches { slot }` (debug builds only) then
    ///   `IncrefRegion { slot }`. The slot resolves, through the activation map,
    ///   to the same physical region `region_of(value)` would return (the alloc
    ///   stamped it; a value never moves regions), so the RC trajectory is
    ///   bit-identical — one fewer runtime deref, stack-neutral.
    /// - **value-resolved** otherwise (the dynamic boundary — a borrowed
    ///   captured upvalue, a pass-through arg, a branch-dependent mix, an opaque
    ///   call result): emit `IncrefValueRegion { src }`, reading the region from
    ///   the value at runtime.
    ///
    /// Either way the caller balances the mint with a `DecrefValueRegion` at the
    /// result binding's `decref_point` (the caller cannot name the callee's
    /// region — prediction-free — so the substitution is purely callee-mint-side).
    pub(super) fn lower_return(&mut self, value: &Hir) -> Result<Reg, String> {
        let reg = self.lower_expr(value)?;
        // Transform 1 (docs/impl/region/mechanism.md § "Compile-time region selection
        // (coalescing)"): when the returned value is a fresh local allocation whose
        // region is a known static slot, the mint is slot-resolved; otherwise the
        // region is a genuine runtime fact (the dynamic boundary) and the mint stays
        // value-resolved. `coalescible_region` is the staticness predicate; this
        // function's doc-comment carries the two encodings and why the caller side
        // is unaffected (prediction-free).
        let slot = self.coalescible_region(value);
        super::rcstats::record_return_mint(slot.is_some());
        if crate::config::get().has_trace("rc") {
            eprintln!(
                "[trace:rc:emit] return_mint hir_id={:?} coalescible={:?} span={}",
                value.id, slot, self.current_span,
            );
        }
        match slot {
            Some(slot) => {
                // The equivalence oracle: assert the slot resolves to the same
                // physical region the value actually lives in — a mis-coalesce
                // would let the cascade free a live region (a UAF). Debug-only; it
                // peeks `reg`, leaving it on top for the following `IncrefRegion`
                // and the `Return`. Release builds omit it entirely, so the
                // bytecode never carries it (C0 emit contract).
                #[cfg(debug_assertions)]
                self.emit(LirInstr::AssertRegionMatches {
                    region_id: slot,
                    src: reg,
                });
                self.emit(LirInstr::IncrefRegion { region_id: slot });
            }
            None => self.emit(LirInstr::IncrefValueRegion { src: reg }),
        }
        Ok(reg)
    }

    pub(super) fn lower_parameterize(
        &mut self,
        bindings: &[(Hir, Hir)],
        body: &Hir,
    ) -> Result<Reg, String> {
        // Lower all param/value pairs
        let mut pairs = Vec::new();
        for (param, value) in bindings {
            let param_reg = self.lower_expr(param)?;
            let value_reg = self.lower_expr(value)?;
            pairs.push((param_reg, value_reg));
        }

        // Emit PushParamFrame
        self.emit(LirInstr::PushParamFrame { pairs });

        // Lower body
        let body_reg = self.lower_expr(body)?;

        // Store result in a local slot so PopParamFrame doesn't interfere
        let result_reg = self.fresh_reg();
        let result_slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;
        self.emit(LirInstr::StoreLocal {
            slot: result_slot,
            src: body_reg,
        });

        // Emit PopParamFrame
        self.emit(LirInstr::PopParamFrame);

        // Reload result
        self.emit(LirInstr::LoadLocal {
            dst: result_reg,
            slot: result_slot,
        });

        Ok(result_reg)
    }
}
