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
        // Track the slot of a fn-local reassigned mutable binding so
        // `emit_decrefs_for` never nil-stamps it mid-scope (the reassigned-loop-
        // counter clobber — see `reassigned_local_slots`). A `false` env_celled
        // binding takes a plain stack slot; a captured one is a cell released by
        // `DecrefCellRegion`, not the value route, so it need not be tracked.
        if !env_celled
            && self
                .region_info
                .reassigned_local_bindings
                .contains(&binding)
        {
            self.reassigned_local_slots.insert(slot);
        }
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
        // The relocation points die with the block by default: the next block
        // the lowerer opens is, in general, one this tail call's path is not a
        // predecessor of at all, and a replica spliced into a point that path
        // never reaches is a release added where none was owed. Only a branch
        // merge knows otherwise, and it re-installs them itself
        // (`open_branch_merge`, docs/impl/region/mechanism.md § "The relocation
        // point outlives the block").
        self.tail_exit_hoist.clear();
        let block = std::mem::replace(&mut self.current_block, BasicBlock::new(Label(0)));
        self.current_func.blocks.push(block);
    }

    /// Start collecting the relocation points of a branch's arms, returning the
    /// enclosing branch's collection for `open_branch_merge` to restore.
    ///
    /// The three branch lowerings bracket their arms with this pair; a branch
    /// nested inside an arm therefore collects into its own list and hands its
    /// union up as that arm's contribution.
    pub(super) fn begin_branch_arms(&mut self) -> Vec<super::TailExitHoist> {
        std::mem::take(&mut self.arm_exit_hoists)
    }

    /// Seal the relocation points of the arm-final block into the branch's
    /// collection. Called immediately before the arm's `finish_block`, while the
    /// block index that call is about to assign is still predictable.
    ///
    /// Rebasing here is what keeps every sealed point addressable: `blocks` is
    /// only ever appended to, so the index this arm is about to take stays valid
    /// for the rest of the function. A point naming some *other* block cannot be
    /// spliced into from here and is dropped rather than carried stale.
    pub(super) fn seal_arm_hoists(&mut self) {
        let index = self.current_func.blocks.len();
        let label = self.current_block.label;
        let sealed = self
            .tail_exit_hoist
            .drain(..)
            .filter_map(|mut h| match h.block {
                super::HoistBlock::Current(l) if l == label => {
                    h.block = super::HoistBlock::Finished(index);
                    Some(h)
                }
                super::HoistBlock::Current(_) => None,
                super::HoistBlock::Finished(_) => Some(h),
            });
        self.arm_exit_hoists.extend(sealed);
    }

    /// Hand the merge block just opened the points its arms sealed, and restore
    /// the enclosing branch's collection.
    ///
    /// Every path into a merge arrives through one of the arms, so these points
    /// together cover the merge — which is what licenses replicating a release
    /// emitted here back into each of them.
    pub(super) fn open_branch_merge(&mut self, saved: Vec<super::TailExitHoist>) {
        self.tail_exit_hoist = std::mem::replace(&mut self.arm_exit_hoists, saved);
    }

    /// Open the relocation point a frame-replacing tail call leaves behind: the
    /// `TailCall` was just emitted as the last instruction of `current_block`,
    /// and every release the lowerer emits after it into this block runs only on
    /// the native fall-through (docs/impl/region/mechanism.md § "A release past a
    /// frame-replacing tail call is not a release").
    ///
    /// `exempt` is read off the call itself — the regions the callee, an operand's
    /// own VALUE, or the call's own result placeholder name — because those are
    /// exactly the ones the tail call can still reach, and their releases are
    /// owed to the ownership move, to the activation that takes over the callee's
    /// region, and to the caller that consumes the result.
    pub(super) fn open_tail_exit_hoist(
        &mut self,
        call_id: HirId,
        func: &Hir,
        args: &[crate::hir::CallArg],
        operands: &[Reg],
    ) {
        // Which slots the operands now on the stack came from. Read off the
        // emitted instructions rather than the HIR, because ANF may have bound an
        // operand to a synthetic binding whose region the syntax walk below does
        // not connect back to this call — but the load that put it on the stack
        // is right here either way.
        let mut operand_locals = rustc_hash::FxHashSet::default();
        let mut operand_captures = rustc_hash::FxHashSet::default();
        for i in &self.current_block.instructions {
            match &i.instr {
                LirInstr::LoadLocal { dst, slot } if operands.contains(dst) => {
                    operand_locals.insert(*slot);
                }
                LirInstr::LoadCapture { dst, index } | LirInstr::LoadCaptureRaw { dst, index }
                    if operands.contains(dst) =>
                {
                    operand_captures.insert(*index);
                }
                _ => {}
            }
        }
        let mut exempt = rustc_hash::FxHashSet::default();
        if let Some(&r) = self.region_info.alloc_region.get(&call_id) {
            exempt.insert(self.region_info.merged_root(r));
        }
        // The merged arena a letrec body's tail call hands to the runtime's
        // deferred release (`TailCall::deferred_release_slot`,
        // docs/impl/region/letrec.md): its binding-scope `DecrefRegion` is dead
        // past the frame replacement BY DESIGN, and the deferred channel supplies
        // it. Hoisting it would make both fire.
        if let Some(&root) = self.region_info.cycle_tail_release.get(&call_id) {
            exempt.insert(self.region_info.merged_root(root));
        }
        self.collect_operand_regions(func, &mut exempt);
        for a in args {
            self.collect_operand_regions(&a.expr, &mut exempt);
        }
        let capture_funded = self
            .region_info
            .tail_callee_facts
            .get(&call_id)
            .map(|f| f.capture_funded.clone())
            .unwrap_or_default();
        // This call dominates every position after it in the block, so it alone
        // covers them — any points a merge left here name arms that reach this
        // call, not the releases that follow it.
        self.tail_exit_hoist.clear();
        self.tail_exit_hoist.push(super::TailExitHoist {
            at: self.current_block.instructions.len() - 1,
            block: super::HoistBlock::Current(self.current_block.label),
            operand_locals,
            operand_captures,
            exempt,
            capture_funded,
        });
    }

    /// Every region one tail-call OPERAND — the callee, or an argument — may hand
    /// the call, read off [`crate::hir::region::RegionInfo::operand_value_regions`]
    /// so this exemption and the branch-arm window's per-point funding question ask
    /// one reading rather than two.
    fn collect_operand_regions(
        &self,
        h: &Hir,
        out: &mut rustc_hash::FxHashSet<crate::hir::region::Region>,
    ) {
        self.region_info.operand_value_regions(h, out);
    }

    /// Emit `f`'s instructions for `region`, placed so that every path runs the
    /// release exactly once even where a frame-replacing tail call stands between
    /// this position and the paths that reach it.
    ///
    /// Two placements, decided by where the relocation points sit:
    ///
    /// - **A point in this block** — the tail call was emitted here, so the run is
    ///   MOVED ahead of it. The same single instruction sequence, at one position,
    ///   reached by the closure path and the native fall-through alike.
    /// - **Points a merge inherited from its arms** — those blocks are closed, so
    ///   the run is emitted here AND replicated at each point. This counts once per
    ///   path only because a value-routed release nil-stamps the slot it read
    ///   ([`Self::self_cancelling_run`]); a run without that stamp keeps the
    ///   baseline.
    ///
    /// Neither placement waives the obligations. Escape supplies the count
    /// argument — on a closure path the release fires where none did before, and no
    /// premise about instruction placement can supply it: a value the tail callee
    /// reaches through its captured environment is named by no argument and by no
    /// callee region, yet the call reads it. `sole_frame_held_regions` is that
    /// argument for a region nobody reads once the frame is gone;
    /// `return_frame_held_regions` plus the point's own `capture_funded` edge is
    /// the argument for one the CALLEE hands back, where the caller's reference is
    /// minted after this release runs. `TailExitHoist::exempt` and
    /// [`Self::hoistable_run`] are the two readings of what each call itself names
    /// — both needed, because ANF is free to rewrite how an operand is spelled —
    /// and they are asked per point, so one arm's ownership move does not hold back
    /// its siblings.
    ///
    /// The emitted sequences are all stack-neutral — each `LoadLocal` push is
    /// consumed by the release that follows it — so splicing them between the
    /// pushed arguments and the call leaves the tail call's operand layout intact.
    pub(super) fn with_tail_exit_hoist(
        &mut self,
        region: crate::hir::region::Region,
        mut f: impl FnMut(&mut Self),
    ) {
        let root = self.region_info.merged_root(region);
        // Two admissions, and which one applies decides whether the point's own
        // funding is consulted. A SOLE-held region is read by nobody once the
        // frame is gone, so any point may take it. A region held by the return
        // facet alone IS read afterwards — by the caller, through the reference
        // the tail callee's `Return` mints after this release would run — so it
        // needs the callee's captured-holder edge to span the gap, which is a fact
        // about the point rather than the region.
        let sole = self.region_info.sole_frame_held_regions.contains(&root);
        if self.tail_exit_hoist.is_empty()
            || !(sole || self.region_info.return_frame_held_regions.contains(&root))
        {
            f(self);
            return;
        }
        let admitted = |h: &super::TailExitHoist| {
            !h.exempt.contains(&root) && (sole || h.capture_funded.contains(&root))
        };
        // The two placements never mix: a tail call emitted into this block
        // dominates every position after it, so `open_tail_exit_hoist` drops the
        // merge's points in favour of its own single one.
        debug_assert!(
            self.tail_exit_hoist.len() == 1
                || !self
                    .tail_exit_hoist
                    .iter()
                    .any(|h| matches!(h.block, super::HoistBlock::Current(_))),
            "a relocation point in the open block must be the only one"
        );
        if let super::HoistBlock::Current(label) = self.tail_exit_hoist[0].block {
            // A tail call in this very block dominates everything after it, so it
            // is the only point and the placement is the move.
            if label != self.current_block.label || !admitted(&self.tail_exit_hoist[0]) {
                f(self);
                return;
            }
            let at = self.tail_exit_hoist[0].at;
            let start = self.current_block.instructions.len();
            f(self);
            let moved: Vec<_> = self.current_block.instructions.drain(start..).collect();
            if moved.is_empty() || !self.hoistable_run(0, &moved) {
                self.current_block.instructions.extend(moved);
                return;
            }
            self.tail_exit_hoist[0].at += moved.len();
            self.current_block.instructions.splice(at..at, moved);
            return;
        }
        // Points inherited from the arms of a branch. The release stays here for
        // the paths that fall through to this merge, and a replica goes ahead of
        // each arm's tail call for the paths that leave through it.
        let start = self.current_block.instructions.len();
        f(self);
        if !self.self_cancelling_run(&self.current_block.instructions[start..]) {
            return;
        }
        for i in 0..self.tail_exit_hoist.len() {
            let super::HoistBlock::Finished(block) = self.tail_exit_hoist[i].block else {
                continue;
            };
            if !admitted(&self.tail_exit_hoist[i]) {
                continue;
            }
            // Re-run rather than clone the emitted run: each replica then names
            // its own registers, so no register is defined twice.
            let start = self.current_block.instructions.len();
            f(self);
            let copy: Vec<_> = self.current_block.instructions.drain(start..).collect();
            if !self.hoistable_run(i, &copy) || !self.self_cancelling_run(&copy) {
                continue;
            }
            let at = self.tail_exit_hoist[i].at;
            self.tail_exit_hoist[i].at += copy.len();
            self.current_func.blocks[block]
                .instructions
                .splice(at..at, copy);
        }
    }

    /// Does this run release by VALUE and nil-stamp the slot it read?
    ///
    /// That stamp is what lets one release be emitted at a merge and replicated
    /// ahead of a branch arm's tail call: whichever copy a path reaches first does
    /// the work and blanks the holder slot, and any later copy loads `nil`, whose
    /// release is a no-op. It is the same discipline that lets a branch's per-arm
    /// compensations coexist with its `decref_point`.
    ///
    /// Everything else keeps the baseline, and the exclusions are the point rather
    /// than an accident of the vocabulary: a `DecrefRegion` by region id, a capture
    /// cell's `DecrefCellRegion` and the transfer `AdoptIntoActivation` all leave
    /// the holder as it was, so a second copy on one path would count twice.
    fn self_cancelling_run(&self, run: &[SpannedInstr]) -> bool {
        let mut released: Option<u16> = None;
        let mut nil_regs: rustc_hash::FxHashSet<Reg> = rustc_hash::FxHashSet::default();
        let mut loaded: rustc_hash::FxHashMap<Reg, u16> = rustc_hash::FxHashMap::default();
        let mut stamped = false;
        for i in run {
            match &i.instr {
                LirInstr::LoadLocal { dst, slot } => {
                    loaded.insert(*dst, *slot);
                }
                LirInstr::Const {
                    dst,
                    value: LirConst::Nil,
                } => {
                    nil_regs.insert(*dst);
                }
                LirInstr::DecrefValueRegion { src } => match loaded.get(src) {
                    // One release, of a value this run loaded from a slot.
                    Some(slot) if released.is_none() => released = Some(*slot),
                    _ => return false,
                },
                LirInstr::StoreLocal { slot, src } => {
                    if released == Some(*slot) && nil_regs.contains(src) {
                        stamped = true;
                    }
                }
                _ => return false,
            }
        }
        stamped
    }

    /// May this just-emitted release run ahead of the tail call at point `index`?
    ///
    /// Two refusals, both about what the instructions themselves name rather than
    /// what the HIR said:
    ///
    /// - **It reloads an operand's slot.** The value now on the operand stack came
    ///   from that slot, so this release is the ownership move the callee's
    ///   owned-param release consumes — running it here drops the callee's
    ///   reference (`TailExitHoist::operand_locals`).
    /// - **It reads a register defined outside the run.** The only such register a
    ///   release names is the enclosing node's just-lowered value, which for a node
    ///   whose subtree ends in the tail call is produced BY that call — a
    ///   definition the hoist point precedes. (This is the discarded-result route
    ///   of `emit_decrefs_for`.)
    fn hoistable_run(&self, index: usize, run: &[SpannedInstr]) -> bool {
        let Some(h) = self.tail_exit_hoist.get(index) else {
            return false;
        };
        let mut defined: rustc_hash::FxHashSet<Reg> = rustc_hash::FxHashSet::default();
        for i in run {
            match &i.instr {
                LirInstr::LoadLocal { dst, slot } => {
                    if h.operand_locals.contains(slot) {
                        return false;
                    }
                    defined.insert(*dst);
                }
                LirInstr::LoadCapture { dst, index } | LirInstr::LoadCaptureRaw { dst, index } => {
                    if h.operand_captures.contains(index) {
                        return false;
                    }
                    defined.insert(*dst);
                }
                LirInstr::Const { dst, .. } => {
                    defined.insert(*dst);
                }
                LirInstr::StoreLocal { src, .. }
                | LirInstr::DecrefValueRegion { src }
                | LirInstr::DecrefCellRegion { src }
                | LirInstr::AdoptIntoActivation { child: src } => {
                    if !defined.contains(src) {
                        return false;
                    }
                }
                LirInstr::DecrefRegion { .. } => {}
                // Anything else is outside the release vocabulary this wrapper
                // was written for; leave it where the lowerer put it.
                _ => return false,
            }
        }
        true
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
