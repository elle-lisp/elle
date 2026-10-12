// audited: 2026-10-20
//! What a region pool can say about itself: its live objects and pages, and the cross-region references it holds.
//!
//! docs/impl/region/model.md
//! docs/impl/region/rules.md

use super::*;

mod fiber;

impl RegionPool {
    #[allow(dead_code)]
    pub fn obj_count(&self) -> usize {
        self.obj_count
    }
    /// The `HeapTag` of every live object in this region, in allocation order.
    /// Drives the `arena/dump` diagnostic. A region's object *kinds* are what
    /// localise a leak (a stray `Fiber` or `Closure` region names the unfreed
    /// value), which the bare counts of `region_info_vec` cannot.
    pub fn debug_tags(&self) -> Vec<crate::value::heap::HeapTag> {
        let mut out = Vec::new();
        for &ptr in self.dtors.iter().chain(self.ref_objs.iter()) {
            if !ptr.is_null() {
                out.push(unsafe { (*ptr).tag() });
            }
        }
        out
    }
    /// Every live object in this region, in allocation order. The free path's
    /// fiber-discharge walk reads this to find parked `Fiber` objects whose
    /// chain state must be released with the region (`teardown_set`).
    pub(crate) fn live_objects(&self) -> impl Iterator<Item = &HeapObject> {
        self.dtors
            .iter()
            .chain(self.ref_objs.iter())
            .filter(|p| !p.is_null())
            .map(|&p| unsafe { &*p })
    }
    /// Total committed bytes across all pages.
    pub fn allocated_bytes(&self) -> usize {
        self.pages.iter().map(|p| p.page.len()).sum()
    }
    /// (base, end) address range of every page this region owns.
    /// Used by the free-log diagnostic to attribute UAF addresses.
    pub fn page_ranges(&self) -> Vec<(usize, usize)> {
        self.pages
            .iter()
            .map(|p| {
                let base = p.page.as_ptr() as usize;
                (base, base + p.page.len())
            })
            .collect()
    }
    /// Check if a pointer falls within any of this region's pages. A hydrated
    /// image's pages answer with one range check, and only the pages claimed
    /// after hydration are scanned.
    pub fn owns(&self, ptr: *const ()) -> bool {
        let claimed = self.image.as_ref().map_or(0, |s| s.pages);
        self.image_contains(ptr as usize)
            || self.pages[claimed..]
                .iter()
                .any(|p| p.contains(ptr as *const u8))
    }
    /// Walk objects and find region IDs of cross-region references (the
    /// free-time cascade decrefs each).
    ///
    /// Must be called BEFORE teardown (dtors are still alive).
    /// `own_id` is this region's ID — self-references are excluded.
    /// `owner` resolves a pointer to the live region of the calling store that
    /// OWNS it, or 0 (see `find_object_cross_refs`).
    pub fn find_region_cross_refs(
        &self,
        own_id: u32,
        owner: &dyn Fn(*const ()) -> u32,
    ) -> Vec<u32> {
        // Breadcrumb the member under scan. A guardfree fault inside `owner`'s
        // header read (an over-freed target page) then names the region that
        // still HELD the dangling edge, not only the region that freed the
        // target.
        crate::value::fiberheap::freelog::set_scan_member(own_id);
        let mut refs = Vec::new();
        for &ptr in self.dtors.iter().chain(self.ref_objs.iter()) {
            if ptr.is_null() {
                continue;
            }
            let obj = unsafe { &*ptr };
            Self::find_object_cross_refs(obj, own_id, owner, &mut refs);
        }
        crate::value::fiberheap::freelog::set_scan_member(0);
        refs
    }

    /// Extract cross-region Value references from a HeapObject.
    ///
    /// `owner(ptr)` answers the live region of the calling store that OWNS
    /// `ptr`'s address, or 0 when none does (`RegionStore::owning_region`).
    /// Ownership, not a bare liveness check, is what decides. A page header
    /// holds a stamped id, and a pointer NOT managed by this store reads
    /// whatever id sits at its masked base, which can collide with a live local
    /// id. Such a pointer is a foreign-heap value (a compile-time-env constant
    /// baked into a template's pool lives on the CompileCtx VM's heap; a worker
    /// reading a parent-heap value), or a shared or Rc allocation.
    ///
    /// Liveness alone is time-dependent: the colliding id may be dead when the
    /// allocation records its edges and live when the free scans them, and the
    /// recorded table and the free-time scan would disagree. Ownership does not
    /// change over time for a foreign pointer, because no local region ever
    /// owns its address. So record and scan stay symmetric by construction.
    pub(crate) fn find_object_cross_refs(
        obj: &HeapObject,
        own_id: u32,
        owner: &dyn Fn(*const ()) -> u32,
        refs: &mut Vec<u32>,
    ) {
        let mut check = |val: &Value| {
            if !val.is_heap() {
                return;
            }
            if let Some(ptr) = val.as_heap_ptr() {
                let rid = owner(ptr);
                // Skip ids 0 and 1 (reserved, not real regions) and the
                // region's own id. RC-track only cross-refs into other live
                // regions OF THIS STORE that own the pointer: the alloc-time
                // incref, balanced by the free-time cascade decref. The funnel
                // side of this accounting lives in value/arena.rs.
                if rid > 1 && rid != own_id {
                    refs.push(rid);
                }
            }
        };

        // The `traits` side-field (set by `with-traits`) is a cross-region
        // edge on EVERY traitable variant: the trait table lives in its own
        // region, not inline. It is enumerated here, once, for all variants, so
        // the alloc scan increfs it and the free cascade decrefs it
        // symmetrically (Rule 5/7). The per-variant `match` below covers only
        // the inline content fields. The counter-factual: omit it, and the
        // table is freed at its constructor's decref_point while the host
        // still references it.
        check(&obj.traits());

        match obj {
            HeapObject::LArrayMut { data, .. } => {
                if let Ok(borrowed) = data.try_borrow() {
                    for v in borrowed.iter() {
                        check(v);
                    }
                }
            }
            HeapObject::LStructMut { data, .. } => {
                if let Ok(borrowed) = data.try_borrow() {
                    for (k, v) in borrowed.iter() {
                        k.for_each_heap_value(&mut check);
                        check(v);
                    }
                }
            }
            HeapObject::LBox { cell, .. } | HeapObject::CaptureCell { cell, .. } => {
                if let Ok(borrowed) = cell.try_borrow() {
                    check(&borrowed);
                }
            }
            HeapObject::LSetMut { data, .. } => {
                if let Ok(borrowed) = data.try_borrow() {
                    for v in borrowed.iter() {
                        check(v);
                    }
                }
            }
            HeapObject::Closure { closure, .. } => {
                // The env RegionSlice backing usually lives in the closure's
                // OWN region (`MakeClosure` builds the two together), so
                // `rid == own_id` filters the synthetic backing ref below.
                // `squelch` and `attune` (src/primitives/meta/syntaxops.rs)
                // build a closure by SHARING another closure's env instead:
                // they clone the template and copy the env's `(ptr, len)`
                // pair. The backing data stays in the SOURCE closure's region,
                // a DIFFERENT region from this closure's.
                //
                // Without a cross-region edge to that backing, the source
                // region is freed at its owning-scope decref while this closure
                // still reads the env (tests/lang/signals.lisp). So synthesize
                // a heap Value at the backing pointer, and the shared `check`
                // routes it like any cross-region ref, as the Fiber arm below
                // does. The edge stays balanced because a closure's env is
                // immutable for its lifetime: the alloc scan increfs the same
                // edge the free cascade decrefs.
                if !closure.env.is_empty() {
                    let backing = Value::from_heap_ptr(
                        closure.env.as_ptr() as *const (),
                        crate::value::repr::TAG_ARRAY,
                    );
                    check(&backing);
                }
                for v in closure.env.iter() {
                    check(v);
                }
                // The instance→template edge. A `MakeClosure`-materialized
                // header shares its instance's region (a self-edge, filtered
                // by `rid == own_id`). A header `squelch` or `attune` shares
                // lives in the SOURCE closure's region: a real cross-region
                // edge that keeps that region alive. Both route through
                // `check`.
                check(&closure.template.value());
            }
            HeapObject::Pair(pair) => {
                check(&pair.first);
                check(&pair.rest);
            }
            HeapObject::LArray { elements, .. } => {
                for v in elements.iter() {
                    check(v);
                }
            }
            HeapObject::LStruct { data, .. } => {
                // A heap-valued key (`TableKey::Heap`, or one nested in an
                // `Array` key) is a cross-region reference just like the
                // value. Enumerate both, so the alloc scan increfs and records
                // the key's region and the free cascade decrefs it
                // (tests/impl/region-struct-heap-key-uaf.lisp).
                for (k, v) in data.iter() {
                    k.for_each_heap_value(&mut check);
                    check(v);
                }
            }
            HeapObject::LSet { data, .. } => {
                for v in data.iter() {
                    check(v);
                }
            }
            HeapObject::Parameter { default, .. } => {
                check(default);
            }
            // A string or bytes payload usually lies in its own region's pages,
            // where the backing resolves to `own_id` and is filtered as a
            // self-edge. A view's payload lies in its SOURCE's pages: `slice`
            // and `string` answer a header in the call's region over the
            // argument's bytes (docs/impl/values.md), and the backing is the
            // edge that keeps the source alive, exactly as the Closure arm's
            // env backing is. An empty payload is skipped: its pointer is
            // dangling, not a page.
            HeapObject::LString { s: backing, .. } | HeapObject::LBytes { data: backing, .. } => {
                if !backing.is_empty() {
                    let value = Value::from_heap_ptr(
                        backing.as_ptr() as *const (),
                        crate::value::repr::TAG_BYTES,
                    );
                    check(&value);
                }
            }
            // Non-container types: no Value references to track.
            HeapObject::LStringMut { .. }
            | HeapObject::LBytesMut { .. }
            | HeapObject::Float(_)
            | HeapObject::LibHandle(_) => {}
            HeapObject::Fiber { handle, .. } => fiber::fiber_edges(handle, &mut check),
            HeapObject::ClosureTemplate(t) => {
                // The header→payload edge. A code object's payload lives in
                // its compile unit's code region
                // (docs/impl/region/template.md), so a header in any other
                // region holds a real cross-region reference. Without it the
                // code region frees while a header still reads its bytecode.
                // Every slice of one payload lands in that one region by
                // construction, so the payload's own backing covers them all,
                // the child table's headers included. Synthesize a heap Value
                // at it, as the closure-env arm does.
                let payload =
                    Value::from_heap_ptr(t.payload_backing(), crate::value::repr::TAG_ARRAY);
                check(&payload);
                // The payload's constant pool. A string or quoted literal is a
                // `MaterializeConst` allocation rather than a pool value. But a
                // `ValueConst` can load a heap value, a closure, from a region
                // of its own. So the scan treats every constant like any other
                // edge: the alloc scan increfs the edge the free cascade
                // decrefs.
                for v in t.constants().iter() {
                    check(v);
                }
            }
            // An `External`'s payload is opaque to the scan
            // (docs/impl/region/rules.md Rule 5).
            HeapObject::ThreadHandle { .. }
            | HeapObject::Syntax { .. }
            | HeapObject::FFISignature(_, _)
            | HeapObject::FFIType(_)
            | HeapObject::ManagedPointer { .. }
            | HeapObject::External { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests;
