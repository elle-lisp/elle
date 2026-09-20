// audited: 2026-09-20
//! Resolving a compiler region slot to the physical region this activation
//! allocates into, drops, or hands a spliced call.
//!
//! docs/impl/region/model.md
//! docs/impl/region/merging.md

use super::*;

mod abandoned;
mod dues;
mod natives;

// Gated where `VM`'s own re-export is, and for the reason given there: the
// compiled entry is the only reader that names the type.
#[cfg(feature = "jit")]
pub(crate) use abandoned::FrameLocals;

impl VM {
    /// Record where a just-minted region came from, for `--trace=arena`
    /// (docs/impl/region/diagnostics.md § "Naming the code that minted a
    /// region"). `arena/dump` prints it beside the region's tags, so a region a
    /// residue window reports as retained names the Elle code that made it.
    ///
    /// The site is the running function and the place it was called from — the
    /// same two facts a stack-trace line carries, read from the innermost
    /// `CallFrame`. `what` names the mint kind and, for a mint the compiler
    /// assigned a slot, that slot: `--dump=regions` on the running function maps
    /// `rN` back to the allocation node, which the call site alone cannot do
    /// when a function allocates in several places.
    ///
    /// Off by default and one relaxed atomic load then: the string is built only
    /// while the bit is set.
    /// `kind` names the mint; `slot` is the compiler's region slot where the mint
    /// has one. Nothing is formatted unless the bit is set, so the allocation path
    /// pays one relaxed atomic load in an ordinary run.
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
    /// Tofte-Talpin (docs/impl/region/merging.md): **every allocation execution gets
    /// its own physical region, period** — merging is the only thing that may
    /// collapse regions onto a shared slot, and a merged slot routes through
    /// `runtime_region_for_alloc_slot_maybe_merged` / `_for_merged_alloc_slot`
    /// instead of here. So this mints a fresh region on every call and stores
    /// `slot → physical` in the current activation frame *for the matching
    /// `DecrefRegion` to find* (`take_runtime_region_for_drop_slot` reads then clears
    /// it); it does **not** return a cached region.
    ///
    /// Returning the cached entry would let a *re-executed* slot reuse the prior
    /// region — commingling distinct values in one region (Rule 6) — and, when
    /// the slot's `DecrefRegion` is dead (a tail-moved alloc whose decref lands
    /// past the `TailCall`, so `take_runtime_region_for_drop_slot` never clears
    /// the slot), every iteration of a tail-recursive body would pile into one
    /// never-cleared region. A `while` loop is unaffected: its reachable
    /// `DecrefRegion` clears the slot each iteration, so the map was already
    /// empty at the next alloc. Overwriting a still-mapped entry is sound — that
    /// entry can only survive a previous alloc when its `DecrefRegion` was dead,
    /// so its region was already going to leak; orphaning the stale mapping
    /// changes nothing.
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
    /// **merging** (docs/impl/region/merging.md § Merging).
    ///
    /// For a slot NOT in `merged_slots` this is exactly
    /// [`Self::runtime_region_for_alloc_slot`] — mint a fresh physical region
    /// every execution and overwrite the activation mapping (the unmerged
    /// one-region-per-value baseline). For a MERGED slot — one shared by ≥2 alloc
    /// instructions after a merge — the FIRST member to execute (the child) finds
    /// the slot unmapped and mints `R`; a LATER member (the parent) finds the slot
    /// already mapped and **reuses** `R`, so every member lands in one region freed
    /// by the single `DecrefRegion`. Per-iteration uniqueness in loops is
    /// preserved because that `DecrefRegion` clears the slot
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
    /// Resolve a **merged** static slot for an allocation: reuse the physical region
    /// a prior member of the merge tree already minted for this activation (the
    /// parent reusing the child's region), else mint fresh (the first/child member).
    /// The JIT calls this directly (`elle_jit_resolve_alloc_region_merged`) for a
    /// slot it determined at compile time to be in `LirFunction.merged_slots`; the
    /// interpreter reaches it through the merged branch of
    /// `runtime_region_for_alloc_slot_maybe_merged`. The single `DecrefRegion` at the
    /// merged root's `decref_point` clears the slot each loop iteration
    /// (`take_runtime_region_for_drop_slot`), preserving per-iteration uniqueness.
    /// (docs/impl/region/merging.md § Merging, mint-or-reuse.)
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
    /// docs/impl/region/letrec.md § The letrec closure-cycle merge).
    ///
    /// Unlike [`Self::take_runtime_region_for_drop_slot`] this leaves the mapping
    /// in place: the arena is handed to the completing activation's
    /// `deferred_releases`, and its own scope-exit `DecrefRegion` is dead code past
    /// the frame-replacing tail call, so the mapping is never consumed by a drop.
    /// `None` when the slot is unmapped — the merged alloc did not execute in this
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
    /// Claim a spliced call's args array — the array the calling convention built
    /// for this call and no binding of the program names
    /// (docs/impl/region/mechanism.md § "A spliced call's arguments come out of
    /// an array the convention owns").
    ///
    /// The claim is split from the free ([`Self::release_splice_args`]) because
    /// the two answer to different moments. Taking the slot must happen BEFORE the
    /// callee runs: a callee that suspends parks the continuation, and the park
    /// SNAPSHOTS this activation's region map, so an entry still naming the array
    /// would outlive the free and detonate the uncounted-borrow check when the
    /// resume reads it (docs/impl/region/generations.md). Taking it first settles
    /// the abandoned-frame walk in the same stroke — the slot is gone, so a callee
    /// that raises leaves the walk nothing to run, and a frame abandoned BEFORE
    /// the call still has it mapped and reclaims the array there.
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
    /// The array counts one reference per element — `ArrayMutPush` /
    /// `ArrayMutExtend` go through the store funnel — so this release runs their
    /// cascade, and the callee's own references are what it must not be the last
    /// of. The region cannot be recycled between the claim and here: it is held by
    /// its own minting reference until this call.
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
