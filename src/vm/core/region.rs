// audited: 2026-10-21
//! Resolving a compiler region slot to the physical region this activation allocates into, drops, or hands a spliced call.
//!
//! docs/impl/region/model.md
//! docs/impl/region/merging.md

use super::*;

mod abandoned;
mod dues;
mod natives;

impl VM {
    /// Record where a just-minted region came from, for `--trace=arena`
    /// (docs/impl/region/diagnostics.md). `arena/dump` prints it beside the
    /// region's tags, so a region a residue window reports as retained names the
    /// Elle code that made it.
    ///
    /// The site is the allocating instruction's function and location, which the
    /// interpreter loop sets in `arena_site`. A mint without one falls back to
    /// the innermost `CallFrame`: its function and the place it was called from.
    /// `kind` names the mint, and `slot` is the compiler's region slot where the
    /// mint has one. `--dump=regions` on the running function maps `rN` back to
    /// the allocation node, which the call site alone cannot do when a function
    /// allocates in several places.
    ///
    /// Nothing is formatted unless the bit is set, so the allocation path pays
    /// one relaxed atomic load in an ordinary run.
    #[inline]
    pub(crate) fn note_region_mint(
        &mut self,
        id: RuntimeRegion,
        kind: &str,
        slot: Option<StaticRegion>,
    ) {
        if !self
            .runtime_config
            .has_trace_bit(crate::config::trace_bits::ARENA)
        {
            return;
        }
        let slot = match slot {
            Some(s) => format!(" r{}", s.get()),
            None => String::new(),
        };
        // The allocating instruction names itself. The call stack is the
        // fallback for a mint the interpreter loop did not set a site for — a
        // compiled frame, or one made off an instruction entirely.
        let site = match &self.arena_site {
            Some((loc, name)) => format!(
                "{} [{}{}] at {}",
                name.unwrap_or("<anonymous>"),
                kind,
                slot,
                loc
            ),
            None => match self.fiber.call_stack.last() {
                Some(f) => {
                    let loc = match f.location() {
                        Some(l) => format!("{l}"),
                        None => "?".to_string(),
                    };
                    format!("{} [{}{}] called at {}", f.name(), kind, slot, loc)
                }
                None => format!("<no frame> [{kind}{slot}]"),
            },
        };
        self.heap().note_region_mint_site(id.get(), site.into());
    }

    /// Resolve a static region slot to a **fresh** physical region for an
    /// allocation that has a matching compiler-emitted `DecrefRegion` at its
    /// decref_point (Pair, arrays, structs, closures, capture cells).
    ///
    /// Tofte-Talpin: **every allocation execution gets its own physical region,
    /// period.** Merging is the only thing that may collapse regions onto a
    /// shared slot, and a merged slot routes through
    /// `runtime_region_for_alloc_slot_maybe_merged` /
    /// `runtime_region_for_merged_alloc_slot` instead of here. So this mints a
    /// fresh region on every call and stores `slot → physical` in the current
    /// activation frame *for the matching `DecrefRegion` to find*.
    /// `take_runtime_region_for_drop_slot` reads then clears it, and this does
    /// **not** return a cached region.
    ///
    /// Returning the cached entry would let a *re-executed* slot whose
    /// `DecrefRegion` did not run reuse the prior region, commingling distinct
    /// values in one region (docs/impl/region/rules.md). A `while` loop's
    /// reachable `DecrefRegion` clears the slot each iteration, so the map is
    /// already empty at the next alloc. A frame-replacing tail call empties the
    /// map for its callee.
    #[inline]
    pub(crate) fn runtime_region_for_alloc_slot(
        &mut self,
        static_id: StaticRegion,
    ) -> RuntimeRegion {
        // Each alloc slot mints a fresh region per execution. (There is no
        // slot-0 case: the operand is a `StaticRegion`, always ≥ 1.)
        let phys = self.heap().new_runtime_region();
        let gen = self.heap().generation_raw(phys.get());
        self.note_region_mint(phys, "alloc", Some(static_id));
        self.fiber
            .activation_region_maps
            .last_mut()
            .expect("region frame stack must be non-empty")
            .insert(static_id.get(), MappedRegion::new(phys, gen));
        phys
    }
    /// Resolve a static region slot for an allocation, honoring builder-idiom
    /// **merging** (docs/impl/region/merging.md).
    ///
    /// For a slot NOT in `merged_slots` this is exactly
    /// [`Self::runtime_region_for_alloc_slot`] — mint a fresh physical region
    /// every execution and overwrite the activation mapping (the unmerged
    /// one-region-per-value baseline). A MERGED slot is one shared by ≥2 alloc
    /// instructions after a merge. The FIRST member to execute (the child) finds
    /// the slot unmapped and mints `R`. A LATER member (the parent) finds the
    /// slot already mapped and **reuses** `R`, so every member lands in one
    /// region freed by the single `DecrefRegion`. Per-iteration uniqueness in
    /// loops holds because that `DecrefRegion` clears the slot
    /// (`take_runtime_region_for_drop_slot`) each iteration, so the next
    /// iteration's first member mints fresh.
    ///
    /// `merged_slots` is the current function's set (from its `Code`). It is empty
    /// unless a builder-idiom merge fired (a nested `%pair` literal seeding the
    /// merge), so this is byte-identical to the plain mint when no merge exists.
    #[inline]
    pub(crate) fn runtime_region_for_alloc_slot_maybe_merged(
        &mut self,
        static_id: StaticRegion,
        merged_slots: crate::value::closure::MergedSlots<'_>,
    ) -> RuntimeRegion {
        if merged_slots.contains(static_id.get()) {
            return self.runtime_region_for_merged_alloc_slot(static_id);
        }
        self.runtime_region_for_alloc_slot(static_id)
    }
    /// Resolve a **merged** static slot for an allocation. Reuse the physical
    /// region a prior member of the merge tree already minted for this
    /// activation (the parent reusing the child's region), else mint fresh (the
    /// first/child member). The JIT calls this directly
    /// (`elle_jit_resolve_alloc_region_merged`) for a slot it determined at
    /// compile time to be in `LirView::merged_slots`. The interpreter reaches it
    /// through the merged branch of `runtime_region_for_alloc_slot_maybe_merged`.
    /// The single `DecrefRegion` at the merged root's `decref_point` clears the
    /// slot each loop iteration (`take_runtime_region_for_drop_slot`), preserving
    /// per-iteration uniqueness.
    #[inline]
    pub(crate) fn runtime_region_for_merged_alloc_slot(
        &mut self,
        static_id: StaticRegion,
    ) -> RuntimeRegion {
        if let Some(m) = self
            .fiber
            .activation_region_maps
            .last()
            .and_then(|frame| frame.get(&static_id.get()))
        {
            return m.region;
        }
        self.runtime_region_for_alloc_slot(static_id)
    }
    /// Resolve a static slot to the physical region it currently maps to in this
    /// activation, WITHOUT minting or clearing — the read a closure-cycle
    /// merged-arena tail-call deferred release needs (`TailCall::deferred_release_slot`,
    /// docs/impl/region/cycletail.md).
    ///
    /// Unlike [`Self::take_runtime_region_for_drop_slot`] this leaves the mapping
    /// in place. The arena joins the deferred set of the completing activation's
    /// `ActivationDues`. Its own scope-exit `DecrefRegion` is dead code past the
    /// frame-replacing tail call, so no drop ever consumes the mapping. `None`
    /// when the slot is unmapped — the merged alloc did not execute in this
    /// activation (nothing to release).
    #[inline]
    pub(crate) fn runtime_region_for_release_slot(
        &self,
        static_id: StaticRegion,
    ) -> Option<RuntimeRegion> {
        self.fiber
            .activation_region_maps
            .last()
            .and_then(|frame| frame.get(&static_id.get()).map(|m| m.region))
    }
    /// Resolve a static region id for a `DecrefRegion` (the compiler's
    /// initial-reference drop at a value's decref_point). Returns the physical
    /// region and clears the slot; `None` if the allocation never executed
    /// in this activation (conditional alloc — a benign no-op).
    #[inline]
    pub(crate) fn take_runtime_region_for_drop_slot(
        &mut self,
        static_id: StaticRegion,
    ) -> Option<RuntimeRegion> {
        self.fiber
            .activation_region_maps
            .last_mut()
            .and_then(|frame| frame.remove(&static_id.get()))
            .map(|m| m.region)
    }
    /// Hand a frame-replacing tail callee an empty slot map, as the frame hands
    /// it NIL locals. Every entry left here belongs to the caller's code, and
    /// its release is dead past the `TailCall`. A merged slot reuses a mapped
    /// entry, so a stale one would put the callee's allocations in the
    /// caller's region, which a release of the moved argument then frees.
    pub(crate) fn clear_region_map_for_tail_callee(&mut self) {
        if let Some(frame) = self.fiber.activation_region_maps.last_mut() {
            frame.clear();
        }
    }
    /// Claim a spliced call's args array — the array the calling convention built
    /// for this call and no binding of the program names
    /// (docs/impl/region/mechanism.md).
    ///
    /// The claim is split from the free ([`Self::release_splice_args`]) because
    /// the two answer to different moments. Taking the slot must happen BEFORE the
    /// callee runs. A callee that suspends parks the continuation, and the park
    /// SNAPSHOTS this activation's region map. An entry still naming the array
    /// would outlive the free and trip the uncounted-borrow check when the
    /// resume reads it (docs/impl/region/generations.md). Taking it first also
    /// settles the abandoned-frame walk. The slot is gone, so a callee that
    /// raises leaves the walk nothing to run. A frame abandoned BEFORE the call
    /// still has the slot mapped and reclaims the array there.
    ///
    /// Shared by both dispatch tiers so the accounting is identical whichever runs
    /// the caller (`handle_call_array`/`handle_tail_call_array`, and the JIT's
    /// `elle_jit_call_array`/`elle_jit_tail_call_array`).
    #[inline]
    pub(crate) fn take_splice_args(&mut self, slot: StaticRegion) -> Option<RuntimeRegion> {
        self.take_runtime_region_for_drop_slot(slot)
    }

    /// Free the region [`Self::take_splice_args`] claimed, once the callee holds
    /// its own reference to every argument.
    ///
    /// The array counts one reference per element, because `ArrayMutPush` /
    /// `ArrayMutExtend` go through the store funnel. So this release runs their
    /// cascade, and the callee's own references keep each argument alive past it.
    /// The region cannot be recycled between the claim and here: its own minting
    /// reference holds it until this call.
    #[inline]
    pub(crate) fn release_splice_args(&mut self, region: Option<RuntimeRegion>) {
        let Some(region) = region else {
            return;
        };
        if crate::config::get().has_trace("rc") {
            let rc = self.heap().region_rc(region);
            eprintln!("[trace:rc] splice args released({region}) rc={rc}");
        }
        self.heap().decref_region(region);
    }
}

#[cfg(test)]
mod tests;
