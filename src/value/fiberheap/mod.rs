//! Per-fiber heap ownership and thread-local current-heap routing.
//!
//! `FiberHeap` uses a `SlabPool` (slab allocator + allocation tracking +
//! destructor list) for all allocations.
//!
//! `peak_alloc_count` tracks the high-water mark of `alloc_count` since the
//! last `clear()`. Updated on every `alloc()`. Queryable via `arena/peak`
//! and `arena/fiber-stats`.
//!
//! ## Per-value regions (FreeRegion)
//!
//! Each allocation is stamped with a region_id by the VM dispatch loop.
//! `FreeRegion(ρ)` walks the slab linked list and frees every slot whose
//! region_id matches ρ. No scope marks or scope stacks are needed.

use std::rc::Rc;

use crate::value::allocator::AllocatorBox;
use crate::value::arena::ArenaMark;
use crate::value::heap::{HeapObject, HeapTag};
use crate::value::Value;

mod routing;
pub use routing::*;

pub(crate) mod bump;
pub(crate) mod region;
pub(crate) mod slab;

pub(crate) mod pool;
use pool::SlabPool;

/// Tracks objects allocated by a single `with-allocator` invocation.
///
/// # Safety invariant
///
/// The `ArenaMark.custom_ptrs_len` field records the position in this
/// struct's `custom_ptrs` at `RegionEnter` time. This is safe because
/// `with-allocator` desugars to `defer`, which wraps the body in a fiber —
/// the body's scope marks live on the child fiber's `FiberHeap`, separate
/// from the parent's. If anyone calls `%install-allocator`/`%uninstall-allocator`
/// directly without a fiber boundary between install and scope marks,
/// `RegionExit` may dealloc from a popped allocator (use-after-free).
/// **These primitives must only be used via the `with-allocator` macro.**
pub(crate) struct CustomAllocState {
    /// The allocator trait object. `Rc` because the Elle `Value` also
    /// holds an `Rc` (via `ExternalObject.data`), and we need the
    /// allocator to outlive the form if cleanup happens during fiber death.
    allocator: Rc<AllocatorBox>,
    /// Objects allocated by this custom allocator.
    /// Each entry is (ptr, size, align) matching the alloc() call.
    /// Ordered by allocation time (oldest first).
    custom_ptrs: Vec<(*mut u8, usize, usize)>,
}

pub struct FiberHeap {
    /// Slab allocator with allocation and destructor tracking.
    /// Shared structure with `SharedAllocator`.
    pool: SlabPool,
    /// Double-buffered marks for JIT self-tail-call rotation.
    /// `jit_prev_mark` is released; `jit_curr_mark` shifts to prev.
    jit_prev_mark: Option<ArenaMark>,
    jit_curr_mark: Option<ArenaMark>,
    /// Peak number of objects allocated (high-water mark).
    peak_alloc_count: usize,
    /// Stack of custom allocators. The top is active.
    /// Pushed by `%install-allocator`, popped by `%uninstall-allocator`.
    custom_alloc_stack: Vec<CustomAllocState>,
    /// Maximum number of objects this fiber may allocate. `None` = unlimited.
    object_limit: Option<usize>,
    /// Allocation limit violation flag. Set by `alloc()` when `object_limit`
    /// is exceeded; read and cleared by the dispatch loop.
    ///
    /// Replaces the global `ALLOC_ERROR` thread-local — making it per-heap
    /// prevents cross-fiber confusion and eliminates a thread-local.
    alloc_error: Option<(usize, usize)>,
}

impl FiberHeap {
    pub fn new() -> Self {
        FiberHeap {
            pool: SlabPool::new(),
            jit_prev_mark: None,
            jit_curr_mark: None,
            peak_alloc_count: 0,
            custom_alloc_stack: Vec::new(),
            object_limit: None,
            alloc_error: None,
        }
    }

    pub fn alloc(&mut self, obj: HeapObject) -> Value {

        // Capture the Value-level tag before obj is moved.
        let value_tag = obj.value_tag();

        // Custom allocator: try Rust trait object before slab.
        if let Some(state) = self.custom_alloc_stack.last_mut() {
            let size = std::mem::size_of::<HeapObject>();
            let align = std::mem::align_of::<HeapObject>();
            let ptr = state.allocator.inner.alloc(size, align);
            if !ptr.is_null() {
                let typed = ptr as *mut HeapObject;
                let drop = needs_drop(obj.tag());
                // SAFETY: ptr is non-null, properly aligned (guaranteed by
                // ElleAllocator contract), and has at least size bytes.
                unsafe { std::ptr::write(typed, obj) };
                state.custom_ptrs.push((ptr, size, align));
                if drop {
                    self.pool.dtors.push(typed);
                }
                self.pool.alloc_count += 1;
                if self.pool.alloc_count > self.peak_alloc_count {
                    self.peak_alloc_count = self.pool.alloc_count;
                }
                return Value::from_heap_ptr(typed as *const (), value_tag);
            }
            // Fall through to slab on null return
        }

        // Check object limit before allocating
        if let Some(limit) = self.object_limit {
            if self.pool.alloc_count >= limit {
                self.alloc_error = Some((self.pool.alloc_count, limit));
                return Value::NIL;
            }
        }

        // Allocate from the slab pool.
        let v = self.pool.alloc(obj);
        if self.pool.alloc_count > self.peak_alloc_count {
            self.peak_alloc_count = self.pool.alloc_count;
        }
        v
    }

    /// Copy `items` into the current allocator's arena and return an
    /// `InlineSlice` pointing to them. Used by immutable collection
    /// constructors to store variable-length data inline.
    ///
    /// Routing mirrors `alloc()`: shared allocator → custom allocator →
    /// private pool. The slice shares the lifetime of adjacent `alloc()` calls.
    pub fn alloc_inline_slice<T: Copy + 'static>(
        &mut self,
        items: &[T],
    ) -> crate::value::inline_slice::InlineSlice<T> {
        if items.is_empty() {
            return crate::value::inline_slice::InlineSlice::empty();
        }
        // Custom allocator: allocate raw bytes, copy items, return slice.
        if let Some(state) = self.custom_alloc_stack.last_mut() {
            let size = std::mem::size_of_val(items);
            let align = std::mem::align_of::<T>();
            let ptr = state.allocator.inner.alloc(size, align);
            if !ptr.is_null() {
                let typed = ptr as *mut T;
                unsafe {
                    std::ptr::copy_nonoverlapping(items.as_ptr(), typed, items.len());
                }
                state.custom_ptrs.push((ptr, size, align));
                return unsafe {
                    crate::value::inline_slice::InlineSlice::from_raw(typed, items.len() as u32)
                };
            }
            // Fall through on null
        }
        // Private pool.
        self.pool.alloc_inline_slice(items)
    }

    pub fn mark(&self) -> ArenaMark {
        let custom_ptrs_len = self
            .custom_alloc_stack
            .last()
            .map_or(0, |s| s.custom_ptrs.len());
        ArenaMark::new_full(
            self.pool.alloc_count,
            self.pool.dtors.len(),
            custom_ptrs_len,
            self.pool.alloc_tail,
            0,
            Some(self.pool.mark().arena_mark),
        )
    }

    /// Release all allocations back to a mark unconditionally.
    pub fn release(&mut self, mark: ArenaMark) {
        use super::fiberheap::slab::ALLOC_NIL;

        if Self::trace_rc() {
            eprintln!("[trace:rc] release mark={}", mark.position());
        }

        // Run destructors for objects allocated after the mark.
        self.pool.run_dtors(mark.dtor_len());
        self.pool.dtors.truncate(mark.dtor_len());

        // Walk the alloc linked list from mark tail to current tail,
        // unlinking and deallocating each slot.
        let start = if mark.alloc_list_tail() == ALLOC_NIL {
            self.pool.alloc_head
        } else {
            self.pool.slab.alloc_next[mark.alloc_list_tail() as usize]
        };
        let mut cur = start;
        while cur != ALLOC_NIL {
            let next = self.pool.slab.alloc_next[cur as usize];
            let ptr = self.pool.slab.flat_to_ptr(cur as usize);
            self.pool.slab.dealloc(ptr);
            cur = next;
        }
        // Truncate the list at the mark point.
        self.pool.alloc_tail = mark.alloc_list_tail();
        if self.pool.alloc_tail == ALLOC_NIL {
            self.pool.alloc_head = ALLOC_NIL;
        } else {
            self.pool.slab.alloc_next[self.pool.alloc_tail as usize] = ALLOC_NIL;
        }

        // Dealloc custom-allocated objects from the exiting scope.
        if let Some(state) = self.custom_alloc_stack.last_mut() {
            let start = mark.custom_ptrs_len();
            for &(ptr, size, align) in state.custom_ptrs[start..].iter().rev() {
                state.allocator.inner.dealloc(ptr, size, align);
            }
            state.custom_ptrs.truncate(start);
        }
    }

    /// Free all objects whose region_id matches `region`.
    ///
    /// Walks the slab's allocation linked list. For each slot where
    /// `region_of(slot) == region`, runs the destructor (if needed),
    /// returns the slot to the free list, and unlinks it from the list.
    pub fn free_region(&mut self, region: u16) {
        use super::fiberheap::slab::ALLOC_NIL;

        let mut cur = self.pool.alloc_head;
        while cur != ALLOC_NIL {
            let next = self.pool.slab.alloc_next[cur as usize];
            if self.pool.slab.region_ids[cur as usize] == region {
                let ptr = self.pool.slab.flat_to_ptr(cur as usize);
                // Run destructor if needed.
                if needs_drop(unsafe { (*ptr).tag() }) {
                    unsafe { std::ptr::drop_in_place(ptr) };
                    // Null the dtor entry so run_dtors won't double-free.
                    self.pool.remove_from_dtors(ptr);
                }
                // Unlink from allocation list and return to free list.
                self.pool.unlink_alloc(cur);
                self.pool.slab.dealloc(ptr);
                self.pool.alloc_count = self.pool.alloc_count.saturating_sub(1);
            }
            cur = next;
        }
    }

    /// Release without deallocating slab slots.
    ///
    /// Called by `ArenaGuard::drop()` via `heap_arena_release()`. The
    /// ArenaGuard is a manual mark/release that does NOT go through
    /// Tofte-Talpin region analysis — it cannot prove which slab slots
    /// are dead. Only runs destructors and truncates tracking vecs.
    /// Slab slots are reclaimed later by teardown or RegionExit.
    pub fn release_no_dealloc(&mut self, mark: ArenaMark) {
        use super::fiberheap::slab::ALLOC_NIL;

        self.pool.run_dtors(mark.dtor_len());
        self.pool.dtors.truncate(mark.dtor_len());

        // Walk the linked list from mark tail to current tail and
        // unlink all nodes (but do NOT dealloc slab slots).
        let start = if mark.alloc_list_tail() == ALLOC_NIL {
            self.pool.alloc_head
        } else {
            self.pool.slab.alloc_next[mark.alloc_list_tail() as usize]
        };
        let mut cur = start;
        while cur != ALLOC_NIL {
            let next = self.pool.slab.alloc_next[cur as usize];
            // Clear links but don't dealloc.
            self.pool.slab.alloc_prev[cur as usize] = ALLOC_NIL;
            self.pool.slab.alloc_next[cur as usize] = ALLOC_NIL;
            cur = next;
        }
        // Truncate the list at the mark point.
        self.pool.alloc_tail = mark.alloc_list_tail();
        if self.pool.alloc_tail == ALLOC_NIL {
            self.pool.alloc_head = ALLOC_NIL;
        } else {
            self.pool.slab.alloc_next[self.pool.alloc_tail as usize] = ALLOC_NIL;
        }

        // Dealloc custom-allocated objects from the exiting scope.
        if let Some(state) = self.custom_alloc_stack.last_mut() {
            let start = mark.custom_ptrs_len();
            for &(ptr, size, align) in state.custom_ptrs[start..].iter().rev() {
                state.allocator.inner.dealloc(ptr, size, align);
            }
            state.custom_ptrs.truncate(start);
        }

        // NOTE: no bump rewind here. release_no_dealloc keeps slab slots
        // alive, so their inline data (InlineSlice pointers into the bump
        // arena) must also stay alive. Bump data is reclaimed later by
        // teardown or a full release().

        self.pool.alloc_count = mark.position();
    }


    /// Private heap object count (used by mark/release scoping).
    pub fn len(&self) -> usize {
        self.pool.alloc_count
    }

    /// Total allocations visible to this fiber, including objects routed
    /// to the parent's shared allocator.  Used by arena/count.
    pub fn visible_len(&self) -> usize {
        self.pool.alloc_count
    }

    pub fn is_empty(&self) -> bool {
        self.pool.alloc_count == 0
    }

    pub fn capacity(&self) -> usize {
        self.pool.capacity_bytes()
    }

    /// Get the current object limit.
    pub fn object_limit(&self) -> Option<usize> {
        self.object_limit
    }

    /// Set the object limit. Returns the previous limit.
    pub fn set_object_limit(&mut self, limit: Option<usize>) -> Option<usize> {
        let prev = self.object_limit;
        self.object_limit = limit;
        prev
    }

    /// Take the allocation error flag, clearing it.
    ///
    /// Returns `Some((count, limit))` if an allocation limit was exceeded
    /// since the last call, `None` otherwise. Used by the dispatch loop.
    pub fn take_alloc_error(&mut self) -> Option<(usize, usize)> {
        self.alloc_error.take()
    }

    /// Bytes committed by the slab allocator.
    pub fn allocated_bytes(&self) -> usize {
        self.pool.allocated_bytes()
    }

    /// Peak number of objects allocated (high-water mark).
    pub fn peak_alloc_count(&self) -> usize {
        self.peak_alloc_count
    }

    /// Number of objects in the destructor list.
    pub(crate) fn dtor_count(&self) -> usize {
        self.pool.dtor_count()
    }

    /// Number of live slots in the root slab.
    pub(crate) fn root_live(&self) -> usize {
        self.pool.live_count()
    }

    /// Number of root allocations tracked for release().
    pub(crate) fn root_alloc_count(&self) -> usize {
        self.pool.alloc_count
    }

    /// Number of owned shared allocators (legacy, always 0).
    pub(crate) fn shared_count(&self) -> usize {
        0
    }

    /// Reset peak to current count. Returns previous peak.
    pub fn reset_peak(&mut self) -> usize {
        let prev = self.peak_alloc_count;
        self.peak_alloc_count = self.pool.alloc_count;
        prev
    }

    /// Double-buffered rotation for JIT self-tail-call loops.
    ///
    /// Same one-iteration-lag protocol as the trampoline: release the
    /// mark from two iterations ago, shift curr → prev, capture fresh.
    pub fn rotate_pools_jit(&mut self) {
        // Release mark from two iterations ago.
        if let Some(mark) = self.jit_prev_mark.take() {
            self.release(mark);
        }
        // Shift curr → prev, capture fresh curr.
        self.jit_prev_mark = self.jit_curr_mark.take();
        self.jit_curr_mark = Some(self.mark());
    }

    /// Save the current JIT rotation state and reset to `None`.
    pub fn save_jit_rotation_base(&mut self) -> (Option<ArenaMark>, Option<ArenaMark>) {
        (self.jit_prev_mark.take(), self.jit_curr_mark.take())
    }

    /// Restore a previously saved JIT rotation state.
    pub fn restore_jit_rotation_base(&mut self, saved: (Option<ArenaMark>, Option<ArenaMark>)) {
        self.jit_prev_mark = saved.0;
        self.jit_curr_mark = saved.1;
    }

    /// Push a custom allocator onto the stack. Allocations will route
    /// to this allocator until it is popped.
    pub fn push_custom_allocator(&mut self, allocator: Rc<AllocatorBox>) {
        self.custom_alloc_stack.push(CustomAllocState {
            allocator,
            custom_ptrs: Vec::new(),
        });
    }

    /// Pop the top custom allocator, run Drop for remaining custom objects
    /// that are still in dtors, then dealloc all remaining custom memory.
    ///
    /// Returns `true` if an allocator was popped, `false` if the stack was empty.
    pub fn pop_custom_allocator(&mut self) -> bool {
        let state = match self.custom_alloc_stack.pop() {
            Some(s) => s,
            None => return false,
        };

        // For remaining custom objects (those not freed by RegionExit):
        // 1. Run Drop for those still in dtors
        // 2. Call dealloc for all of them
        //
        // We need to find which dtors point into our custom_ptrs set.
        // Since dtors is ordered and custom_ptrs is ordered, and
        // RegionExit already truncated both lists for scoped objects,
        // the remaining custom_ptrs entries have corresponding dtors
        // entries (if they need Drop) at the END of the dtors list.
        //
        // We walk custom_ptrs in reverse. For each, check if it appears
        // in dtors (as a HeapObject pointer). If so, drop_in_place and
        // remove from dtors.
        for &(ptr, size, align) in state.custom_ptrs.iter().rev() {
            let typed = ptr as *mut HeapObject;
            // Check if this pointer is in dtors and run Drop if so.
            if let Some(pos) = self.pool.dtors.iter().rposition(|&d| d == typed) {
                // SAFETY: The pointer is valid — it was allocated by the
                // custom allocator and has not been freed yet.
                unsafe { std::ptr::drop_in_place(typed) };
                self.pool.dtors.swap_remove(pos);
            }
            state.allocator.inner.dealloc(ptr, size, align);
        }
        true
    }

    /// Check whether a shared allocator is active (legacy, always false).
    pub fn has_shared_alloc(&self) -> bool {
        false
    }

    /// Check if a heap value's pointer is in this heap's private pool.
    pub fn value_in_private_pool(&self, value: Value) -> bool {
        if !value.is_heap() {
            return false;
        }
        let ptr = match value.as_heap_ptr() {
            Some(p) => p,
            None => return false,
        };
        self.pool.owns(ptr)
    }

    /// Check if a value is owned by this fiber's private pool.
    pub fn value_owned_by_fiber(&self, value: Value) -> bool {
        let ptr = match value.as_heap_ptr() {
            Some(p) => p,
            None => return false,
        };
        self.pool.owns(ptr)
    }


    // ── Refcounting ───────────────────────────────────────────────────

    /// Check if `--trace=rc` is active (zero-cost when off: one static read + AND).
    #[inline(always)]
    pub(crate) fn trace_rc() -> bool {
        crate::config::get().trace_bits() & crate::config::trace_bits::RC != 0
    }

    /// Increment the durable reference count for a heap value.
    /// No-op for non-heap values (int, float, bool, nil, keyword, symbol).
    #[inline]
    pub fn incref_value(&mut self, val: Value) {
        if !val.is_heap() {
            return;
        }
        if let Some(ptr) = val.as_heap_ptr() {
            if self.pool.slab_owns(ptr) {
                self.pool.incref(ptr as *const HeapObject);
                if Self::trace_rc() {
                    let rc = self.pool.refcount(ptr as *const HeapObject);
                    eprintln!("[trace:rc] incref {:?} → rc={}", ptr, rc);
                }
            }
        }
    }

    /// Decrement the durable reference count for a heap value.
    /// No-op for non-heap values. Returns the new refcount (0 if non-heap).
    pub fn decref_value(&mut self, val: Value) -> u32 {
        if !val.is_heap() {
            return 0;
        }
        if let Some(ptr) = val.as_heap_ptr() {
            if self.pool.slab_owns(ptr) {
                let old_rc = self.pool.refcount(ptr as *const HeapObject);
                if old_rc == 0 {
                    return 0; // Already at 0, no transition.
                }
                let new_rc = self.pool.decref(ptr as *const HeapObject);
                if Self::trace_rc() {
                    eprintln!("[trace:rc] decref {:?} → rc={}", ptr, new_rc);
                }
                return new_rc;
            }
        }
        0
    }

    /// Get the durable reference count for a heap value.
    #[inline]
    pub fn refcount_value(&self, val: Value) -> u32 {
        if !val.is_heap() {
            return 0;
        }
        if let Some(ptr) = val.as_heap_ptr() {
            if self.pool.slab_owns(ptr) {
                return self.pool.refcount(ptr as *const HeapObject);
            }
        }
        0
    }

    /// Set the region id for a heap value.
    #[inline]
    pub fn stamp_region(&mut self, val: Value, region: u16) {
        if let Some(ptr) = val.as_heap_ptr() {
            if self.pool.slab_owns(ptr) {
                self.pool
                    .slab
                    .set_region(ptr as *const HeapObject, region);
            }
        }
    }

    /// Get the region id for a heap value. Returns 0 (default region)
    /// for non-heap values or values not owned by this slab.
    #[inline]
    pub fn region_of(&self, val: Value) -> u16 {
        if !val.is_heap() {
            return 0;
        }
        if let Some(ptr) = val.as_heap_ptr() {
            if self.pool.slab_owns(ptr) {
                return self.pool.slab.region_of(ptr as *const HeapObject);
            }
        }
        0
    }

    /// Decrement a value's refcount, and if it reaches 0, run its
    /// destructor and return its slab slot to the free list.
    ///
    /// Called at mutation points (put/push/set) when the old value is
    /// evicted from a collection. The old value is no longer referenced
    /// by any collection; if no other collection holds it (refcount 0),
    /// it can be freed immediately.
    pub fn decref_and_free(&mut self, val: Value) {
        // Just decref. With alloc-time child incref, children are
        // managed by their own refcounts — no recursive traversal.
        // The old value will be freed by release_refcounted when its
        // rc reaches 0 (from this decref or a subsequent DecrefLocal).
        self.decref_value(val);
    }

    /// Collect all heap-typed child Values from a HeapObject into `out`.
    pub(crate) fn collect_heap_children(obj: &HeapObject, out: &mut Vec<Value>) {
        // Helper: push traits if heap-allocated (permanent traitsets
        // won't match slab_owns, but user-attached traits will).
        let push_traits = |traits: &Value, out: &mut Vec<Value>| {
            if traits.is_heap() {
                out.push(*traits);
            }
        };
        match obj {
            HeapObject::LArrayMut { data, traits, .. } => {
                if let Ok(d) = data.try_borrow() {
                    out.extend(d.iter().filter(|v| v.is_heap()).copied());
                }
                push_traits(traits, out);
            }
            HeapObject::LStructMut { data, traits, .. } => {
                if let Ok(d) = data.try_borrow() {
                    out.extend(d.values().filter(|v| v.is_heap()).copied());
                }
                push_traits(traits, out);
            }
            HeapObject::LArray {
                elements, traits, ..
            } => {
                out.extend(elements.as_slice().iter().filter(|v| v.is_heap()).copied());
                push_traits(traits, out);
            }
            HeapObject::LStruct { data, traits, .. } => {
                out.extend(data.iter().map(|(_, v)| *v).filter(|v| v.is_heap()));
                push_traits(traits, out);
            }
            HeapObject::Pair(pair) => {
                out.extend(
                    [pair.first, pair.rest, pair.traits]
                        .iter()
                        .filter(|v| v.is_heap())
                        .copied(),
                );
            }
            HeapObject::Closure {
                closure, traits, ..
            } => {
                out.extend(
                    closure
                        .env
                        .as_slice()
                        .iter()
                        .filter(|v| v.is_heap())
                        .copied(),
                );
                push_traits(traits, out);
            }
            HeapObject::LBox { cell, traits, .. }
            | HeapObject::CaptureCell { cell, traits, .. } => {
                if let Ok(v) = cell.try_borrow() {
                    if v.is_heap() {
                        out.push(*v);
                    }
                }
                push_traits(traits, out);
            }
            HeapObject::LSet { data, traits, .. } => {
                out.extend(data.as_slice().iter().filter(|v| v.is_heap()).copied());
                push_traits(traits, out);
            }
            HeapObject::LSetMut { data, traits, .. } => {
                if let Ok(d) = data.try_borrow() {
                    out.extend(d.iter().filter(|v| v.is_heap()).copied());
                }
                push_traits(traits, out);
            }
            HeapObject::LString { traits, .. }
            | HeapObject::LStringMut { traits, .. }
            | HeapObject::LBytes { traits, .. }
            | HeapObject::LBytesMut { traits, .. }
            | HeapObject::Syntax { traits, .. }
            | HeapObject::ManagedPointer { traits, .. }
            | HeapObject::External { traits, .. }
            | HeapObject::Fiber { traits, .. }
            | HeapObject::ThreadHandle { traits, .. } => {
                push_traits(traits, out);
            }
            HeapObject::Parameter {
                default, traits, ..
            } => {
                out.extend(std::iter::once(*default).filter(|v| v.is_heap()));
                push_traits(traits, out);
            }
            _ => {}
        }
    }

    /// Drop all tracked objects and reset the slab allocator.
    pub fn clear(&mut self) {
        // Dealloc all custom-allocated objects (dtors run by pool.teardown below).
        // We need to run custom dtors and dealloc before pool.teardown
        // because pool.teardown will clear dtors.
        // Actually: run pool dtors first (covers both slab and custom objects),
        // then dealloc custom memory, then clear pool slab.
        self.pool.run_dtors(0);
        self.pool.dtors.clear();

        // Dealloc all custom-allocated objects.
        for state in self.custom_alloc_stack.drain(..) {
            for &(ptr, size, align) in state.custom_ptrs.iter().rev() {
                state.allocator.inner.dealloc(ptr, size, align);
            }
            // Rc<AllocatorBox> dropped here
        }

        // Clear pool slab tracking and reset slab (keeps first chunk).
        // SAFETY: all dtors have been run above.
        unsafe { self.pool.clear_slab() };

        self.alloc_error = None;
        self.pool.alloc_count = 0;
        self.peak_alloc_count = 0;
        self.jit_prev_mark = None;
        self.jit_curr_mark = None;
    }
}

impl Drop for FiberHeap {
    fn drop(&mut self) {
        // ── Approach B: cross-pool Rc sharing assertion ──────────────
        //
        // Check for RcInner sharing across ALL pools BEFORE any teardown.
        // If two objects (possibly on different pools: private, outbox,
        // shared alloc) share an RcInner, tearing down one pool drops
        // one object (freeing the RcInner) and the other pool's dtor
        // hits freed memory → UAF.
        #[cfg(debug_assertions)]
        {
            use std::cell::RefCell;
            use std::collections::HashSet;

            fn extract_rc_inner(
                ptr: *mut HeapObject,
            ) -> Option<(usize, super::heap::HeapTag, *mut HeapObject)> {
                let obj = unsafe { &*ptr };
                match obj {
                    HeapObject::LBox { cell, .. } => {
                        Some((&**cell as *const RefCell<Value> as usize, obj.tag(), ptr))
                    }
                    HeapObject::CaptureCell { cell, .. } => {
                        Some((&**cell as *const RefCell<Value> as usize, obj.tag(), ptr))
                    }
                    _ => None,
                }
            }

            fn collect_pool_rc_inners(
                dtors: &[*mut HeapObject],
            ) -> Vec<(usize, super::heap::HeapTag, *mut HeapObject)> {
                dtors
                    .iter()
                    .filter_map(|&ptr| {
                        if ptr.is_null() {
                            None
                        } else {
                            extract_rc_inner(ptr)
                        }
                    })
                    .collect()
            }

            let mut seen: HashSet<usize> = HashSet::new();
            #[allow(clippy::type_complexity)]
            let mut duplicates: Vec<(
                (usize, super::heap::HeapTag, *mut HeapObject),
                (usize, super::heap::HeapTag, *mut HeapObject),
            )> = Vec::new();

            let private_entries = collect_pool_rc_inners(&self.pool.dtors);

            let all_entries: Vec<_> = private_entries;

            for entry @ (addr, _, ptr) in &all_entries {
                if !seen.insert(*addr) {
                    if let Some(prev) = all_entries
                        .iter()
                        .find(|(a, _, p)| *a == *addr && !std::ptr::eq(*p, *ptr))
                    {
                        duplicates.push((*prev, *entry));
                    }
                }
            }

            if !duplicates.is_empty() {
                for ((addr_a, tag_a, ptr_a), (_addr_b, tag_b, ptr_b)) in &duplicates {
                    eprintln!(
                        "[invariant] DUPLICATE RcInner 0x{:x}: {:?} at {:?} and {:?} at {:?}",
                        addr_a, tag_a, ptr_a, tag_b, ptr_b
                    );
                }
                panic!(
                    "FiberHeap::drop invariant violated: {} duplicate RcInner(s) across pools",
                    duplicates.len()
                );
            }
        }

        // Run destructors for all tracked objects while slab memory is still valid.
        self.pool.run_dtors(0);
        self.pool.dtors.clear(); // Prevent SlabPool::Drop from double-dropping.
                                 // Dealloc custom-allocated objects. Drop has already run above.
        for state in self.custom_alloc_stack.drain(..) {
            for &(ptr, size, align) in state.custom_ptrs.iter().rev() {
                state.allocator.inner.dealloc(ptr, size, align);
            }
        }
        // pool (and its slab) drops implicitly here. MaybeUninit slots do not
        // call HeapObject::drop — dtors have already run above.
    }
}

impl Default for FiberHeap {
    fn default() -> Self {
        Self::new()
    }
}

/// Exhaustive check: does this HeapObject variant have inner heap allocations
/// that require Drop? No wildcard arm — adding a new HeapObject variant
/// forces a decision here (compile error).
pub(crate) fn needs_drop(tag: HeapTag) -> bool {
    match tag {
        // Copy/scalar innards — no heap allocations
        HeapTag::Pair => false,
        // LBox and CaptureCell hold Rc<RefCell<Value>> for cross-fiber
        // sharing; dropping them must decrement the Rc strong count.
        HeapTag::LBox => true,
        HeapTag::CaptureCell => true,
        HeapTag::Float => false,
        HeapTag::NativeFn => false,
        HeapTag::LibHandle => false,
        HeapTag::ManagedPointer => false,
        // Inner heap allocations (Box<str>, Vec, Rc, BTreeMap, Arc, Cif, etc.)
        HeapTag::LString => true,
        HeapTag::LArrayMut => true,
        HeapTag::LStructMut => true,
        HeapTag::LStruct => true,
        HeapTag::Closure => true,
        HeapTag::LArray => true,
        HeapTag::LStringMut => true,
        HeapTag::LBytes => true,
        HeapTag::LBytesMut => true,
        HeapTag::Syntax => true,
        HeapTag::Fiber => true,
        HeapTag::ThreadHandle => true,
        HeapTag::FFISignature => true,
        HeapTag::FFIType => true,
        HeapTag::External => true,
        // Parameter contains a Value (Copy) — no inner heap allocations
        HeapTag::Parameter => false,
        // Sets (immutable) contain BTreeSet which needs Drop
        HeapTag::LSet => true,
        // Sets (mutable) contain RefCell<BTreeSet> which needs Drop
        HeapTag::LSetMut => true,
    }
}

#[cfg(test)]
mod tests;
