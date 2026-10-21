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
    /// Drives the `arena/dump` diagnostic — a region's object *kinds* are what
    /// localise a leak (a stray `Fiber` / `Closure` region naming the unfreed
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
    /// Check if a pointer falls within any of this region's pages.
    pub fn owns(&self, ptr: *const ()) -> bool {
        let addr = ptr as *const u8;
        self.pages.iter().any(|p| p.contains(addr))
    }
    /// Walk objects and find region IDs of cross-region references (the
    /// free-time cascade decrefs each).
    ///
    /// Must be called BEFORE teardown (dtors are still alive).
    /// `own_id` is this region's ID — self-references are excluded.
    /// `page_size` is needed for `region_of_page_ptr`.
    /// `valid_region` receives the resolved id AND the pointer, and must verify
    /// the id names a live region of the calling store that genuinely OWNS the
    /// pointer's address (see `find_object_cross_refs`).
    pub fn find_region_cross_refs(
        &self,
        own_id: u32,
        page_size: usize,
        valid_region: &dyn Fn(u32, *const ()) -> bool,
    ) -> Vec<u32> {
        // Breadcrumb the member under scan so a guardfree fault inside
        // `region_of_page_ptr` (an over-freed target page) names the region that
        // still HELD the dangling edge, not only the region that freed the target.
        crate::value::fiberheap::freelog::set_scan_member(own_id);
        let mut refs = Vec::new();
        for &ptr in self.dtors.iter().chain(self.ref_objs.iter()) {
            if ptr.is_null() {
                continue;
            }
            let obj = unsafe { &*ptr };
            Self::find_object_cross_refs(obj, own_id, page_size, valid_region, &mut refs);
        }
        crate::value::fiberheap::freelog::set_scan_member(0);
        refs
    }

    /// Extract cross-region Value references from a HeapObject.
    ///
    /// `valid_region(rid, ptr)` is the OWNERSHIP predicate, not a bare liveness
    /// check: it must verify that the calling store's live region `rid` genuinely
    /// owns `ptr`'s address (`RegionPool::owns`). `region_of_page_ptr` reads a
    /// stamped page-header id, and a pointer NOT managed by this store — a
    /// foreign-heap value (a compile-time-env constant baked into a template's
    /// pool lives on the CompileCtx VM's heap; a worker reading a parent-heap
    /// value), or a shared/Rc allocation — reads whatever bytes sit at the masked
    /// base, which can collide with a live local id. Liveness alone is
    /// time-dependent (the colliding id may be dead at alloc-record time and live
    /// at free-scan time), which would let the recorded edge table and the
    /// free-time scan disagree; ownership is time-invariant for a foreign pointer
    /// (no local region ever owns its address), so record and scan stay
    /// symmetric by construction.
    pub(crate) fn find_object_cross_refs(
        obj: &HeapObject,
        own_id: u32,
        page_size: usize,
        valid_region: &dyn Fn(u32, *const ()) -> bool,
        refs: &mut Vec<u32>,
    ) {
        let mut check = |val: &Value| {
            if !val.is_heap() {
                return;
            }
            if let Some(ptr) = val.as_heap_ptr() {
                let rid = unsafe { region_of_page_ptr(ptr, page_size) };
                // Skip ids 0 and 1 (reserved, not real regions) and the region's
                // own id; RC-track only cross-refs into other live regions OF THIS
                // STORE that own the pointer (the alloc-time incref balanced by
                // the free-time cascade decref). The funnel side of this
                // accounting lives in value/arena.rs.
                if rid != 0 && rid != 1 && rid != own_id && valid_region(rid, ptr) {
                    refs.push(rid);
                }
            }
        };

        // The `traits` side-field (set by `with-traits`) is a cross-region edge
        // on EVERY traitable variant — the trait table lives in its own region,
        // not inline. It is enumerated here, once, for all variants so the
        // alloc-scan increfs and the free-cascade decrefs it symmetrically
        // (Rule 5/7); the per-variant `match` below covers only the inline
        // content fields. The counter-factual: omit it, and the table is freed at
        // its constructor's decref_point while the host still references it.
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
                // OWN region (built together by `MakeClosure`), so the synthetic
                // backing ref below is filtered by `rid == own_id`. But a
                // closure built by SHARING another closure's env — `squelch` /
                // `attune` (src/primitives/meta/syntaxops.rs) clone the template and copy
                // the env's `(ptr, len)` pair, leaving the backing data in the
                // SOURCE closure's region — has its env backing in a DIFFERENT
                // region. Without a cross-region edge to that backing, the
                // source region is freed at its owning-scope decref while this
                // closure still reads the env: `populate_env` reads a freed page
                // on the first fiber resume (tests/lang/signals.lisp).
                // Synthesize a heap Value at the backing pointer so the shared
                // `check` routes it like any cross-region ref — symmetric with
                // the Fiber arm below, and balanced because a closure's env is
                // immutable for its lifetime (alloc-scan increfs, free-cascade
                // decrefs the same edge).
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
                // header is co-region with its instance (self-edge, filtered by
                // `rid == own_id`); a header shared by `squelch`/`attune` lives
                // in the SOURCE closure's region (a real cross-region edge
                // keeping that region alive). Both route through `check`.
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
                // A heap-valued key (`TableKey::Heap`/nested in an `Array` key) is
                // a cross-region reference just like the value — enumerate both so
                // the key's region is increfed/recorded at alloc and cascade-decrefed
                // at free (else the key's region frees while the struct still points
                // into it; see tests/impl/region-struct-heap-key-uaf.lisp).
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
                // The header→payload edge. A code object's payload lives in its
                // compile unit's code region (docs/impl/region/template.md), so
                // a header in any other region holds a real cross-region
                // reference: without it the code region frees while a header
                // still reads its bytecode. Every slice of one payload lands in
                // that one region by construction, so the payload's own backing
                // covers them all, the child table's headers included.
                // Synthesize a heap Value at it, as the closure-env arm does.
                let payload =
                    Value::from_heap_ptr(t.payload_backing(), crate::value::repr::TAG_ARRAY);
                check(&payload);
                // The payload's constant pool. A string or quoted literal is a
                // `MaterializeConst` allocation rather than a pool value, but a
                // `ValueConst` can load a heap value — a closure — from a region
                // of its own, so every constant is scanned like any other edge
                // (alloc-scan increfs, free-cascade decrefs the same edge).
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
