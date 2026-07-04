use super::*;

impl<'a> Lowerer<'a> {
    /// Record the slot owning the result of an allocating
    /// expression at `hir_id`, keyed by its allocation region.
    /// Called from `lower_let` / `lower_letrec` / `lower_define`
    /// after `allocate_slot`. After ANF, every Call/Lambda/Eval/
    /// allocating-intrinsic in a consumer position is bound to a
    /// synthetic Let — so this map covers the result via its binding
    /// slot directly, without a separate stash-and-reload slot.
    pub(super) fn record_region_slot(&mut self, hir_id: HirId, slot: u16) {
        if let Some(&r) = self.region_info.alloc_region.get(&hir_id) {
            self.region_to_slot.insert(r, slot);
        }
    }
    /// Store a top-level captured binding's init value into its pre-allocated
    /// `MakeCaptureCell` (the binding `slot` holds the CELL, created nil by the
    /// `lower_begin`/`lower_letrec` pre-pass).
    ///
    /// `reassigned` selects how the init value's ALLOC reference is dropped:
    ///
    /// - `false` (the binding is never reassigned): the ordinary route. The
    ///   caller leaves `record_region_slot(init → slot)` in place, so the init
    ///   region's `DecrefValueRegion` reloads the cell at its `decref_point` and
    ///   `result_region_of` unwraps to the cell's content — which, with no
    ///   reassignment, is always exactly this init value. Nothing extra here.
    ///
    /// - `true` (the binding is reassigned): the cell content CHANGES, so a
    ///   later slot-load + unwrap would free a different, live value (the
    ///   capture-cell reassign UAF; region-capture-cell-reassign-uaf.lisp). The
    ///   caller SKIPS `record_region_slot` for the init, and we drop its alloc
    ///   reference HERE off `value_reg` directly. `StoreCaptureCell`
    ///   (`handle_update_capture`) already raised the value's region for the
    ///   cell's membership; this releases the producer's reference, leaving
    ///   exactly the cell's. That membership reference is reclaimed by the cell's
    ///   free cascade (the final value) or by the next reassignment's
    ///   drop-on-overwrite.
    ///
    ///   This drop is transform 1's **decref side** (docs/impl/region-rules.md
    ///   § "Compile-time region selection (coalescing)"): when `value` is a fresh
    ///   local allocation whose region is a known slot (the usual case for a
    ///   captured binding's init), the release is slot-resolved
    ///   (`DecrefRegion`, guarded under `debug_assertions` by the equivalence
    ///   oracle `AssertRegionMatches`), otherwise it stays value-resolved. The
    ///   guard refuses any captured/cross-thread region (`coalescible_region` —
    ///   the slot must be stamped by an allocation emitted in this function).
    ///   `DecrefRegion` touches no operand stack, so `value_reg` is left on top
    ///   exactly as the never-reassigned (`false`) path leaves it — a benign
    ///   orphan the block-end cleanup consumes.
    pub(super) fn store_captured_cell_init(
        &mut self,
        slot: u16,
        value_reg: Reg,
        value: &Hir,
        reassigned: bool,
    ) {
        let cell_reg = self.fresh_reg();
        self.emit(LirInstr::LoadLocal {
            dst: cell_reg,
            slot,
        });
        self.emit(LirInstr::StoreCaptureCell {
            cell: cell_reg,
            value: value_reg,
        });
        if reassigned {
            let coalesced = self.coalescible_region(value);
            super::rcstats::record_captured_init(coalesced.is_some());
            match coalesced {
                Some(region_id) => {
                    #[cfg(debug_assertions)]
                    self.emit(LirInstr::AssertRegionMatches {
                        region_id,
                        src: value_reg,
                    });
                    self.emit(LirInstr::DecrefRegion { region_id });
                }
                None => self.emit(LirInstr::DecrefValueRegion { src: value_reg }),
            }
        }
    }
    /// Record `region_to_slot[cell_r] = slot` for a captured local's env-cell
    /// placeholder (the analysis put it in `binding_source_regions[binding]` and
    /// `cell_release_regions`; see `RegionInference::env_cell_placeholder`). This
    /// lets `emit_decrefs_for` release the env cell at the binding's last use via
    /// `LoadCaptureRaw` + `DecrefCellRegion`. `slot` is the binding's env/upvalue
    /// index (the same index `StoreCapture`/`LoadCapture` use). Mirrors the
    /// captured-param recording in `lower_lambda_body`, but per-binding because a
    /// local's slot is only known once its define/let is lowered (mid-body), and
    /// `record_region_slot` keys off `alloc_region`, which a phantom placeholder
    /// is deliberately absent from.
    pub(super) fn record_env_cell_release_slot(&mut self, binding: Binding, slot: u16) {
        let Some(regions) = self.region_info.binding_source_regions.get(&binding) else {
            return;
        };
        for &r in regions.clone().iter() {
            if self.region_info.cell_release_regions.contains(&r) {
                self.region_to_slot.insert(r, slot);
            }
        }
    }
    /// The static region **slot** to coalesce a value's mint onto, or `None` to
    /// stay value-resolved (docs/impl/region-rules.md § "Compile-time region
    /// selection (coalescing)"). Layers the lowering-time runtime-population guard
    /// over [`coalescible_solver_region`]'s solver-fact class logic: the region's
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
    pub(super) fn coalescible_region(
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
    /// (`suppressed_decref_regions`), or a reassigned 1-slot-container value
    /// region (`mutated_binding_value_regions`, which also catches a returned
    /// `Var` aliasing a store target — escape.md divergence 2).
    ///
    /// A returned `Var` whose `binding_source_regions` names *more than one*
    /// region is a branch-dependent mix — not statically nameable — so it is
    /// refused. Pure: no emission, no `region_table` mutation.
    pub(super) fn coalescible_solver_region(
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
        {
            return None;
        }
        Some(region)
    }

    /// Emit `AdoptRegion(parent, child)` for an interior owned-subtree edge: load
    /// both values from their binding slots and link the child's runtime region
    /// into the parent's Owned subtree (docs/impl/region-model.md § "Adoption and
    /// subtree drop"). A missing slot for an endpoint (no value-resolved home)
    /// skips the adopt — the regions then stay independently RC'd, the always-legal
    /// fallback (the frozen-RC contract makes the skip correctness-neutral).
    fn emit_adopt_region(
        &mut self,
        child: crate::hir::region::Region,
        parent: crate::hir::region::Region,
    ) {
        let (Some(&pslot), Some(&cslot)) = (
            self.region_to_slot.get(&parent),
            self.region_to_slot.get(&child),
        ) else {
            return;
        };
        let preg = self.fresh_reg();
        self.emit(LirInstr::LoadLocal {
            dst: preg,
            slot: pslot,
        });
        let creg = self.fresh_reg();
        self.emit(LirInstr::LoadLocal {
            dst: creg,
            slot: cslot,
        });
        self.emit(LirInstr::AdoptRegion {
            parent: preg,
            child: creg,
        });
        if crate::config::get().has_trace("rc") {
            eprintln!(
                "[trace:rc:emit] adopt_region child={} parent={}",
                child.0, parent.0
            );
        }
    }

    /// Emit `AdoptIntoActivation` for each member of a capture-back-edge SCC:
    /// load the member's value from its binding slot and adopt its runtime
    /// region into the executing activation's owner node
    /// (docs/impl/region-model.md § "Owner nodes" — "The capture-back-edge
    /// SCC"). The adopt transfers ownership only; the free is the node's release
    /// at the activation's completion. A member with no slot is skipped — its
    /// region stays `Counted` and, its decref being suppressed, over-kept to
    /// teardown: a bounded fallback, never a double-free. Slots are deduped so
    /// two member regions resolving to one slot (a branch-dependent union)
    /// adopt once, keeping the runtime's one-adoption assert unreachable (the
    /// admission's pairwise-distinct-holder gate makes both cases unreachable
    /// in practice; this is the emit-side belt).
    pub(super) fn emit_adopt_into_activation(&mut self, members: &[crate::hir::region::Region]) {
        let mut seen: rustc_hash::FxHashSet<u16> = rustc_hash::FxHashSet::default();
        for &m in members {
            let Some(&slot) = self.region_to_slot.get(&m) else {
                continue;
            };
            if !seen.insert(slot) {
                continue;
            }
            let reg = self.fresh_reg();
            self.emit(LirInstr::LoadLocal { dst: reg, slot });
            self.emit(LirInstr::AdoptIntoActivation { child: reg });
            if crate::config::get().has_trace("rc") {
                eprintln!(
                    "[trace:rc:emit] adopt_into_activation member={} local_slot={}",
                    m.0, slot
                );
            }
        }
    }

    /// Emit IncrefRegion for any cross-region references at this HIR node.
    pub(super) fn emit_increfs_for(&mut self, hir_id: HirId) {
        // Ownership forest: an interior edge of an externally-unique Owned subtree
        // becomes an `AdoptRegion` (parent adopts the child's region; no RC),
        // emitted here in place of the edge's `IncrefRegion`. `owned_adopt_edges`
        // is empty unless `--region-ownership`, so this is inert on the baseline
        // path (docs/impl/region-model.md § "Adoption and subtree drop").
        let adopt_edges = self
            .region_info
            .owned_adopt_edges
            .get(&hir_id)
            .cloned()
            .unwrap_or_default();
        for &(child, parent) in &adopt_edges {
            self.emit_adopt_region(child, parent);
        }
        // O(1) lookup into the site-indexed map built in
        // `with_region_info` (was a linear scan of every cross-region ref
        // per node — O(n²) over a large compilation unit). Each entry is an
        // edge `(source, target)`: `source` is the region the incref names,
        // `target` rides along so a post-merge intra-region self-edge can be
        // detected (`is_merge_self_edge`) and dropped (transform 2, below).
        let refs: Vec<_> = match self.increfs_by_site.get(&hir_id) {
            Some(edges) => edges.clone(),
            None => return,
        };
        let hard_site = self.region_info.hard_edge_sites.contains(&hir_id);
        for (src, dst) in refs {
            // An interior owned-subtree edge's reference count is replaced by the
            // `AdoptRegion` emitted above (the subtree frees as a unit) — skip its
            // incref. Inert on the baseline path (`adopt_edges` empty).
            if adopt_edges.contains(&(src, dst)) {
                continue;
            }
            // A call-result region is a marker, not a prediction: its static
            // slot is never populated at runtime (only alloc opcodes record
            // region mints in the activation map), so a slot-based
            // `IncrefRegion` against it resolves to nothing and the edge's
            // balancing decref — the store target's free-time cascade — then
            // steals a live reference (the call-result-arg clique UAF,
            // tests/elle/region-native-clique-callresult-uaf.lisp). At a HARD
            // edge site (a declared native uncounted-store effect —
            // docs/impl/region-effects.md "Hard edges") incref by VALUE instead, the
            // exact mirror of `emit_decrefs_for`'s call-result branch: load
            // the value from its binding slot and retain the runtime region
            // it actually lives in. Opaque user-fn sites keep the slot path
            // (the no-op — a real incref there never balances
            // when the callee stores through the runtime funnel;
            // region-userfn-clique-callresult-noleak.lisp). Cell phantom
            // placeholders keep the slot path too (their `region_to_slot`
            // entry is an upvalue index, not a local slot).
            if hard_site
                && self.region_info.call_result_regions.contains(&src)
                && !self.region_info.cell_release_regions.contains(&src)
            {
                if let Some(&slot) = self.region_to_slot.get(&src) {
                    let val_reg = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal { dst: val_reg, slot });
                    self.emit(LirInstr::IncrefValueRegion { src: val_reg });
                    // `IncrefValueRegion` peeks (Return-position contract: the
                    // value must stay on top for the caller); mid-stream that
                    // leaves an unconsumed entry that skews the emitter's
                    // stack model. Store the value back to its own slot — a
                    // semantic no-op whose emission consumes the entry.
                    self.emit(LirInstr::StoreLocal { slot, src: val_reg });
                }
                // No slot: nothing to load — the same net no-op as the
                // unpopulated-slot `IncrefRegion` this replaces.
                continue;
            }
            // Self-edge elimination (transform 2; docs/impl/region-rules.md
            // § "Self-edge elimination"). A builder-idiom merge collapses this
            // `src → dst` store edge's endpoints onto one region (they share a
            // `merged_root`), making it an intra-region self-edge. The free-time
            // cascade skips a region's references into itself
            // (`regionpool/introspect.rs`, `rid != own_id`), so its
            // `IncrefRegion(root)` would have no balancing decref — keeping it leaks
            // the merged region. Drop it. This is the emission counterpart of
            // `static_slot`'s canonicalization and `emit_decrefs_for`'s child-decref
            // suppression: the three move together, taking the edge from
            // cross-region (incref + cascade decref) to intra-region (no RC).
            if self.region_info.is_merge_self_edge(src, dst) {
                super::rcstats::record_self_edge_eliminated();
                if crate::config::get().has_trace("rc") {
                    eprintln!(
                        "[trace:rc:emit] merge_self_edge_eliminated source={} target={} hir_id={:?} span={}",
                        src.0, dst.0, hir_id, self.current_span
                    );
                }
                continue;
            }
            let region_id = self.static_slot(src);
            self.emit(LirInstr::IncrefRegion { region_id });
        }
    }
    /// Record the static slots a builder-idiom merge collapses two or more
    /// allocations onto — the `merged_slots` set the runtime mint-or-reuses
    /// (docs/impl/region-model.md § Merging) — into the current function, after its
    /// body is lowered (so `region_to_table` holds this function's slots). Called at
    /// each function's finalization (the entry in `lower`, every lambda in
    /// `lower_lambda_body`).
    ///
    /// Every member of a merge tree resolves to the **root's** slot — `static_slot`
    /// canonicalizes through `merged_root` — so the shared slot is the root's, read
    /// from the (root-keyed) `region_to_table`. The runtime-population guard
    /// (`emitted_alloc_regions`, mirroring `coalescible_region`) keeps a slot no
    /// allocation in THIS function stamped out of the set: such a slot has no
    /// activation mapping to reuse. With no merge (`merged_parent` empty — the
    /// `--checked-intrinsics=on` default seeds none) this returns immediately and
    /// `merged_slots` stays empty, so mint-or-reuse is the plain mint and the change
    /// is behaviour-preserving on that path.
    pub(super) fn record_merged_slots(&mut self) {
        if self.region_info.merged_parent.is_empty() {
            return;
        }
        let mut merged: rustc_hash::FxHashSet<StaticRegion> = rustc_hash::FxHashSet::default();
        for &child in self.region_info.merged_parent.keys() {
            // The merged slot is the root's; read it (never mint) and keep it only
            // if an allocation emitted in this function stamped it.
            let root = self.region_info.merged_root(child);
            if let Some(&slot) = self.region_to_table.get(&root) {
                if self.emitted_alloc_regions.contains(&slot) {
                    merged.insert(slot);
                }
            }
        }
        // Decref-dominance: each merged slot must carry exactly one `DecrefRegion`
        // (the root's; non-root children are suppressed in `emit_decrefs_for`), so
        // the single drop frees the whole merged region and clears the slot once
        // per activation — the invariant mint-or-reuse relies on (the next loop
        // iteration's child re-mints against the cleared slot). A merge that cannot
        // prove this is never recorded; the unmerged baseline (always legal)
        // stands.
        #[cfg(debug_assertions)]
        for &slot in &merged {
            let decref_count = self
                .current_func
                .blocks
                .iter()
                .flat_map(|b| b.instructions.iter())
                .filter(|si| {
                    matches!(&si.instr, LirInstr::DecrefRegion { region_id } if *region_id == slot)
                })
                .count();
            debug_assert_eq!(
                decref_count, 1,
                "merged slot {} must carry exactly one DecrefRegion \
                 (decref-dominance for mint-or-reuse); got {}",
                slot, decref_count
            );
        }
        self.current_func.merged_slots = merged.into_iter().collect();
    }
    /// Emit pending `DecrefRegion` instructions. Called at tail-call
    /// sites where region cleanup is deferred. Deduplicates to avoid
    /// double-decrementing regions shared between nested scopes.
    pub(super) fn emit_pending_free_regions(&mut self) {
        let pending: Vec<StaticRegion> = self.pending_free_regions.clone();
        let mut seen = std::collections::HashSet::new();
        for region_id in pending {
            if seen.insert(region_id) {
                self.emit_decref_region(region_id);
            }
        }
    }
}
