use super::*;

impl RegionStore {
    /// Increment the reference count for a region.
    pub fn incref(&mut self, id: RuntimeRegion) {
        self.ensure(id);
        let entry = self.regions[id.get() as usize].as_mut().unwrap();
        match &mut entry.reclaim {
            Reclaim::Counted(rc) => {
                *rc = rc.saturating_add(1);
                if crate::config::get().has_trace("rc") {
                    eprintln!("[trace:rc] incref({id}) → rc={}", *rc);
                }
            }
            // Owned ⇒ no count to bump: the region is reclaimed by its owner's
            // subtree drop regardless of how many references point at it. The
            // auto-incref of a cross-region ref *into* an owned member (a sibling
            // storing it) lands here and is correctly inert — the symmetric
            // free-time cascade decref no-ops against the Owned mode too.
            Reclaim::Owned { .. } => {
                if crate::config::get().has_trace("rc") {
                    eprintln!("[trace:rc] incref({id}) → owned (no count)");
                }
            }
        }
    }
    /// Decrement the reference count for a region.
    /// If RC is already 0, free the region. Otherwise decrement,
    /// and free if RC reaches 0. Returns the number of objects freed.
    pub fn decref(&mut self, id: RuntimeRegion) -> usize {
        self.decref_with_cascade(id, None)
    }
    /// Decrement with optional cascade source (for tracing).
    /// Returns the number of objects freed (0 if RC > 0 after decrement).
    ///
    /// The free itself runs the iterative worklist driver
    /// ([`Self::free_region_set`]), so a decref that reaches rc 0 never recurses
    /// per cascaded link — a chain of N cross-region references frees in O(1)
    /// native stack regardless of N.
    pub(super) fn decref_with_cascade(
        &mut self,
        id: RuntimeRegion,
        from_cascade: Option<RuntimeRegion>,
    ) -> usize {
        if self.decref_reaches_zero(id, from_cascade) {
            self.free_runtime_region_pages(id, from_cascade)
        } else {
            0
        }
    }

    /// Decrement `id`'s reference count, returning `true` iff it just reached 0
    /// and its pages must now be freed. This is the pure bookkeeping half of
    /// [`Self::decref_with_cascade`] — it performs **no** free, so the free
    /// cascade can drive reclamation from an explicit worklist
    /// ([`Self::free_region_set`]) instead of recursing through
    /// `decref → free → decref` once per cross-region link.
    pub(super) fn decref_reaches_zero(
        &mut self,
        id: RuntimeRegion,
        from_cascade: Option<RuntimeRegion>,
    ) -> bool {
        let idx = id.get() as usize;
        // Direct decrefs (not from cascade) must hit a region that
        // was actually allocated. A miss here means either (a) the
        // solver assigned a region id that no instruction
        // alloc_in_region'd (the phantom-region class of bug —
        // see docs/impl/region/rules.md § "Every region must correspond to a
        // real allocation"), or (b) a double-free. Cascade decrefs
        // are exempt because a single region may be referenced
        // multiple times by another's contents and may have already
        // been freed by an earlier cascade visit.
        debug_assert!(
            from_cascade.is_some() || (idx < self.regions.len() && self.regions[idx].is_some()),
            "DecrefRegion({id}) but region was never alloc_in_region'd \
             (or already freed) — phantom region or double-free; \
             see docs/impl/region/rules.md § 'Every region must correspond to a \
             real allocation'",
        );
        if idx >= self.regions.len() {
            return false;
        }
        match self.regions[idx].as_mut() {
            None => false,
            // owned ⇒ RC frozen (docs/impl/region/ownership.md § "The runtime: a
            // reclamation typestate"): an `Owned` region has no count to
            // decrement — it is reclaimed only by its owner's subtree drop. Both a
            // direct decref and a cascade decref (the owner's free-time content
            // scan hitting an interior child) land here and no-op; the subtree
            // drop frees the child explicitly. The no-op is *structural* (the
            // variant carries no `u32`), which is what lets the interior
            // containment edge stay un-suppressed at runtime.
            Some(e) if matches!(e.reclaim, Reclaim::Owned { .. }) => false,
            Some(entry) => {
                let Reclaim::Counted(rc) = &mut entry.reclaim else {
                    unreachable!("Owned handled by the arm above")
                };
                // rc==0 means a cascade decref reached an already-zeroed counted
                // region; free without underflowing. Copy `freed`/`new_rc` out so
                // the `rc` borrow ends before `entry.pool` is read for tracing.
                let (freed, new_rc) = if *rc == 0 {
                    (true, 0)
                } else {
                    *rc -= 1;
                    (*rc == 0, *rc)
                };
                if crate::config::get().has_trace("rc") {
                    let src =
                        from_cascade.map_or("direct".to_string(), |s| format!("cascade({s})"));
                    if freed {
                        eprintln!(
                            "[trace:rc] decref({id}) → rc=0 → FREE objs={} src={src}",
                            entry.pool.obj_count()
                        );
                    } else {
                        eprintln!("[trace:rc] decref({id}) → rc={new_rc} src={src}");
                    }
                }
                freed
            }
        }
    }
    /// Get the current RC for a region (0 if not created).
    pub fn rc(&self, id: RuntimeRegion) -> u32 {
        self.rc_raw(id.get())
    }
    /// Get the current RC for a raw physical id (0 if not created). The `rc`
    /// entry point that already holds a `u32`.
    pub fn rc_raw(&self, id: u32) -> u32 {
        let idx = id as usize;
        if idx < self.regions.len() {
            // `count()` is 0 for an `Owned` region — it has no independent reference
            // count (reclaimed by its owner's subtree drop).
            self.regions[idx].as_ref().map_or(0, |e| e.count())
        } else {
            0
        }
    }
    /// Free a region — tolerant of "never alloc'd" callers.
    ///
    /// This is the entry point for runtime/macro callers (the per-expansion
    /// transient in `expand_macro_call`, the per-compilation transient in
    /// `pipeline::compile::with_transient`, the embedding API) that reserve a
    /// region id without necessarily
    /// allocating into it: the block may end without producing any
    /// heap value, in which case there is no slot in the store. That
    /// is a legitimate pattern, not a bug, so this path silently
    /// skips when the slot is absent.
    ///
    /// For the bytecode `DecrefRegion` path — the one the regions
    /// audit covers — use `decref`, which asserts the slot exists
    /// (debug builds only). The split keeps the strict invariant on
    /// the path it applies to without breaking the tolerant runtime
    /// pattern.
    pub fn decref_if_present(&mut self, id: RuntimeRegion) -> usize {
        let idx = id.get() as usize;
        if idx >= self.regions.len() || self.regions[idx].is_none() {
            return 0;
        }
        self.decref(id)
    }
    /// Record one outgoing content edge `src → dst` into `src`'s edge table
    /// (docs/impl/region/ownership.md § "The outgoing edge table"), mirrored into
    /// `dst`'s `incoming` table (§ "The incoming edge table and the external-
    /// reference rescue") in the same call so the two ledgers cannot drift
    /// independently. Applies the same
    /// filter `find_object_cross_refs` applies — a reserved target (0/1) or a
    /// self-edge (`dst == src`) records nothing — so the recorded table stays
    /// scan-equivalent, which the free-time oracle asserts. `src` must already
    /// exist (the alloc funnel `ensure_raw`s it; the mutable-store seam and the
    /// fiber-signal funnel hold a live container/fiber); a missing source is a
    /// defensive no-op. `dst` names the region a live `Value` resides in, so it
    /// always has an entry when an edge is recorded.
    pub(crate) fn record_outgoing(&mut self, src: u32, dst: u32) {
        if crate::config::get().has_trace("rc") {
            eprintln!("[trace:rc] record_outgoing({src} -> {dst})");
        }
        if dst == 0 || dst == 1 || dst == src {
            return;
        }
        let (Some(src_r), Some(dst_r)) = (RuntimeRegion::new(src), RuntimeRegion::new(dst)) else {
            return;
        };
        let recorded = match self.regions.get_mut(src as usize).and_then(|s| s.as_mut()) {
            Some(entry) => {
                *entry.outgoing.entry(dst_r).or_insert(0) += 1;
                true
            }
            None => false,
        };
        if recorded {
            match self.regions.get_mut(dst as usize).and_then(|s| s.as_mut()) {
                Some(entry) => {
                    *entry.incoming.entry(src_r).or_insert(0) += 1;
                }
                None => debug_assert!(
                    false,
                    "record_outgoing({src} → {dst}): the target has no entry, so the \
                     incoming mirror cannot be maintained — an edge to a region no live \
                     value can reside in (docs/impl/region/ownership.md § 'The incoming \
                     edge table and the external-reference rescue')"
                ),
            }
        }
    }

    /// Remove one outgoing content edge `src → dst` — the overwrite / removal half
    /// of the mutable-store seam (a pop/remove/del, or the old target of a
    /// replace) — and its `incoming` mirror. Same filter as
    /// [`Self::record_outgoing`]. A debug-assert fires on
    /// an absent edge: an unbalanced un-record is recording drift, caught here
    /// rather than only at the next free's equivalence oracle.
    pub(crate) fn unrecord_outgoing(&mut self, src: u32, dst: u32) {
        if crate::config::get().has_trace("rc") {
            eprintln!("[trace:rc] unrecord_outgoing({src} -> {dst})");
        }
        if dst == 0 || dst == 1 || dst == src {
            return;
        }
        let (Some(src_r), Some(dst_r)) = (RuntimeRegion::new(src), RuntimeRegion::new(dst)) else {
            return;
        };
        let removed = match self.regions.get_mut(src as usize).and_then(|s| s.as_mut()) {
            Some(entry) => match entry.outgoing.get_mut(&dst_r) {
                Some(count) if *count > 1 => {
                    *count -= 1;
                    true
                }
                Some(_) => {
                    entry.outgoing.remove(&dst_r);
                    true
                }
                None => {
                    debug_assert!(
                        false,
                        "unrecord_outgoing({src} → {dst}): no recorded edge to remove — \
                         outgoing-edge accounting drift (docs/impl/region/ownership.md \
                         § 'The outgoing edge table')"
                    );
                    false
                }
            },
            None => false,
        };
        if removed {
            self.unmirror_incoming(src_r, dst_r, 1);
        }
    }

    /// Remove `count` from `dst`'s incoming mirror of the edge `src → dst` — the
    /// shared bookkeeping of [`Self::unrecord_outgoing`] and the subtree drop's
    /// frontier walk (which retires a dying source's whole footprint at once).
    /// An absent `dst` entry is tolerated: the edge outlived its target (the
    /// target died first, taking its mirror with it), so there is nothing left to
    /// maintain. An absent mirror ENTRY on a live target is drift, debug-asserted
    /// exactly as the outgoing side is.
    pub(super) fn unmirror_incoming(&mut self, src: RuntimeRegion, dst: RuntimeRegion, count: u32) {
        if let Some(entry) = self
            .regions
            .get_mut(dst.get() as usize)
            .and_then(|s| s.as_mut())
        {
            match entry.incoming.get_mut(&src) {
                Some(c) if *c > count => *c -= count,
                Some(c) => {
                    debug_assert!(
                        *c == count,
                        "unmirror_incoming({src} → {dst}): removing {count} from a mirror \
                         of {c} — incoming-edge accounting drift \
                         (docs/impl/region/ownership.md § 'The incoming edge table and \
                         the external-reference rescue')"
                    );
                    entry.incoming.remove(&src);
                }
                None => debug_assert!(
                    false,
                    "unmirror_incoming({src} → {dst}): no mirrored edge to remove — \
                     incoming-edge accounting drift (docs/impl/region/ownership.md \
                     § 'The incoming edge table and the external-reference rescue')"
                ),
            }
        }
    }

    /// Scan a HeapObject for cross-region Value refs and incref each.
    /// Called at allocation time to balance cascade decrefs at free time.
    /// The callback is the OWNERSHIP predicate (`find_object_cross_refs`): the
    /// resolved id must name a live region of THIS store that owns the pointer,
    /// so a foreign-heap value (a compile-time-env constant, a parent-heap
    /// borrow) whose masked page-header bytes collide with a live local id is
    /// never increfed or recorded — keeping the table scan-symmetric.
    pub(super) fn incref_cross_region_refs(&mut self, obj: &HeapObject, own_id: u32) {
        let page_size = self.pool.initial_page_size();
        let valid_region = |rid: u32, ptr: *const ()| -> bool {
            self.regions
                .get(rid as usize)
                .and_then(|s| s.as_ref())
                .is_some_and(|e| e.pool.owns(ptr))
        };
        let mut refs = Vec::new();
        RegionPool::find_object_cross_refs(obj, own_id, page_size, &valid_region, &mut refs);
        for rid in refs {
            // `find_object_cross_refs` filters `rid != 0`, so the wrap holds.
            if let Some(r) = RuntimeRegion::new(rid) {
                self.incref(r);
            }
            // Record the outgoing edge in the SAME loop that increfs it, so the
            // alloc-path table is scan-equivalent by construction — `refs` is the
            // very output of `find_object_cross_refs`, the function the free-time
            // oracle scans with (docs/impl/region/ownership.md § "The outgoing edge
            // table"). The reserved/self filter lives in `record_outgoing`.
            self.record_outgoing(own_id, rid);
        }
    }
}
