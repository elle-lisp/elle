use super::*;

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
        // content fields. Omitting this was a UAF: the table was freed at its
        // constructor's decref_point while the host still referenced it.
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
                // OWN region (built together by the lowerer), so the synthetic
                // backing ref below is filtered by `rid == own_id`. But a
                // closure built by SHARING another closure's env — `squelch` /
                // `attune` (src/primitives/meta.rs) clone the template and copy
                // the env's `(ptr, len)` pair, leaving the backing data in the
                // SOURCE closure's region — has its env backing in a DIFFERENT
                // region. Without a cross-region edge to that backing, the
                // source region is freed at its owning-scope decref while this
                // closure still reads the env (the protect+squelch+nested-yield
                // UAF: `populate_env` reads a freed page on first fiber resume).
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
                // template is co-region with its instance (self-edge, filtered
                // by `rid == own_id`); a template shared by `squelch`/`attune`
                // lives in the SOURCE closure's region (a real cross-region edge
                // keeping that region alive). Both route through `check`. A
                // `Shared` (Rc) template has no region Value and is skipped.
                if let crate::value::closure::TemplateRef::Region(tv) = &closure.template {
                    check(tv);
                }
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
                // into it; see region-struct-heap-key-uaf.lisp).
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
                // The fiber's `traits` edge is tracked by the top-level
                // `check(&obj.traits())` above, with every other traitable
                // variant. This arm covers only the fiber's closure/env/signal.
                // EXPERIMENT: keep the fiber's (immutable) closure alive. The
                // closure's env RegionSlice backing and its captured Values
                // live in the region where the closure was built — usually a
                // *different* activation than the fiber. Without a cross-region
                // edge here, the spawning activation frees that region while
                // the parked fiber still references it (closure env reads as
                // garbage on first resume).
                let mut env_vals: Vec<Value> = Vec::new();
                let mut backing = Value::NIL;
                // A parked/dead fiber holds its result/yield value in `signal`,
                // read later by `fiber/value`. That value lives in whatever
                // region the fiber allocated it in — a region the parent will
                // `DecrefValueRegion` when the producing call's decref_point fires.
                // Track it as a cross-region edge so the fiber keeps it alive
                // until the fiber itself dies (cascade-decref here at free).
                // The matching incref is added when the fiber parks/dies (see
                // `incref_signal_region` in vm/fiber.rs).
                let mut signal_val = Value::NIL;
                // The fiber's closure holds a `Region` template (a region-allocated
                // `HeapObject::ClosureTemplate`) usually CO-region with the env
                // backing, but for an EMPTY-env closure there is no backing synth —
                // so the template edge must be tracked explicitly, else the fiber's
                // own template region is freed while the parked fiber still needs it
                // (`closure.template.code()`/arity read garbage on first resume).
                let mut template_val = Value::NIL;
                // The fiber's `closure_value` — the wrapper VALUE it installs as the
                // body's executing-closure register (`Fiber::closure_value`,
                // `pending_entry_closure` on first resume). For a `MakeClosure`
                // closure its region coincides with the env/template region tracked
                // above (a duplicate edge, balanced against a duplicate free-cascade
                // decref by the counted `outgoing` multiset). But a `squelch`/`attune`
                // wrapper (src/primitives/meta/syntaxops.rs) shares the SOURCE
                // closure's template and env yet is itself a fresh closure value in a
                // DIFFERENT region — so its region is reachable ONLY through
                // `closure_value`. Without this edge that region frees at its binding's
                // decref_point while the fiber still holds the value, and a later
                // free's cross-ref scan reads the freed page
                // (tests/elle/region-squelch-fiber-uaf.lisp).
                let mut closure_value = Value::NIL;
                let _ = handle.try_with(|fib| {
                    closure_value = fib.closure_value;
                    // An empty env uses a dangling sentinel pointer (not in any
                    // region page) — skip it. For a non-empty env, synthesize a
                    // heap Value at the backing so the shared `check` routes the
                    // closure's own region (where the RegionSlice lives) the
                    // same way as any cross-region ref.
                    if !fib.closure.env.is_empty() {
                        let p = fib.closure.env.as_ptr() as *const ();
                        backing = Value::from_heap_ptr(p, crate::value::repr::TAG_ARRAY);
                    }
                    if let crate::value::closure::TemplateRef::Region(tv) = &fib.closure.template {
                        template_val = *tv;
                    }
                    for v in fib.closure.env.iter() {
                        env_vals.push(*v);
                    }
                    if let Some((bits, v)) = fib.signal {
                        // Only terminal results are park-retained (see
                        // vm/fiber.rs `is_terminal_signal`); match that here so
                        // the cascade-decref balances the park-retain. Yield /
                        // suspending signal values are not pinned.
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
            }
            HeapObject::ClosureTemplate(t) => {
                // The template's constant pool. These are immediates
                // (string/quoted literals are their own `MaterializeConst`
                // allocations, not pool Values), so this
                // is normally a no-op — but scan it for symmetry so any future
                // region-allocated constant is RC-tracked (alloc-scan increfs,
                // free-cascade decrefs the same edge). The `child_protos`
                // blueprints are plain `Rc` data with no region edge — skipped.
                for v in t.constants.iter() {
                    check(v);
                }
            }
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
