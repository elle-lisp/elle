//! Region table: maps RegionId → RegionEntry (RegionPool + RC).
//!
//! `RegionStore` lives on `FiberHeap` and owns the `PagePool` (per-thread
//! page cache). Each region is lazily created on first use.
//!
//! ## Reference counting
//!
//! Each region has a `u32` RC that starts at 1 (scope ref).
//! `incref(id)` / `decref(id)` adjust it. `free_region(id)` is a
//! decref: if RC > 0 it decrements and frees when RC reaches 0.
//! The initial 1 is the scope's ref; cross-region refs add beyond that.
//!
//! ## Cascading frees
//!
//! When a region is freed, mutable collections in it may reference objects
//! in other regions. `teardown_and_cascade` walks collection contents and
//! decrefs each referenced region. This is a worklist, not recursion.

use super::pagepool::PagePool;
use super::regionpool::RegionPool;
use crate::value::heap::HeapObject;
use crate::value::inline_slice::InlineSlice;
use crate::value::Value;

/// Per-region entry: storage pool + reference count.
struct RegionEntry {
    pool: RegionPool,
    /// Cross-region reference count. FreeRegion is a decref.
    rc: u32,
}

/// Region table on FiberHeap.
pub(crate) struct RegionStore {
    /// Indexed by RegionId. `None` = not yet created.
    regions: Vec<Option<RegionEntry>>,
    /// Per-thread page cache shared across all regions.
    pool: PagePool,
}

impl RegionStore {
    pub fn new(initial_page_size: usize, max_cached: usize) -> Self {
        RegionStore {
            regions: Vec::new(),
            pool: PagePool::new(initial_page_size, max_cached),
        }
    }

    /// Ensure a region entry exists for `id`, creating it lazily.
    fn ensure(&mut self, id: u16) {
        let idx = id as usize;
        if idx >= self.regions.len() {
            self.regions.resize_with(idx + 1, || None);
        }
        if self.regions[idx].is_none() {
            self.regions[idx] = Some(RegionEntry {
                pool: RegionPool::new(id, self.pool.initial_page_size()),
                rc: 1,
            });
        }
    }

    /// Allocate a HeapObject into a specific region.
    /// Automatically increfs any cross-region Value refs in the object.
    pub fn alloc_obj(&mut self, id: u16, obj: HeapObject) -> Value {
        self.ensure(id);
        if crate::config::get().has_trace("rc") {
            let page_size = self.pool.initial_page_size();
            let valid_region = |rid: u16| -> bool {
                let ridx = rid as usize;
                ridx < self.regions.len() && self.regions[ridx].is_some()
            };
            let mut refs = Vec::new();
            RegionPool::collect_value_refs(&obj, id, page_size, &valid_region, &mut refs);
            if !refs.is_empty() {
                eprintln!(
                    "[trace:rc] alloc_obj({id}) xrefs={refs:?} tag={:?}",
                    obj.tag()
                );
            }
        }
        self.incref_cross_region_refs(&obj, id);
        let entry = self.regions[id as usize].as_mut().unwrap();
        entry.pool.alloc_obj(obj, &mut self.pool)
    }

    /// Allocate an inline slice into a specific region.
    pub fn alloc_inline_slice<T: Copy + 'static>(
        &mut self,
        id: u16,
        items: &[T],
    ) -> InlineSlice<T> {
        self.ensure(id);
        let entry = self.regions[id as usize].as_mut().unwrap();
        entry.pool.alloc_inline_slice(items, &mut self.pool)
    }

    /// Increment the reference count for a region.
    pub fn incref(&mut self, id: u16) {
        self.ensure(id);
        let entry = self.regions[id as usize].as_mut().unwrap();
        entry.rc = entry.rc.saturating_add(1);
        if crate::config::get().has_trace("rc") {
            eprintln!("[trace:rc] incref({id}) → rc={}", entry.rc);
        }
    }

    /// Decrement the reference count for a region.
    /// If RC is already 0, free the region. Otherwise decrement,
    /// and free if RC reaches 0. Returns the number of objects freed.
    pub fn decref(&mut self, id: u16) -> usize {
        self.decref_with_cascade(id, None)
    }

    /// Decrement with optional cascade source (for tracing).
    /// Returns the number of objects freed (0 if RC > 0 after decrement).
    fn decref_with_cascade(&mut self, id: u16, from_cascade: Option<u16>) -> usize {
        let idx = id as usize;
        // Direct decrefs (not from cascade) must hit a region that
        // was actually allocated. A miss here means either (a) the
        // solver assigned a region id that no instruction
        // alloc_in_region'd (the phantom-region class of bug —
        // see docs/regions.md § "Every region must correspond to a
        // real allocation"), or (b) a double-free. Cascade decrefs
        // are exempt because a single region may be referenced
        // multiple times by another's contents and may have already
        // been freed by an earlier cascade visit.
        debug_assert!(
            from_cascade.is_some()
                || (idx < self.regions.len() && self.regions[idx].is_some()),
            "DecrefRegion({id}) but region was never alloc_in_region'd \
             (or already freed) — phantom region or double-free; \
             see docs/regions.md § 'Every region must correspond to a \
             real allocation'",
        );
        if idx >= self.regions.len() {
            return 0;
        }
        let should_free = if let Some(entry) = &mut self.regions[idx] {
            if entry.rc == 0 {
                if crate::config::get().has_trace("rc") {
                    let src =
                        from_cascade.map_or("direct".to_string(), |s| format!("cascade({s})"));
                    eprintln!(
                        "[trace:rc] decref({id}) rc=0 → FREE objs={} src={src}",
                        entry.pool.obj_count()
                    );
                }
                true
            } else {
                entry.rc -= 1;
                if crate::config::get().has_trace("rc") {
                    let src =
                        from_cascade.map_or("direct".to_string(), |s| format!("cascade({s})"));
                    if entry.rc == 0 {
                        eprintln!(
                            "[trace:rc] decref({id}) → rc=0 → FREE objs={} src={src}",
                            entry.pool.obj_count()
                        );
                    } else {
                        eprintln!("[trace:rc] decref({id}) → rc={} src={src}", entry.rc);
                    }
                }
                entry.rc == 0
            }
        } else {
            false
        };
        if should_free {
            self.do_free(id)
        } else {
            0
        }
    }

    /// Get the current RC for a region (0 if not created).
    pub fn rc(&self, id: u16) -> u32 {
        let idx = id as usize;
        if idx < self.regions.len() {
            self.regions[idx].as_ref().map_or(0, |e| e.rc)
        } else {
            0
        }
    }

    /// Free a region — tolerant of "never alloc'd" callers.
    ///
    /// This is the entry point for runtime/macro callers
    /// (`with_transient_region!`, `vm/call.rs` alloc-region cleanup,
    /// embedding API) that reserve a region id without necessarily
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
    pub fn free_region(&mut self, id: u16) -> usize {
        let idx = id as usize;
        if idx >= self.regions.len() || self.regions[idx].is_none() {
            return 0;
        }
        self.decref(id)
    }

    /// Scan a HeapObject for cross-region Value refs and incref each.
    /// Called at allocation time to balance cascade decrefs at free time.
    fn incref_cross_region_refs(&mut self, obj: &HeapObject, own_id: u16) {
        let page_size = self.pool.initial_page_size();
        let valid_region = |rid: u16| -> bool {
            let ridx = rid as usize;
            ridx < self.regions.len() && self.regions[ridx].is_some()
        };
        let mut refs = Vec::new();
        RegionPool::collect_value_refs(obj, own_id, page_size, &valid_region, &mut refs);
        for rid in refs {
            self.incref(rid);
        }
    }

    /// Actually tear down a region: collect cross-region refs, run dtors,
    /// return pages, then decref referenced regions (may cascade).
    fn do_free(&mut self, id: u16) -> usize {
        let idx = id as usize;
        if idx >= self.regions.len() {
            return 0;
        }
        if let Some(mut entry) = self.regions[idx].take() {
            let page_size = self.pool.initial_page_size();
            let valid_region = |rid: u16| -> bool {
                let ridx = rid as usize;
                ridx < self.regions.len() && self.regions[ridx].is_some()
            };
            let cross_refs = entry
                .pool
                .collect_cross_region_refs(id, page_size, &valid_region);
            let freed = entry.pool.teardown(&mut self.pool);
            if crate::config::get().has_trace("rc") && !cross_refs.is_empty() {
                eprintln!("[trace:rc] do_free({id}) cascade: {cross_refs:?}");
            }
            for ref_id in cross_refs {
                self.decref_with_cascade(ref_id, Some(id));
            }
            freed
        } else {
            0
        }
    }

    /// Check if a pointer is owned by any region in this store.
    pub fn owns(&self, ptr: *const ()) -> bool {
        self.regions
            .iter()
            .any(|r| r.as_ref().is_some_and(|e| e.pool.owns(ptr)))
    }

    #[cfg(test)]
    pub fn region_obj_count(&self, id: u16) -> usize {
        let idx = id as usize;
        if idx < self.regions.len() {
            self.regions[idx].as_ref().map_or(0, |e| e.pool.obj_count())
        } else {
            0
        }
    }

    #[cfg(test)]
    pub fn total_obj_count(&self) -> usize {
        self.regions
            .iter()
            .filter_map(|r| r.as_ref())
            .map(|e| e.pool.obj_count())
            .sum()
    }

    /// Total allocated bytes across all regions + cached pages.
    pub fn allocated_bytes(&self) -> usize {
        let region_bytes: usize = self
            .regions
            .iter()
            .filter_map(|r| r.as_ref())
            .map(|e| e.pool.allocated_bytes())
            .sum();
        region_bytes + self.pool.cached_bytes()
    }

    /// Page size used by this store's pool.
    pub fn page_size(&self) -> usize {
        self.pool.initial_page_size()
    }

    /// Number of active (non-empty) regions.
    pub fn active_region_count(&self) -> usize {
        self.regions.iter().filter(|r| r.is_some()).count()
    }

    /// Per-region info: (region_id, rc, object_count) for every active region.
    pub fn region_info_vec(&self) -> Vec<(u16, u32, usize)> {
        self.regions
            .iter()
            .enumerate()
            .filter_map(|(idx, slot)| {
                slot.as_ref()
                    .map(|e| (idx as u16, e.rc, e.pool.obj_count()))
            })
            .collect()
    }

    /// Tear down all regions (fiber death).
    pub fn teardown_all(&mut self) {
        for slot in self.regions.iter_mut() {
            if let Some(mut entry) = slot.take() {
                entry.pool.teardown(&mut self.pool);
            }
        }
    }
}

impl Drop for RegionStore {
    fn drop(&mut self) {
        self.teardown_all();
    }
}

impl Default for RegionStore {
    fn default() -> Self {
        Self::new(4096, 4 * 1024 * 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::heap::Pair;

    fn cons_obj() -> HeapObject {
        HeapObject::Pair(Pair::new(Value::NIL, Value::NIL))
    }

    #[test]
    fn alloc_obj_creates_region_lazily() {
        let mut store = RegionStore::default();
        let v = store.alloc_obj(5, cons_obj());
        assert!(v.is_heap());
        assert_eq!(store.region_obj_count(5), 1);
    }

    #[test]
    fn alloc_inline_slice_in_region() {
        let mut store = RegionStore::default();
        let s = store.alloc_inline_slice(3, b"hello");
        assert_eq!(s.as_slice(), b"hello");
    }

    #[test]
    fn free_region_tears_down() {
        let mut store = RegionStore::default();
        for _ in 0..10 {
            store.alloc_obj(1, cons_obj());
        }
        assert_eq!(store.region_obj_count(1), 10);
        // rc=0, decref frees immediately.
        store.decref(1);
        assert_eq!(store.region_obj_count(1), 0);
    }

    #[test]
    #[should_panic(expected = "DecrefRegion(99) but region was never alloc_in_region'd")]
    fn decref_of_unallocated_region_panics_in_debug() {
        // Decref of a region id that was never alloc_in_region'd
        // is the "phantom region" class of bug — solver assigned a
        // region id to a node whose lowerer emits no alloc
        // instruction (DerefCell, MakeCell pre-fix; Eval without
        // call_result_regions registration). docs/regions.md
        // § "Every region must correspond to a real allocation"
        // documents the rule; this debug_assert! catches violators
        // at the runtime boundary.
        let mut store = RegionStore::default();
        store.decref(99); // never allocated — debug build panics
    }

    #[test]
    #[should_panic(expected = "DecrefRegion(1) but region was never alloc_in_region'd")]
    fn double_decref_panics_in_debug() {
        // A region freed once must not be decref'd again. The
        // bytecode emitter must not produce two DecrefRegion(N)
        // instructions for the same N along the same path. The
        // saturating-arithmetic tolerance the data structure used
        // to provide hid bugs that the regions audit was exactly
        // chasing — replace tolerance with loud failure in debug.
        let mut store = RegionStore::default();
        store.alloc_obj(1, cons_obj());
        store.decref(1); // rc=1 → 0, region freed, slot becomes None
        store.decref(1); // debug build panics on the second decref
    }

    #[test]
    fn rc_prevents_free() {
        let mut store = RegionStore::default();
        store.alloc_obj(1, cons_obj()); // rc=1 (scope ref)
        store.incref(1); // rc=2 (simulate cross-region ref)
        assert_eq!(store.rc(1), 2);

        // FreeRegion (decref): rc 2→1, not freed (cross-ref holds it).
        store.decref(1);
        assert_eq!(store.rc(1), 1);
        assert_eq!(
            store.region_obj_count(1),
            1,
            "region not freed while rc > 0"
        );

        // Cascade decref: rc 1→0, freed.
        store.decref(1);
        assert_eq!(
            store.region_obj_count(1),
            0,
            "region freed when rc reaches 0"
        );
    }

    #[test]
    fn incref_decref_basic() {
        let mut store = RegionStore::default();
        store.alloc_obj(7, cons_obj()); // rc=1 (scope ref)
        store.incref(7); // rc=2
        assert_eq!(store.rc(7), 2);
        store.decref(7); // rc=1
        assert_eq!(store.rc(7), 1);
        store.decref(7); // rc=0, freed
        assert_eq!(store.rc(7), 0);
    }

    #[test]
    fn decref_at_zero_frees() {
        let mut store = RegionStore::default();
        store.alloc_obj(1, cons_obj());
        assert_eq!(store.region_obj_count(1), 1);
        store.decref(1);
        assert_eq!(store.region_obj_count(1), 0);
    }

    #[test]
    fn total_obj_count_across_regions() {
        let mut store = RegionStore::default();
        store.alloc_obj(1, cons_obj());
        store.alloc_obj(1, cons_obj());
        store.alloc_obj(2, cons_obj());
        assert_eq!(store.total_obj_count(), 3);
    }

    #[test]
    fn teardown_all_clears_everything() {
        let mut store = RegionStore::default();
        store.alloc_obj(1, cons_obj());
        store.alloc_obj(2, cons_obj());
        store.alloc_obj(3, cons_obj());
        store.teardown_all();
        assert_eq!(store.total_obj_count(), 0);
    }

    #[test]
    fn owns_detects_region_pointers() {
        let mut store = RegionStore::default();
        let v = store.alloc_obj(1, cons_obj());
        let ptr = v.as_heap_ptr().unwrap();
        assert!(store.owns(ptr));

        let x: i64 = 42;
        assert!(!store.owns(&x as *const _ as *const ()));
    }

    #[test]
    fn dtors_run_on_free() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let mut store = RegionStore::default();
        let cell = Rc::new(RefCell::new(Value::NIL));
        let weak = Rc::downgrade(&cell);
        store.alloc_obj(
            1,
            HeapObject::LBox {
                cell,
                traits: Value::NIL,
            },
        );
        store.decref(1);
        assert!(
            weak.upgrade().is_none(),
            "Rc should be dropped when region is freed"
        );
    }

    #[test]
    fn multiple_regions_independent() {
        let mut store = RegionStore::default();
        store.alloc_obj(1, cons_obj());
        store.alloc_obj(1, cons_obj());
        store.alloc_obj(2, cons_obj());

        store.decref(1);
        assert_eq!(store.region_obj_count(1), 0);
        assert_eq!(
            store.region_obj_count(2),
            1,
            "region 2 should be unaffected"
        );
    }

    #[test]
    fn cascade_decrefs_cross_region_refs() {
        // Region 2 has a value (rc=1). Region 3 has an @array with that value.
        // alloc_obj auto-increfs region 2 for the cross-region ref → rc(2)=2.
        let mut store = RegionStore::default();
        let val_in_r2 = store.alloc_obj(2, cons_obj()); // rc(2)=1

        let arr = HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![val_in_r2])),
            traits: Value::NIL,
        };
        store.alloc_obj(3, arr); // auto-incref r2 → rc(2)=2

        assert_eq!(store.rc(2), 2);

        // Free region 3 — cascade decrefs region 2 → rc(2)=1 (scope ref remains).
        store.decref(3);
        assert_eq!(store.region_obj_count(3), 0, "region 3 should be freed");
        assert_eq!(
            store.rc(2),
            1,
            "cascade decrefs cross-region ref, scope ref remains"
        );
    }

    #[test]
    fn free_region_decrefs_escaped() {
        // Region 2 value held by @array in region 3. auto-incref → rc(2)=2.
        // FreeRegion(2) decrefs to 1 (cross-ref holds it).
        // Free r3 → cascade decrefs r2 → rc=0, freed.
        let mut store = RegionStore::default();
        let val_in_r2 = store.alloc_obj(2, cons_obj()); // rc(2)=1

        let arr = HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![val_in_r2])),
            traits: Value::NIL,
        };
        store.alloc_obj(3, arr); // auto-incref r2 → rc(2)=2

        assert_eq!(store.rc(2), 2);

        // FreeRegion(2): rc 2→1, not freed (cross-ref holds it).
        store.decref(2);
        assert_eq!(store.rc(2), 1);
        assert_eq!(
            store.region_obj_count(2),
            1,
            "region 2 held by cross-ref from r3"
        );

        // Free r3 → cascade decrefs r2 → rc=0, freed.
        store.decref(3);
        assert_eq!(
            store.region_obj_count(2),
            0,
            "region 2 freed after cascade from r3"
        );
    }

    #[test]
    fn cascade_box_cross_region() {
        let mut store = RegionStore::default();
        let val_in_r2 = store.alloc_obj(2, cons_obj()); // rc(2)=1

        let bx = HeapObject::LBox {
            cell: std::rc::Rc::new(std::cell::RefCell::new(val_in_r2)),
            traits: Value::NIL,
        };
        store.alloc_obj(3, bx); // auto-incref r2 → rc(2)=2

        store.decref(3); // cascade: rc(2)=1
        assert_eq!(
            store.rc(2),
            1,
            "cascade should decref box's cross-region ref"
        );
    }

    #[test]
    fn cascade_struct_mut_cross_region() {
        let mut store = RegionStore::default();
        let val_in_r2 = store.alloc_obj(2, cons_obj()); // rc(2)=1

        let mut map = std::collections::BTreeMap::new();
        map.insert(
            crate::value::TableKey::from_value(&Value::keyword("x")).unwrap(),
            val_in_r2,
        );
        let sm = HeapObject::LStructMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(map)),
            traits: Value::NIL,
        };
        store.alloc_obj(3, sm); // auto-incref r2 → rc(2)=2

        store.decref(3); // cascade: rc(2)=1
        assert_eq!(
            store.rc(2),
            1,
            "cascade should decref @struct's cross-region ref"
        );
    }

    #[test]
    fn cascade_pair_cross_region() {
        let mut store = RegionStore::default();
        let val_in_r2 = store.alloc_obj(2, cons_obj()); // rc(2)=1

        let pair = HeapObject::Pair(Pair::new(val_in_r2, Value::NIL));
        store.alloc_obj(3, pair); // auto-incref r2 → rc(2)=2

        store.decref(3); // cascade: rc(2)=1
        assert_eq!(
            store.rc(2),
            1,
            "cascade should decref pair's cross-region ref"
        );
    }
}
