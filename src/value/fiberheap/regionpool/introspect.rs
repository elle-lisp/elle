// audited: 2026-10-12
//! What a region pool can say about itself: its live objects and pages, and the cross-region references it holds.
//!
//! docs/impl/region/model.md
//! docs/impl/region/rules.md

use super::*;

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
        // Breadcrumb the member under scan. A guardfree fault inside
        // `region_of_page_ptr` (an over-freed target page) then names the
        // region that still HELD the dangling edge, not only the region that
        // freed the target.
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
    /// check: it must verify that the calling store's live region `rid`
    /// genuinely owns `ptr`'s address (`RegionPool::owns`).
    /// `region_of_page_ptr` reads a stamped page-header id. A pointer NOT
    /// managed by this store reads whatever bytes sit at the masked base, which
    /// can collide with a live local id. Such a pointer is a foreign-heap value
    /// (a compile-time-env constant baked into a template's pool lives on the
    /// CompileCtx VM's heap; a worker reading a parent-heap value), or a shared
    /// or Rc allocation.
    ///
    /// Liveness alone is time-dependent: the colliding id may be dead when the
    /// allocation records its edges and live when the free scans them, and the
    /// recorded table and the free-time scan would disagree. Ownership does not
    /// change over time for a foreign pointer, because no local region ever
    /// owns its address. So record and scan stay symmetric by construction.
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
                // Skip ids 0 and 1 (reserved, not real regions) and the
                // region's own id. RC-track only cross-refs into other live
                // regions OF THIS STORE that own the pointer: the alloc-time
                // incref, balanced by the free-time cascade decref. The funnel
                // side of this accounting lives in value/arena.rs.
                if rid != 0 && rid != 1 && rid != own_id && valid_region(rid, ptr) {
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
            // Non-container types: no Value references to track.
            HeapObject::LString { .. }
            | HeapObject::LStringMut { .. }
            | HeapObject::LBytes { .. }
            | HeapObject::LBytesMut { .. }
            | HeapObject::Float(_)
            | HeapObject::LibHandle(_) => {}
            HeapObject::Fiber { handle, .. } => {
                // The top-level `check(&obj.traits())` above tracks the
                // fiber's `traits` edge, with every other traitable variant.
                // This arm covers only the fiber's closure, env and signal.
                //
                // Keep the fiber's (immutable) closure alive. The closure's
                // env RegionSlice backing and its captured Values live in the
                // region where the closure was built, usually a *different*
                // activation than the fiber. Without a cross-region edge here,
                // the spawning activation frees that region while the parked
                // fiber still references it.
                let mut env_vals: Vec<Value> = Vec::new();
                let mut backing = Value::NIL;
                // A parked or dead fiber holds its result or yield value in
                // `signal`, which `fiber/value` reads later. That value lives
                // in whatever region the fiber allocated it in, a region the
                // parent will `DecrefValueRegion` when the producing call's
                // decref_point fires. Track it as a cross-region edge, so the
                // fiber keeps it alive until the fiber itself dies (the
                // cascade decrefs it here at free). The matching incref is
                // added when the fiber parks or dies (`incref_signal_region`
                // in src/vm/fiber/refcount.rs).
                let mut signal_val = Value::NIL;
                // The fiber's closure holds a template (a region-allocated
                // `HeapObject::ClosureTemplate`), usually in the same region
                // as the env backing. An EMPTY-env closure has no backing to
                // synthesize, so track the template edge explicitly. Otherwise
                // the fiber's own template region is freed while the parked
                // fiber still needs `closure.template.code()` and its arity on
                // first resume.
                let mut template_val = Value::NIL;
                // The fiber's `closure_value`: the wrapper VALUE it installs
                // as the body's executing-closure register
                // (`Fiber::closure_value`, `pending_entry_closure` on first
                // resume). For a `MakeClosure` closure its region is the
                // env/template region tracked above. That duplicate edge is
                // balanced against a duplicate free-cascade decref by the
                // counted `outgoing` multiset.
                //
                // A `squelch` or `attune` wrapper
                // (src/primitives/meta/syntaxops.rs) shares the SOURCE
                // closure's template and env, yet is itself a fresh closure
                // value in a DIFFERENT region. So only `closure_value` reaches
                // its region. Without this edge that region frees at its
                // binding's decref_point while the fiber still holds the value
                // (tests/impl/region-squelch-fiber-uaf.lisp).
                let mut closure_value = Value::NIL;
                // The seeded parameter BASELINE (frames[0], exactly when
                // `param_baseline_seeded`) was retained at the seed, with a
                // recorded fiber → value edge per heap entry
                // (`retain_param_baseline`). Walking it here is the symmetric
                // release at the fiber object's free. The fiber's own later
                // `parameterize` frames are NOT walked: their values belong to
                // the parked activation (docs/impl/region/owner.md).
                let mut param_vals: Vec<Value> = Vec::new();
                let _ = handle.try_with(|fib| {
                    closure_value = fib.closure_value;
                    if fib.param_baseline_seeded {
                        if let Some(baseline) = fib.param_frames.first() {
                            for &(_, v) in baseline {
                                param_vals.push(v);
                            }
                        }
                    }
                    // An empty env uses a dangling sentinel pointer, in no
                    // region page, so skip it. For a non-empty env, synthesize
                    // a heap Value at the backing, so the shared `check`
                    // routes the closure's own region (where the RegionSlice
                    // lives) like any cross-region ref.
                    if !fib.closure.env.is_empty() {
                        let p = fib.closure.env.as_ptr() as *const ();
                        backing = Value::from_heap_ptr(p, crate::value::repr::TAG_ARRAY);
                    }
                    template_val = fib.closure.template.value();
                    for v in fib.closure.env.iter() {
                        env_vals.push(*v);
                    }
                    if let Some((bits, v)) = fib.signal {
                        // Only terminal results are park-retained
                        // (`is_terminal_signal` in src/vm/fiber/refcount.rs),
                        // so match that here, and the cascade decref balances
                        // the park retain. A yield or suspending signal value
                        // is not pinned.
                        if crate::vm::fiber::is_terminal_signal(bits) {
                            signal_val = v;
                        }
                    }
                });
                if backing != Value::NIL {
                    check(&backing);
                }
                if template_val != Value::NIL {
                    check(&template_val);
                }
                for v in &env_vals {
                    check(v);
                }
                check(&signal_val);
                check(&closure_value);
                for v in &param_vals {
                    check(v);
                }
            }
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
