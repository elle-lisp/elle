//! Register/slot allocation, instruction emission, and block management
//! primitives for the LIR lowerer.

use super::*;

impl<'a> Lowerer<'a> {
    // === Helper Methods ===

    pub(super) fn fresh_reg(&mut self) -> Reg {
        let r = Reg::new(self.next_reg);
        self.next_reg += 1;
        r
    }

    pub(super) fn allocate_slot(&mut self, binding: Binding) -> u16 {
        let env_celled = self.arena.get(binding).needs_capture();
        self.allocate_slot_routed(binding, env_celled)
    }

    /// `allocate_slot` with the env-vs-stack routing decided by the caller.
    /// `env_celled` routes an in-lambda binding to the env address space (a
    /// `capture_locals_mask` bit + an env-relative slot, backed by the
    /// `populate_env` cell); `false` gives a plain stack slot. `lower_letrec`
    /// passes `false` for a compiled-cell letrec binding
    /// (`BindingInner::letrec_compiled_cell`): its slot holds the
    /// `MakeCaptureCell` VALUE, so the env must not mint a shadow cell for it.
    pub(super) fn allocate_slot_routed(&mut self, binding: Binding, env_celled: bool) -> u16 {
        // Inside a lambda, two address spaces coexist:
        //   - Env (captures + params + LBox locals): LoadCapture/StoreCapture
        //   - Stack/register locals (non-LBox let-bound): LoadLocal/StoreLocal
        //
        // Environment layout: [captures..., params..., lbox_locals..., nil_placeholders...]
        // Stack frame layout:  [params..., all_locally_defined...]
        //
        // LBox locals get ENV-relative slots (num_captures + num_locals).
        // Non-LBox locals get STACK-relative slots (num_locals).
        // Both increment num_locals to keep env placeholder slots aligned.
        let slot = if self.in_lambda {
            // local_index is relative to locally-defined vars (after param locals)
            let local_index = self.current_func.num_locals - self.num_local_params;
            // Record EVERY env-celled local, at any index. The mask is unbounded
            // (`CaptureMask`), so a celled slot >= 64 is named precisely
            // instead of relying on a conservative >=64 fallback that also
            // celled — and leaked — uncaptured high locals.
            if env_celled {
                self.current_func
                    .capture_locals_mask
                    .set(local_index as usize);
                // Env-relative: for LoadCapture/StoreCapture
                self.num_captures + self.current_func.num_locals
            } else {
                // Stack-relative: for LoadLocal/StoreLocal
                self.current_func.num_locals
            }
        } else {
            self.current_func.num_locals
        };
        self.current_func.num_locals += 1;
        self.binding_to_slot.insert(binding, slot);
        if !env_celled || !self.in_lambda {
            // Initialize slot to NIL so LoadLocal finds a valid value.
            if let Ok(nil_reg) = self.emit_const(LirConst::Nil) {
                self.emit(LirInstr::StoreLocal { slot, src: nil_reg });
            }
        }
        slot
    }

    /// Extract a compile-time constant value from an HIR node.
    /// Returns `Some(value)` for literals and references to already-known
    /// constants. Used to seed `immutable_values` so reads of immutable
    /// bindings emit `LoadConst` instead of `LoadLocal`.
    pub(super) fn hir_const_value(&self, hir: &Hir) -> Option<Value> {
        match &hir.kind {
            HirKind::Int(n) => Some(Value::int(*n)),
            HirKind::Float(f) => Some(Value::float(*f)),
            HirKind::Bool(b) => Some(Value::bool(*b)),
            HirKind::Nil => Some(Value::NIL),
            HirKind::Keyword(k) => Some(Value::keyword(k)),
            HirKind::Quote(v) => Some(*v),
            // Propagate through references to known constants
            HirKind::Var(b) => self.immutable_values.get(b).copied(),
            _ => None,
        }
    }

    /// If `binding` is immutable and `init` is a compile-time constant,
    /// record it in `immutable_values` so that subsequent reads of this
    /// binding emit `LoadConst` instead of slot loads.
    pub(super) fn try_seed_immutable(&mut self, binding: Binding, init: &Hir) {
        if self.arena.get(binding).is_immutable {
            if let Some(val) = self.hir_const_value(init) {
                self.immutable_values.insert(binding, val);
            }
        }
    }

    pub(super) fn emit(&mut self, instr: LirInstr) {
        self.current_block
            .instructions
            .push(SpannedInstr::new(instr, self.current_span.clone()));
    }

    /// Emit a heap-allocating instruction, building it with the region
    /// assigned by region inference for the current HIR node.
    ///
    /// The region is resolved *first* and handed to `build`, so the region-
    /// bearing variant is constructed with its `region: StaticRegion` field
    /// already set — there is no "build with no region, stamp later" window in
    /// which an allocation could exist without a region. Panics if the solver
    /// assigned no region (Rule 1: every allocation must have one).
    pub(super) fn emit_alloc(&mut self, build: impl FnOnce(StaticRegion) -> LirInstr) {
        let region = self.alloc_region_id().unwrap_or_else(|| {
            panic!(
                "emit_alloc: no region for hir_id {:?} — solver must assign a region to every allocation",
                self.current_hir_id
            )
        });
        self.emit_alloc_with_slot(region, build);
    }

    /// `emit_alloc` with an explicitly named solver region instead of the
    /// current HIR node's `alloc_region` entry — for the one site that emits
    /// SEVERAL allocations at one HirId (`lower_begin`'s capture-cell
    /// pre-pass; one region per cell via `begin_cell_regions`, since N
    /// allocations against one slot orphan all but the last minted physical
    /// region — docs/impl/region/model.md, "one allocation execution per slot between
    /// drops").
    pub(super) fn emit_alloc_in(
        &mut self,
        region: crate::hir::region::Region,
        build: impl FnOnce(StaticRegion) -> LirInstr,
    ) {
        let slot = self.static_slot(region);
        self.emit_alloc_with_slot(slot, build);
    }

    fn emit_alloc_with_slot(
        &mut self,
        region: StaticRegion,
        build: impl FnOnce(StaticRegion) -> LirInstr,
    ) {
        let instr = build(region);
        if crate::config::get().has_trace("rc") {
            // Correlates runtime [trace:rc] alloc_in_region(R) events back
            // to a HirId and source span. Pair with grep on the region id
            // to find the full lifecycle (incref/decref/FREE/cascade), or
            // grep on the payload address from a deref-mismatch panic.
            eprintln!(
                "[trace:rc:emit] emit_alloc hir_id={:?} region={} instr={} span={}",
                self.current_hir_id,
                region,
                instr_kind_name(&instr),
                self.current_span,
            );
        }
        self.emitted_alloc_regions.insert(region);
        self.current_block
            .instructions
            .push(SpannedInstr::new(instr, self.current_span.clone()));
    }

    /// The solver-minted region for `binding`'s pre-allocated capture cell at
    /// the CURRENT node (`begin_cell_regions[current_hir_id]`). Panics if the
    /// solver registered no cell for the binding — the walk's Begin/Let/Letrec
    /// arms must mirror the lowerer's MakeCaptureCell conditions exactly
    /// (Rule 1: every allocation has a region).
    pub(super) fn cell_region_for(
        &self,
        binding: crate::hir::Binding,
    ) -> crate::hir::region::Region {
        self.current_hir_id
            .and_then(|id| self.region_info.begin_cell_regions.get(&id))
            .and_then(|cells| cells.iter().find(|(b, _)| *b == binding).map(|&(_, r)| r))
            .unwrap_or_else(|| {
                panic!(
                    "no cell region for captured binding {:?} at node {:?} — the \
                     solver's begin_cell_regions must mirror the lowerer's \
                     MakeCaptureCell sites",
                    binding, self.current_hir_id
                )
            })
    }

    /// The compiled capture-cell region for `binding` — the same single-cell resolution
    /// the ownership forest's `closure ⊇ cell` re-point uses
    /// ([`RegionInfo::single_cell_region_of`]). Unlike
    /// [`cell_region_for`](Self::cell_region_for), which keys on the CURRENT node, this
    /// resolves a binding whose scope is not the current node — the `closure ⊇ cell`
    /// capture adopt in `lower_lambda_expr` runs at the CLOSURE's construction. `None` for
    /// a binding with no compiled cell OR with an ambiguous multi-cell double-declare (so
    /// analysis and emit agree to name the same cell, or agree to refuse).
    pub(super) fn cell_region_of_binding(
        &self,
        binding: crate::hir::Binding,
    ) -> Option<crate::hir::region::Region> {
        self.region_info.single_cell_region_of(binding)
    }

    /// Look up the region for the current HIR node and return the u16
    /// index into the function's region_table.
    pub(super) fn alloc_region_id(&mut self) -> Option<StaticRegion> {
        let hir_id = self.current_hir_id?;
        let region = *self.region_info.alloc_region.get(&hir_id)?;
        Some(self.static_slot(region))
    }

    /// Look up the static region slot for a scope's region.
    /// Returns `None` if the scope has no region.
    pub(super) fn scope_region_id(&mut self, hir_id: HirId) -> Option<StaticRegion> {
        let region = *self.region_info.scope_region.get(&hir_id)?;
        Some(self.static_slot(region))
    }

    /// Map a solver `Region` to its compile-time bytecode region slot,
    /// minting (and recording in the function's `region_table`) a fresh
    /// `StaticRegion` on first sight and caching it for repeat queries.
    ///
    /// A region in a builder-idiom **merge** tree resolves to its `merged_root`'s
    /// slot, so every member of the tree (child, parent, deeper nests) allocates
    /// against, increfs, and decrefs ONE slot — the root's. This is the emission
    /// half of the merge (docs/impl/region/merging.md § "Emission: one slot per merge
    /// tree, one demise at the root"): canonicalizing here is what makes the child
    /// and parent share a physical region at runtime, and what `record_merged_slots`
    /// detects (two regions resolving to one slot) to flag the slot for
    /// mint-or-reuse. With no merge (`merged_parent` empty — a compile whose
    /// `%pair` allocation nodes seed no builder idiom) `merged_root` is the
    /// identity and this is the unmerged one-region-per-value slot.
    ///
    /// Slots are globally unique (via the atomic counter in
    /// `new_static_region`) so that different compilation units never collide
    /// at runtime — a `DecrefRegion` from one unit must not free objects
    /// stamped by another. Every minted slot is ≥ 2, hence nonzero by construction.
    pub(super) fn static_slot(&mut self, region: crate::hir::region::Region) -> StaticRegion {
        let region = self.region_info.merged_root(region);
        if let Some(&slot) = self.region_to_table.get(&region) {
            slot
        } else {
            let slot = new_static_region();
            self.current_func.region_table.push(slot);
            self.region_to_table.insert(region, slot);
            slot
        }
    }

    /// Mint a fresh static region slot for a synthetic allocation that region
    /// inference does not track — the caller pairs the `MaterializeConst`/alloc
    /// with its own `DecrefRegion` to free it. Recorded in the function's
    /// `region_table` like any solver slot, so every tier (interpreter, JIT)
    /// resolves it to a fresh physical region per activation and the matching
    /// `DecrefRegion` reclaims it. Used by the string PATTERN literal, whose
    /// compare-string is materialized, read once, and freed in place.
    pub(super) fn fresh_managed_region(&mut self) -> StaticRegion {
        let slot = new_static_region();
        self.current_func.region_table.push(slot);
        slot
    }

    pub(super) fn emit_const(&mut self, c: LirConst) -> Result<Reg, String> {
        let dst = self.fresh_reg();
        self.emit(LirInstr::Const { dst, value: c });
        Ok(dst)
    }

    pub(super) fn emit_value_const(&mut self, value: Value) -> Result<Reg, String> {
        let dst = self.fresh_reg();
        self.emit(LirInstr::ValueConst { dst, value });
        Ok(dst)
    }

    pub(super) fn terminate(&mut self, term: Terminator) {
        self.current_block.terminator = SpannedTerminator::new(term, self.current_span.clone());
    }

    pub(super) fn finish_block(&mut self) {
        let block = std::mem::replace(&mut self.current_block, BasicBlock::new(Label(0)));
        self.current_func.blocks.push(block);
    }

    /// Allocate a new basic block label.
    pub(super) fn fresh_label(&mut self) -> Label {
        let label = Label(self.next_label);
        self.next_label += 1;
        label
    }

    /// Finish the current block and start a new one with the given label.
    pub(super) fn start_new_block(&mut self, label: Label) {
        self.finish_block();
        self.current_block = BasicBlock::new(label);
    }

    /// Emit a store for a named binding.
    pub(super) fn emit_binding_store(&mut self, slot: u16, src: Reg) {
        self.emit(LirInstr::StoreLocal { slot, src });
    }

    /// The function's lazily-allocated scratch slot — shared by value
    /// discards and the discarded-call-result release roundtrip
    /// (`emit_decrefs_for`). Contents are garbage between uses.
    pub(super) fn scratch_slot(&mut self) -> u16 {
        match self.discard_slot {
            Some(s) => s,
            None => {
                let s = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.discard_slot = Some(s);
                s
            }
        }
    }

    /// Discard an unused value by storing it to a scratch slot.
    /// StoreLocal does not incref, so no refcount tracking needed.
    pub(super) fn discard(&mut self, src: Reg) {
        let slot = self.scratch_slot();
        self.emit(LirInstr::StoreLocal { slot, src });
    }
}
