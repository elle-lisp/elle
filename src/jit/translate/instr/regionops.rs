// audited: 2026-09-29
//! Translating the region instructions: the slot and value refcounts, the ownership-forest ops, and the join.
//!
//! docs/impl/region/mechanism.md
//! docs/impl/region/ownership.md
//! docs/impl/region/colocation.md

use super::*;

impl<'a> FunctionTranslator<'a> {
    /// Region-refcount, ownership and join instructions (chain tail). Each arm
    /// calls the helper that mirrors the interpreter's handler for it.
    pub(super) fn translate_instr_region(
        &mut self,
        builder: &mut FunctionBuilder,
        instr: &LirInstr,
    ) -> Result<bool, JitError> {
        match instr {
            LirInstr::IncrefRegion { region_id } => {
                // Resolve the static slot through THIS activation's region map
                // (in the helper), not as a physical region id. Mirror of the
                // interpreter's defensive `IncrefRegion` arm.
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("IncrefRegion without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.incref_region, builder.func);
                let rid = builder.ins().iconst(I32, region_id.get() as i64);
                builder.ins().call(func_ref, &[vm, rid]);
            }

            LirInstr::DecrefRegion { region_id } => {
                // Resolve+clear the static slot through the activation map (in the
                // helper via `take_runtime_region_for_drop_slot`) and decref the
                // physical region — never treat the slot id as a physical region.
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("DecrefRegion without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.decref_region, builder.func);
                let rid = builder.ins().iconst(I32, region_id.get() as i64);
                builder.ins().call(func_ref, &[vm, rid]);
            }

            LirInstr::DecrefValueRegion { src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("DecrefValueRegion without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.decref_value_region, builder.func);
                builder.ins().call(func_ref, &[st, sp, vm]);
            }

            LirInstr::DecrefCellRegion { src } => {
                // Free the CELL's own region via `region_of` (NOT
                // `result_region_of`): `elle_jit_decref_cell_region`, mirroring
                // the interpreter's `DecrefCellRegion` arm. `DecrefValueRegion`
                // above uses `result_region_of` to unwrap a capture cell to the
                // inner value — the two must not be conflated, or a cell-wrapped
                // call result is freed twice.
                let (st, sp) = self.use_var_pair(builder, src.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("DecrefCellRegion without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.decref_cell_region, builder.func);
                builder.ins().call(func_ref, &[st, sp, vm]);
            }

            LirInstr::IncrefValueRegion { src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("IncrefValueRegion without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.incref_value_region, builder.func);
                builder.ins().call(func_ref, &[st, sp, vm]);
            }

            LirInstr::AdoptRegion { parent, child } => {
                // Link the child's region as Owned by the parent's region — the
                // runtime `AdoptRegion` (docs/impl/region/ownership.md).
                // Value-resolved like `IncrefValueRegion`/`DecrefValueRegion`:
                // load both values and hand them to the helper, which resolves
                // each to its runtime region and adopts.
                // Mirrors the interpreter's `handle_adopt_region`.
                let (pt, pp) = self.use_var_pair(builder, parent.0);
                let (ct, cp) = self.use_var_pair(builder, child.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("AdoptRegion without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.adopt_region, builder.func);
                let call = builder.ins().call(func_ref, &[pt, pp, ct, cp, vm]);
                let _ = builder.inst_results(call);
            }

            LirInstr::AdoptCellRegion { parent, child } => {
                // Like `AdoptRegion`, but the helper resolves BOTH operands with
                // `region_of` (NOT `result_region_of`) so a `CaptureCell` operand's
                // OWN region is adopted (the cell↔closure containment —
                // docs/impl/region/adopt.md). Mirrors the
                // interpreter's `handle_adopt_cell_region`.
                let (pt, pp) = self.use_var_pair(builder, parent.0);
                let (ct, cp) = self.use_var_pair(builder, child.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("AdoptCellRegion without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.adopt_cell_region, builder.func);
                let call = builder.ins().call(func_ref, &[pt, pp, ct, cp, vm]);
                let _ = builder.inst_results(call);
            }

            LirInstr::AdoptIntoActivation { child } => {
                // Adopt the child's region into the current activation's owner
                // node — the runtime channel of the activation-ownership cuts
                // (docs/impl/region/owner.md). Value-resolved
                // like `AdoptRegion`, with no parent operand (the node is VM
                // state, minted lazily by the helper). Mirrors the
                // interpreter's `handle_adopt_into_activation`.
                let (ct, cp) = self.use_var_pair(builder, child.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("AdoptIntoActivation without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.adopt_into_activation, builder.func);
                let call = builder.ins().call(func_ref, &[ct, cp, vm]);
                let _ = builder.inst_results(call);
            }

            LirInstr::FreeRegionGroup { members } => {
                // Free a co-owned region group as one unit — the runtime
                // `FreeRegionGroup`. Spill each member value to a stack slot
                // (16 bytes: tag, payload) and pass a pointer + count to the
                // helper (exactly as `PushParamFrame` spills its pairs), which
                // resolves each to its runtime region and frees the whole set.
                // Mirrors the interpreter's `handle_free_region_group`.
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("FreeRegionGroup without vm pointer".to_string())
                })?;
                let count = members.len();
                let (members_ptr, count_val) = if count == 0 {
                    (builder.ins().iconst(I64, 0), builder.ins().iconst(I64, 0))
                } else {
                    let slot =
                        builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                            (count * 16) as u32,
                            0,
                        ));
                    for (i, member_reg) in members.iter().enumerate() {
                        let (mt, mp) = self.use_var_pair(builder, member_reg.0);
                        let base = (i * 16) as i32;
                        builder.ins().stack_store(mt, slot, base);
                        builder.ins().stack_store(mp, slot, base + 8);
                    }
                    let ptr = builder.ins().stack_addr(I64, slot, 0);
                    let cnt = builder.ins().iconst(I64, count as i64);
                    (ptr, cnt)
                };
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.free_region_group, builder.func);
                let call = builder.ins().call(func_ref, &[members_ptr, count_val, vm]);
                let _ = builder.inst_results(call);
            }

            // The coalescing equivalence oracle is a VM-interp-only debug
            // instrument; the JIT translates it to nothing. Coalesced sites on
            // the optimizing tiers are covered by cross-tier divergence + the
            // escape golden (docs/impl/region/mechanism.md).
            LirInstr::AssertRegionMatches { .. } => {}

            LirInstr::JoinRegion { region, partner } => {
                // Record the pending join the next mint of `region` consumes
                // (docs/impl/region/colocation.md). Value-resolved like
                // `IncrefValueRegion`: the helper reads the partner's runtime
                // region. Mirrors the interpreter's `handle_join_region`.
                let (pt, pp) = self.use_var_pair(builder, partner.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("JoinRegion without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.join_region, builder.func);
                let rid = builder.ins().iconst(I32, region.get() as i64);
                builder.ins().call(func_ref, &[pt, pp, vm, rid]);
            }
            _ => unreachable!("translate_instr_region: instruction handled earlier in chain"),
        }
        Ok(false)
    }
}
