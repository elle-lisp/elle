//! Per-fiber heap ownership and thread-local current-heap routing.
//!
//! `FiberHeap` uses a `SlabPool` (slab allocator + allocation tracking +
//! destructor list) for all allocations. The pool is shared with
//! `SharedAllocator`, which wraps the same `SlabPool` type for inter-fiber
//! value exchange.
//!
//! `peak_alloc_count` tracks the high-water mark of `alloc_count` since the
//! last `clear()`. Updated on every `alloc()`. Queryable via `arena/peak`
//! and `arena/fiber-stats`.
//!
//! ## Scope marks
//!
//! `FiberHeap` maintains a stack of scope marks (`scope_marks: Vec<ArenaMark>`)
//! for `RegionEnter`/`RegionExit` bytecodes. `RegionEnter` pushes a mark
//! recording the current slab position; `RegionExit` pops the mark and calls
//! `release()` to run destructors and deallocate slab slots for objects
//! allocated within the scope, returning them to the slab free list.
//!
//! The lowerer gates `RegionEnter`/`RegionExit` emission on escape analysis
//! (`src/lir/lower/escape.rs`): only scopes where no allocated values can
//! escape get region instructions. The analysis checks: no captures, no
//! suspension, result is immediate, no outward mutation.
//!
//! ## Outbox for inter-fiber value exchange
//!
//! When a child fiber yields, the yielded value must survive the child's
//! death so the parent can read it. The outbox mechanism handles this:
//!
//! - Parent installs a fresh `SlabPool` outbox before each child execution
//! - Child receives a borrowed pointer to the parent's outbox
//! - `OutboxEnter`/`OutboxExit` bytecodes route allocations to the outbox
//! - `deep_copy_to_outbox` copies private-pool values to the outbox at yield
//!
//! ## Shared allocator (legacy)
//!
//! `FiberHeap` owns zero or more `SharedAllocator`s (in `owned_shared`)
//! and has a `shared_alloc` pointer for routing. When non-null, `alloc()`
//! routes ALL allocations to the shared allocator. This is a legacy
//! mechanism that will be replaced by per-region allocation routing.

#[cfg(feature = "ffi")]
use std::cell::RefCell;
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
    /// Stack of scope marks pushed by `RegionEnter`, popped by `RegionExit`.
    /// Each mark records the `(alloc_count, dtors.len())` at scope entry.
    /// `RegionExit` pops the mark and calls `release()` to run destructors
    /// for objects allocated within the scope.
    scope_marks: Vec<ArenaMark>,
    /// Shared allocators this fiber owns (as parent of yielding children).
    /// `Box` for pointer stability — descendant fibers hold raw pointers
    /// to the `SharedAllocator` data, which must not move when the `Vec` grows.
    #[allow(clippy::vec_box)]
    owned_shared: Vec<Box<crate::value::shared_alloc::SharedAllocator>>,
    /// Raw pointer to the shared allocator for inter-fiber value exchange.
    /// When non-null, `alloc()` routes all allocations to this shared
    /// allocator instead of the private slab. Set by `with_child_fiber`
    /// for yielding child fibers; nulled on swap-back.
    shared_alloc: *mut crate::value::shared_alloc::SharedAllocator,
    /// Number of `RegionEnter` instructions executed (scope marks pushed).
    scope_enters: usize,
    /// Number of destructors run by `RegionExit` (objects freed at scope exit).
    scope_dtors_run: usize,
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
    /// Count of allocations routed through the shared allocator (not owned
    /// by this heap).  Kept separate from `alloc_count` so that mark/release
    /// scoping is not affected.  `visible_len()` returns the sum.
    shared_alloc_count: usize,
    /// Owned outbox pools. Only non-empty on parent fibers.
    ///
    /// Current outbox is the last element; previous outboxes preserve
    /// earlier yield values so the parent can still reference them.
    /// All outboxes are freed on parent fiber death (`FiberHeap::drop`
    /// or `clear()`).
    ///
    /// `Box<SlabPool>` is intentional: boxing stabilizes the pool's address
    /// so that `outbox_ptr` (a raw pointer into the box) remains valid.
    #[allow(clippy::vec_box)]
    owned_outboxes: Vec<Box<SlabPool>>,
    /// Raw pointer to the current outbox pool (element of `owned_outboxes`
    /// on the PARENT's heap). Set on both parent and child:
    ///   - Parent: points into own `owned_outboxes`
    ///   - Child: borrowed pointer into parent's `owned_outboxes`
    ///
    /// Null when no outbox is active.
    ///
    /// The child NEVER owns the outbox. When the child's FiberHeap is dropped,
    /// `outbox_ptr` is simply nulled — no teardown. The parent owns the pool
    /// and tears it down when it's safe (at parent fiber death, or when the
    /// RC system confirms no references survive).
    outbox_ptr: *mut SlabPool,
    /// True when allocations should route to `outbox_ptr` (between
    /// `OutboxEnter` and `OutboxExit` bytecodes).
    outbox_active: bool,
}

impl FiberHeap {
    pub fn new() -> Self {
        FiberHeap {
            pool: SlabPool::new(),
            jit_prev_mark: None,
            jit_curr_mark: None,
            peak_alloc_count: 0,
            scope_marks: Vec::new(),
            owned_shared: Vec::new(),
            shared_alloc: std::ptr::null_mut(),
            scope_enters: 0,
            scope_dtors_run: 0,
            custom_alloc_stack: Vec::new(),
            object_limit: None,
            alloc_error: None,
            shared_alloc_count: 0,
            outbox_active: false,
            owned_outboxes: Vec::new(),
            outbox_ptr: std::ptr::null_mut(),
        }
    }

    pub fn alloc(&mut self, obj: HeapObject) -> Value {
        // Outbox routing: when outbox is active (between OutboxEnter/OutboxExit),
        // allocations go to the outbox pool for yield-bound values.
        if self.outbox_active && !self.outbox_ptr.is_null() {
            self.shared_alloc_count += 1;
            let visible = self.pool.alloc_count + self.shared_alloc_count;
            if visible > self.peak_alloc_count {
                self.peak_alloc_count = visible;
            }
            return unsafe { &mut *self.outbox_ptr }.alloc(obj);
        }

        // Legacy: shared allocator routing for yielding child fibers.
        // Will be removed once outbox escape-context is fully wired.
        if !self.shared_alloc.is_null() {
            self.shared_alloc_count += 1;
            let visible = self.pool.alloc_count + self.shared_alloc_count;
            if visible > self.peak_alloc_count {
                self.peak_alloc_count = visible;
            }
            return unsafe { &mut *self.shared_alloc }.alloc(obj);
        }

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
        // Incref all heap children so they're protected from
        // release_refcounted when the parent binding is decref'd.
        if let Some(ptr) = v.as_heap_ptr() {
            let typed = ptr as *const HeapObject;
            let obj_ref = unsafe { &*typed };
            let mut children = Vec::new();
            Self::collect_heap_children(obj_ref, &mut children);
            for child in children {
                self.incref_value(child);
            }
        }
        v
    }

    /// Copy `items` into the current allocator's arena and return an
    /// `InlineSlice` pointing to them. Used by immutable collection
    /// constructors to store variable-length data inline.
    ///
    /// Routing mirrors `alloc()`: outbox → shared allocator → custom
    /// allocator → private pool. The slice shares the lifetime of
    /// adjacent `alloc()` calls.
    pub fn alloc_inline_slice<T: Copy + 'static>(
        &mut self,
        items: &[T],
    ) -> crate::value::inline_slice::InlineSlice<T> {
        if items.is_empty() {
            return crate::value::inline_slice::InlineSlice::empty();
        }
        // Outbox routing.
        if self.outbox_active && !self.outbox_ptr.is_null() {
            return unsafe { &mut *self.outbox_ptr }.alloc_inline_slice(items);
        }
        // Shared allocator routing (yielding child fibers).
        if !self.shared_alloc.is_null() {
            return unsafe { &mut *self.shared_alloc }.alloc_inline_slice(items);
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
            self.shared_alloc_count,
            Some(self.pool.mark().arena_mark),
        )
    }

    /// Release allocations back to a mark: free rc=0 objects, keep
    /// rc>0 pinned.
    ///
    /// Called by `pop_scope_mark_and_release()` (RegionExit). Uses
    /// release_refcounted so values pinned by push/put incref or
    /// StoreLocalRefcounted survive scope exit.
    pub fn release(&mut self, mark: ArenaMark) {
        if Self::trace_rc() {
            eprintln!("[trace:rc] release mark={}", mark.position());
        }
        self.release_refcounted(mark);
    }

    /// Refcount-aware release: free objects with refcount == 0, skip
    /// pinned objects (refcount > 0).
    ///
    /// Used by scope marks in while loops with outward mutations, where
    /// escape analysis cannot prove all values are dead but refcounting
    /// tracks which values are pinned by mutable collections/bindings.
    pub fn release_refcounted(&mut self, mark: ArenaMark) {
        use super::fiberheap::slab::ALLOC_NIL;

        // Collect scope allocs by walking the linked list from the mark
        // tail to the current tail.
        let start = if mark.alloc_list_tail() == ALLOC_NIL {
            self.pool.alloc_head
        } else {
            self.pool.slab.alloc_next[mark.alloc_list_tail() as usize]
        };

        let mut scope_ptrs: Vec<*mut HeapObject> = Vec::new();
        let mut scope_flats: Vec<u32> = Vec::new();
        {
            let mut cur = start;
            while cur != ALLOC_NIL {
                let next = self.pool.slab.alloc_next[cur as usize];
                let ptr = self.pool.slab.flat_to_ptr(cur as usize);
                scope_ptrs.push(ptr);
                scope_flats.push(cur);
                cur = next;
            }
        }

        // Phase 1: Propagate protection from pinned objects (rc > 0)
        // to their transitive children. This uses temporary increfs so
        // that children reachable from surviving objects are not freed.
        let mut worklist: Vec<*mut HeapObject> = scope_ptrs
            .iter()
            .filter(|&&ptr| self.pool.refcount(ptr as *const HeapObject) > 0)
            .copied()
            .collect();
        while let Some(ptr) = worklist.pop() {
            let obj = unsafe { &*ptr };
            let mut children = Vec::new();
            Self::collect_heap_children(obj, &mut children);
            for child_val in children {
                if let Some(child_ptr) = child_val.as_heap_ptr() {
                    if self.pool.slab_owns(child_ptr) {
                        let child_typed = child_ptr as *mut HeapObject;
                        if self.pool.refcount(child_typed as *const HeapObject) == 0 {
                            self.pool.incref(child_typed as *const HeapObject);
                            worklist.push(child_typed);
                        }
                    }
                }
            }
        }

        // Phase 2: Free unprotected objects (rc still == 0 after phase 1).
        // Run dtors in reverse order for refcount-0 objects.
        for i in (mark.dtor_len()..self.pool.dtors.len()).rev() {
            let ptr = self.pool.dtors[i];
            if self.pool.refcount(ptr as *const HeapObject) == 0 {
                unsafe { std::ptr::drop_in_place(ptr) };
            }
        }
        // Compact dtors: keep pinned, remove dead.
        let mut kept = mark.dtor_len();
        for i in mark.dtor_len()..self.pool.dtors.len() {
            let ptr = self.pool.dtors[i];
            if self.pool.refcount(ptr as *const HeapObject) > 0 {
                self.pool.dtors[kept] = ptr;
                kept += 1;
            }
        }
        self.pool.dtors.truncate(kept);

        // Dealloc refcount-0 slab slots, unlink from alloc list, keep pinned.
        for (i, &ptr) in scope_ptrs.iter().enumerate() {
            let flat = scope_flats[i];
            if self.pool.refcount(ptr as *const HeapObject) == 0 {
                self.pool.unlink_alloc(flat);
                unsafe { self.pool.dealloc_slot(ptr) };
            }
        }

        // NOTE: bump arena is NOT rewound here. Even when all slab objects
        // in the scope are rc=0 and freed, their InlineSlice data in the
        // bump arena may still be referenced by Values that escaped the
        // scope (return values, values pushed to outer collections, etc.).
        // The escaped Value's HeapObject was freed from the slab but the
        // Value copy (16-byte tag+pointer) still exists outside the scope,
        // pointing to InlineSlice data in this bump region. Rewinding
        // would corrupt those strings/arrays.
        //
        // Bump arena reclamation requires region inference: the compiler
        // must prove that no InlineSlice pointer into the region escapes.

        // Dealloc custom-allocated objects from the exiting scope.
        if let Some(state) = self.custom_alloc_stack.last_mut() {
            let start = mark.custom_ptrs_len();
            for &(ptr, size, align) in state.custom_ptrs[start..].iter().rev() {
                state.allocator.inner.dealloc(ptr, size, align);
            }
            state.custom_ptrs.truncate(start);
        }

        self.pool.alloc_count = mark.position();
        self.shared_alloc_count = mark.shared_alloc_count();
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
        self.shared_alloc_count = mark.shared_alloc_count();
    }

    /// Push a scope mark onto the scope stack (called by `RegionEnter`).
    ///
    /// Records the current slab position so that `pop_scope_mark_and_release`
    /// can run destructors and deallocate slab slots for objects allocated
    /// within the scope. When a shared allocator is active (child fiber),
    /// also pushes a mark on the shared allocator.
    ///
    /// Also creates a parallel `RuntimeRegion` for future per-region RC.
    /// Currently unused for allocation routing — all allocations still go
    /// through `pool`. The region is tracked for lifecycle correctness.
    pub fn push_scope_mark(&mut self) {
        if !self.shared_alloc.is_null() {
            unsafe { &mut *self.shared_alloc }.push_mark();
        }
        #[cfg(debug_assertions)]
        {
            // Verify no duplicates exist in dtors at scope entry time.
            // If this fires, a duplicate was introduced within the PARENT
            // scope (before this RegionEnter).
            use std::collections::HashSet;
            let mut seen: HashSet<*mut HeapObject> = HashSet::new();
            for &ptr in &self.pool.dtors {
                if !ptr.is_null() {
                    assert!(
                        seen.insert(ptr),
                        "push_scope_mark: duplicate slab slot {:?} in dtors at scope entry. \
                         dtors.len={}",
                        ptr,
                        self.pool.dtors.len()
                    );
                }
            }
        }
        self.scope_marks.push(self.mark());
        self.scope_enters += 1;
    }

    /// Discard the top scope mark without releasing any objects.
    /// Used by the tail-call trampoline on normal return: the return
    /// value may reference objects allocated in this iteration.
    pub fn discard_scope_mark(&mut self) {
        self.scope_marks.pop();
    }

    /// Pop the top scope mark and release objects allocated since it
    /// was pushed (called by `RegionExit`).
    ///
    /// Runs destructors for objects allocated within the scope, then
    /// deallocates their slab slots back to the free list. When a shared
    /// allocator is active, also pops its mark and releases shared objects.
    ///
    /// Panics (debug) if the scope stack is empty.
    pub fn pop_scope_mark_and_release(&mut self) {
        if !self.shared_alloc.is_null() {
            unsafe { &mut *self.shared_alloc }.pop_mark_and_release();
        }
        let mark = self
            .scope_marks
            .pop()
            .expect("RegionExit without matching RegionEnter");
        let dtors_before = self.pool.dtors.len();
        self.release(mark);
        self.scope_dtors_run += dtors_before - self.pool.dtors.len();
    }

    /// Rotate loop scope marks for double-buffered deallocation.
    ///
    /// The scope mark stack has two marks: [prev, curr]. This operation:
    /// 1. Pops curr (saves it)
    /// 2. Pops prev and releases (frees the iteration-before-last's allocs)
    /// 3. Pushes saved curr as new prev
    /// 4. Pushes a fresh mark as new curr
    ///
    /// This ensures that values from the PREVIOUS iteration survive into
    /// the CURRENT iteration (recur args, loop params), and only the
    /// iteration-before-last's allocs are freed.
    pub fn rotate_scope_marks(&mut self) {
        if self.scope_marks.len() < 2 {
            return; // guard: no-op if marks are missing
        }
        if !self.shared_alloc.is_null() {
            unsafe { &mut *self.shared_alloc }.rotate_marks();
        }
        let curr = self.scope_marks.pop().unwrap();
        let dtors_before = self.pool.dtors.len();
        let prev = self
            .scope_marks
            .pop()
            .expect("RegionRotate: missing previous scope mark");
        // With universal incref, use release_refcounted: free rc=0 objects,
        // keep rc>0 pinned (protected by push/put incref or binding incref).
        // DecrefLocal before rotation brings dead bindings to rc=0.
        self.release_refcounted(prev);
        self.scope_dtors_run += dtors_before - self.pool.dtors.len();
        self.scope_marks.push(curr);
        self.scope_marks.push(self.mark());
        self.scope_enters += 1;
    }

    /// Pop two scope marks and release only the range between them.
    ///
    /// Used by `RegionExitCall`: mark2 (top) is the barrier pushed
    /// after arg evaluation; mark1 (below) is the region start.
    /// Objects in [mark1..mark2) (arg temporaries) are freed.
    /// Objects after mark2 (callee's allocations) are preserved.
    ///
    /// Panics if fewer than two marks are on the stack.
    pub fn pop_call_scope_marks_and_release(&mut self) {
        use super::fiberheap::slab::ALLOC_NIL;

        let mark2 = self
            .scope_marks
            .pop()
            .expect("RegionExitCall: missing barrier mark");
        let mark1 = self
            .scope_marks
            .pop()
            .expect("RegionExitCall: missing region mark");

        // Run dtors in reverse for objects allocated between mark1 and mark2.
        let mut dtors_freed = 0;
        for i in (mark1.dtor_len()..mark2.dtor_len()).rev() {
            let ptr = self.pool.dtors[i];
            if !ptr.is_null() {
                unsafe { std::ptr::drop_in_place(ptr) };
                dtors_freed += 1;
            }
        }
        self.pool.dtors.drain(mark1.dtor_len()..mark2.dtor_len());
        self.scope_dtors_run += dtors_freed;

        // Walk the linked list from mark1.tail.next to mark2.tail,
        // unlinking and deallocating each node in the range.
        let range_start = if mark1.alloc_list_tail() == ALLOC_NIL {
            self.pool.alloc_head
        } else {
            self.pool.slab.alloc_next[mark1.alloc_list_tail() as usize]
        };
        // We need to stop after mark2.alloc_list_tail().
        let range_end_next = if mark2.alloc_list_tail() == ALLOC_NIL {
            ALLOC_NIL
        } else {
            self.pool.slab.alloc_next[mark2.alloc_list_tail() as usize]
        };

        let mut cur = range_start;
        while cur != ALLOC_NIL && cur != range_end_next {
            let next = self.pool.slab.alloc_next[cur as usize];
            let ptr = self.pool.slab.flat_to_ptr(cur as usize);
            // Clear links before dealloc.
            self.pool.slab.alloc_prev[cur as usize] = ALLOC_NIL;
            self.pool.slab.alloc_next[cur as usize] = ALLOC_NIL;
            unsafe { self.pool.dealloc_slot(ptr) };
            cur = next;
        }

        // Stitch the list: connect mark1.tail to mark2.tail.next (range_end_next).
        if mark1.alloc_list_tail() != ALLOC_NIL {
            self.pool.slab.alloc_next[mark1.alloc_list_tail() as usize] = range_end_next;
        } else {
            self.pool.alloc_head = range_end_next;
        }
        if range_end_next != ALLOC_NIL {
            self.pool.slab.alloc_prev[range_end_next as usize] = mark1.alloc_list_tail();
        } else {
            self.pool.alloc_tail = mark1.alloc_list_tail();
        }

        self.pool.alloc_count -= mark2.position() - mark1.position();
    }

    /// Private heap object count (used by mark/release scoping).
    pub fn len(&self) -> usize {
        self.pool.alloc_count
    }

    /// Total allocations visible to this fiber, including objects routed
    /// to the parent's shared allocator.  Used by arena/count.
    pub fn visible_len(&self) -> usize {
        self.pool.alloc_count + self.shared_alloc_count
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

    /// Bytes committed by local slab plus shared allocator (if active).
    pub fn allocated_bytes(&self) -> usize {
        let local = self.pool.allocated_bytes();
        let shared = if self.shared_alloc.is_null() {
            0
        } else {
            unsafe { (*self.shared_alloc).allocated_bytes() }
        };
        local + shared
    }

    /// Number of `RegionEnter` instructions executed (scope regions entered).
    pub fn scope_enters(&self) -> usize {
        self.scope_enters
    }

    /// Number of destructors run by `RegionExit` (objects freed at scope exit).
    pub fn scope_dtors_run(&self) -> usize {
        self.scope_dtors_run
    }

    /// Peak number of objects allocated (high-water mark).
    pub fn peak_alloc_count(&self) -> usize {
        self.peak_alloc_count
    }

    /// Number of active scope marks (scope depth).
    pub(crate) fn scope_depth(&self) -> usize {
        self.scope_marks.len()
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

    /// Number of owned shared allocators.
    pub(crate) fn shared_count(&self) -> usize {
        self.owned_shared.len()
    }

    /// Reset peak to current visible count (local + shared). Returns previous peak.
    pub fn reset_peak(&mut self) -> usize {
        let prev = self.peak_alloc_count;
        self.peak_alloc_count = self.pool.alloc_count + self.shared_alloc_count;
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

    /// Create a new shared allocator on this fiber's `owned_shared` list.
    ///
    /// Returns a raw pointer to the shared allocator. The `Box` in the Vec
    /// provides pointer stability — the pointer remains valid even if the
    /// Vec grows (Box stores the data on the heap, Vec stores the Box pointer).
    #[allow(dead_code)]
    pub(crate) fn create_shared_allocator(
        &mut self,
    ) -> *mut crate::value::shared_alloc::SharedAllocator {
        let mut sa = Box::new(crate::value::shared_alloc::SharedAllocator::new());
        let ptr = &mut *sa as *mut crate::value::shared_alloc::SharedAllocator;
        self.owned_shared.push(sa);
        ptr
    }

    /// Current shared allocator pointer. Returns null if none is set.
    #[cfg(test)]
    pub(crate) fn shared_alloc(&self) -> *mut crate::value::shared_alloc::SharedAllocator {
        self.shared_alloc
    }

    /// Check whether a shared allocator is active for this fiber.
    pub fn has_shared_alloc(&self) -> bool {
        !self.shared_alloc.is_null()
    }

    /// Set the shared allocator pointer for this fiber.
    /// When non-null, `alloc()` routes all allocations to the shared allocator.
    #[cfg(test)]
    pub(crate) fn set_shared_alloc(
        &mut self,
        ptr: *mut crate::value::shared_alloc::SharedAllocator,
    ) {
        self.shared_alloc = ptr;
    }

    /// Clear the shared allocator pointer (set to null).
    /// Called on swap-back when the child is no longer executing.
    pub fn clear_shared_alloc(&mut self) {
        self.shared_alloc = std::ptr::null_mut();
    }

    // ── Outbox management ──────────────────────────────────────────

    /// Install a fresh outbox pool. Called by the parent before each
    /// child execution. Previous outboxes are preserved so the parent
    /// can still read values from earlier yields. All outboxes are freed
    /// in bulk on parent fiber death (O(1) via clear/drop).
    ///
    /// MUST be called on the PARENT's FiberHeap, not the child's. The parent
    /// owns the outbox pool; the child receives a borrowed raw pointer via
    /// `set_outbox_borrow()`.
    pub(crate) fn install_outbox(&mut self, pool: SlabPool) {
        self.shared_alloc_count = 0;
        self.owned_outboxes.push(Box::new(pool));
        self.outbox_ptr = &mut **self.owned_outboxes.last_mut().unwrap();
        self.outbox_active = false;
    }

    /// Set a borrowed outbox pointer. Called on the CHILD's FiberHeap with
    /// a raw pointer into the parent's `owned_outboxes`. The child allocates
    /// through this pointer but never owns or tears down the pool.
    pub(crate) fn set_outbox_borrow(&mut self, ptr: *mut SlabPool) {
        self.outbox_ptr = ptr;
        self.outbox_active = false;
    }

    /// Get the raw pointer to the current outbox pool.
    /// Used to pass the pointer to a child fiber via `set_outbox_borrow()`.
    pub(crate) fn outbox_ptr(&self) -> *mut SlabPool {
        self.outbox_ptr
    }

    /// Check whether an outbox is active (pointer is non-null).
    pub fn has_outbox(&self) -> bool {
        !self.outbox_ptr.is_null()
    }

    /// Enter outbox routing context. Allocations go to outbox until
    /// `outbox_exit()` is called. No-op if no outbox is installed.
    pub fn outbox_enter(&mut self) {
        if !self.outbox_ptr.is_null() {
            self.outbox_active = true;
        }
    }

    /// Exit outbox routing context. Allocations revert to private heap.
    pub fn outbox_exit(&mut self) {
        self.outbox_active = false;
    }

    /// Check if a heap value's pointer is in this heap's private pool
    /// (not in any outbox). Used by the yield/return safety net: if a
    /// value is in the private pool, it must be deep-copied to the
    /// outbox before yield (otherwise the parent reads a dangling pointer).
    pub fn value_in_private_pool(&self, value: Value) -> bool {
        if !value.is_heap() {
            return false;
        }
        let ptr = match value.as_heap_ptr() {
            Some(p) => p,
            None => return false,
        };
        // Check if the pointer is in any outbox (current or old).
        if !self.outbox_ptr.is_null() && unsafe { &*self.outbox_ptr }.owns(ptr) {
            return false;
        }
        for ob in &self.owned_outboxes {
            if ob.owns(ptr) {
                return false;
            }
        }
        self.pool.owns(ptr)
    }

    /// Check if a value is owned by this fiber (in private pool, outbox,
    /// or old outboxes). Returns false for values from foreign heaps.
    pub fn value_owned_by_fiber(&self, value: Value) -> bool {
        let ptr = match value.as_heap_ptr() {
            Some(p) => p,
            None => return false,
        };
        if self.pool.owns(ptr) {
            return true;
        }
        if !self.outbox_ptr.is_null() && unsafe { &*self.outbox_ptr }.owns(ptr) {
            return true;
        }
        for ob in &self.owned_outboxes {
            if ob.owns(ptr) {
                return true;
            }
        }
        false
    }

    /// Deep-copy a value from the private pool to the outbox.
    /// Returns the new value (pointing into the outbox). If the value
    /// is immediate or already in the outbox, returns it unchanged.
    ///
    /// Recursively copies cons cells so the entire reachable graph is
    /// relocated. Other compound types (struct, array, closure) are
    /// rebuilt with new slab slots; their inner Rust heap allocations
    /// (Vec, BTreeMap, Rc) are reference-counted and survive independently
    /// of the slab slot.
    pub fn deep_copy_to_outbox(&mut self, value: Value) -> Value {
        if !value.is_heap() {
            return value;
        }
        let ptr = match value.as_heap_ptr() {
            Some(p) => p,
            None => return value,
        };
        // If outbox exists and owns it, recurse into children to relocate
        // any nested private-pool references (e.g. yield [:send target msg]
        // where the outer array is in the outbox but msg is in the private pool).
        if !self.outbox_ptr.is_null() && unsafe { &*self.outbox_ptr }.owns(ptr) {
            let heap_obj = unsafe { &*(ptr as *const HeapObject) };
            if self.outbox_value_has_private_children(heap_obj) {
                return self.rebuild_in_outbox(heap_obj);
            }
            return value;
        }
        // If not in private pool either, return as-is (constant pool, etc.).
        if !self.pool.owns(ptr) {
            return value;
        }
        // Read the HeapObject and rebuild it in the outbox.
        let heap_obj = unsafe { &*(ptr as *const HeapObject) };
        self.rebuild_in_outbox(heap_obj)
    }

    /// Deep-copy a value out of this fiber's outbox/private-pool into the
    /// current (parent) heap.  Called by `fiber/value` so the parent can
    /// store the value safely across subsequent resumes (which tear down
    /// the outbox).
    ///
    /// Values that are not owned by this fiber (immediates, constant pool,
    /// other heaps) are returned as-is.
    pub fn deep_copy_out_of_outbox(&self, value: Value) -> Value {
        if !value.is_heap() {
            return value;
        }
        let ptr = match value.as_heap_ptr() {
            Some(p) => p,
            None => return value,
        };
        let owned_by_child = self.pool.owns(ptr)
            || (!self.outbox_ptr.is_null() && unsafe { &*self.outbox_ptr }.owns(ptr));
        if !owned_by_child {
            return value;
        }
        let heap_obj = unsafe { &*(ptr as *const HeapObject) };
        self.rebuild_on_current_heap(heap_obj)
    }

    /// Rebuild a HeapObject on the current (parent) heap.
    /// Recursively copies children that belong to this fiber.
    fn rebuild_on_current_heap(&self, obj: &HeapObject) -> Value {
        // Snapshot children that need recursive copying before we
        // borrow the parent heap for allocation.
        let new_obj = match obj {
            HeapObject::Pair(c) => {
                let head = self.deep_copy_out_of_outbox(c.first);
                let tail = self.deep_copy_out_of_outbox(c.rest);
                HeapObject::Pair(crate::value::heap::Pair {
                    first: head,
                    rest: tail,
                    traits: c.traits,
                })
            }
            HeapObject::LArray { elements, traits } => {
                let elems: Vec<Value> = elements
                    .as_slice()
                    .iter()
                    .map(|v| self.deep_copy_out_of_outbox(*v))
                    .collect();
                // Allocate inline slice on parent heap inside the alloc call below.
                // For now, use a temporary; the pool.alloc will intern it.
                return crate::value::fiberheap::routing::with_current_heap_mut(|heap| {
                    let slice = heap.pool.alloc_inline_slice::<Value>(&elems);
                    heap.pool.alloc(HeapObject::LArray {
                        elements: slice,
                        traits: *traits,
                    })
                })
                .expect("rebuild_on_current_heap: no current heap");
            }
            HeapObject::LStruct { data, traits } => {
                let entries: Vec<_> = data
                    .iter()
                    .map(|(k, v)| (k.clone(), self.deep_copy_out_of_outbox(*v)))
                    .collect();
                HeapObject::LStruct {
                    data: entries,
                    traits: *traits,
                }
            }
            HeapObject::LSet { data, traits } => {
                let elems: Vec<Value> = data
                    .as_slice()
                    .iter()
                    .map(|v| self.deep_copy_out_of_outbox(*v))
                    .collect();
                return crate::value::fiberheap::routing::with_current_heap_mut(|heap| {
                    let slice = heap.pool.alloc_inline_slice::<Value>(&elems);
                    heap.pool.alloc(HeapObject::LSet {
                        data: slice,
                        traits: *traits,
                    })
                })
                .expect("rebuild_on_current_heap: no current heap");
            }
            // Rc-backed types: clone the Rc to share the backing store.
            HeapObject::LBox { cell, traits } => HeapObject::LBox {
                cell: cell.clone(),
                traits: *traits,
            },
            HeapObject::CaptureCell { cell, traits } => HeapObject::CaptureCell {
                cell: cell.clone(),
                traits: *traits,
            },
            HeapObject::Closure { closure, traits } => HeapObject::Closure {
                closure: closure.clone(),
                traits: *traits,
            },
            HeapObject::LArrayMut { data, traits } => HeapObject::LArrayMut {
                data: data.clone(),
                traits: *traits,
            },
            HeapObject::LStructMut { data, traits } => HeapObject::LStructMut {
                data: data.clone(),
                traits: *traits,
            },
            HeapObject::LStringMut { data, traits } => HeapObject::LStringMut {
                data: data.clone(),
                traits: *traits,
            },
            HeapObject::LBytesMut { data, traits } => HeapObject::LBytesMut {
                data: data.clone(),
                traits: *traits,
            },
            HeapObject::LSetMut { data, traits } => HeapObject::LSetMut {
                data: data.clone(),
                traits: *traits,
            },
            // Inline data types: copy the inline slice/value.
            HeapObject::LString { s, traits } => HeapObject::LString {
                s: *s,
                traits: *traits,
            },
            HeapObject::LBytes { data, traits } => HeapObject::LBytes {
                data: *data,
                traits: *traits,
            },
            HeapObject::Float(f) => HeapObject::Float(*f),
            HeapObject::NativeFn(f) => HeapObject::NativeFn(f),
            HeapObject::Parameter {
                id,
                default,
                traits,
            } => HeapObject::Parameter {
                id: *id,
                default: *default,
                traits: *traits,
            },
            // External, Fiber, LibHandle, etc. — Rc-backed, survive
            // independently of the slab.  Return the original value.
            _ => {
                let tag = obj.value_tag();
                return Value::from_heap_ptr(obj as *const HeapObject as *const (), tag);
            }
        };
        crate::value::fiberheap::routing::with_current_heap_mut(|heap| heap.pool.alloc(new_obj))
            .expect("rebuild_on_current_heap: no current heap")
    }

    /// Check whether a heap object (already in the outbox) contains any
    /// child values that live in the private pool.
    fn outbox_value_has_private_children(&self, obj: &HeapObject) -> bool {
        let check = |v: &Value| -> bool {
            if let Some(p) = v.as_heap_ptr() {
                self.pool.owns(p)
            } else {
                false
            }
        };
        match obj {
            HeapObject::Pair(c) => check(&c.first) || check(&c.rest),
            HeapObject::LArray { elements, .. } => elements.as_slice().iter().any(check),
            HeapObject::LStruct { data, .. } => data.iter().any(|(_, v)| check(v)),
            HeapObject::LBox { cell, .. } => check(&cell.borrow()),
            HeapObject::LSet { data, .. } => data.as_slice().iter().any(check),
            _ => false,
        }
    }

    /// Allocate a copy of `obj` into the outbox. For Pair, recursively
    /// copies sub-values that are in the private pool.
    ///
    /// Panics if no outbox is installed. Callers must check `has_outbox()`
    /// before calling. When no outbox exists (silent fibers), private pool
    /// values are returned as-is — they live as long as the FiberHandle.
    fn rebuild_in_outbox(&mut self, obj: &HeapObject) -> Value {
        assert!(!self.outbox_ptr.is_null(), "rebuild_in_outbox: no outbox");
        let outbox = self.outbox_ptr;
        match obj {
            HeapObject::Pair(c) => {
                let head = c.first;
                let tail = c.rest;
                let traits = c.traits;
                let head = self.deep_copy_to_outbox(head);
                let tail = self.deep_copy_to_outbox(tail);
                let new_obj = HeapObject::Pair(crate::value::heap::Pair {
                    first: head,
                    rest: tail,
                    traits,
                });
                unsafe { &mut *outbox }.alloc(new_obj)
            }
            HeapObject::LString { s, traits } => {
                // Deep-copy string bytes into the outbox's bump arena.
                // The InlineSlice from the source may point to the child's
                // bump arena, which is freed when the child's FiberHeap drops.
                let bytes = s.as_slice();
                let new_slice = if bytes.is_empty() {
                    crate::value::inline_slice::InlineSlice::empty()
                } else {
                    unsafe { &mut *outbox }.alloc_inline_slice::<u8>(bytes)
                };
                unsafe { &mut *outbox }.alloc(HeapObject::LString {
                    s: new_slice,
                    traits: *traits,
                })
            }
            HeapObject::LStruct { data, traits } => {
                let entries: Vec<_> = data.iter().map(|(k, v)| (k.clone(), *v)).collect();
                let traits = *traits;
                let entries: Vec<_> = entries
                    .into_iter()
                    .map(|(k, v)| (k, self.deep_copy_to_outbox(v)))
                    .collect();
                unsafe { &mut *outbox }.alloc(HeapObject::LStruct {
                    data: entries,
                    traits,
                })
            }
            HeapObject::LArray { elements, traits } => {
                let elems: Vec<Value> = elements.as_slice().to_vec();
                let traits = *traits;
                let elems: Vec<Value> = elems
                    .into_iter()
                    .map(|v| self.deep_copy_to_outbox(v))
                    .collect();
                let ob = unsafe { &mut *outbox };
                let slice = ob.alloc_inline_slice::<Value>(&elems);
                ob.alloc(HeapObject::LArray {
                    elements: slice,
                    traits,
                })
            }
            HeapObject::LBox { cell, traits } => unsafe { &mut *outbox }.alloc(HeapObject::LBox {
                cell: cell.clone(),
                traits: *traits,
            }),
            HeapObject::CaptureCell { cell, traits } => {
                unsafe { &mut *outbox }.alloc(HeapObject::CaptureCell {
                    cell: cell.clone(),
                    traits: *traits,
                })
            }
            HeapObject::Float(f) => unsafe { &mut *outbox }.alloc(HeapObject::Float(*f)),
            HeapObject::Closure { closure, traits } => {
                unsafe { &mut *outbox }.alloc(HeapObject::Closure {
                    closure: closure.clone(),
                    traits: *traits,
                })
            }
            HeapObject::LArrayMut { data, traits } => {
                unsafe { &mut *outbox }.alloc(HeapObject::LArrayMut {
                    data: data.clone(),
                    traits: *traits,
                })
            }
            HeapObject::LStructMut { data, traits } => {
                unsafe { &mut *outbox }.alloc(HeapObject::LStructMut {
                    data: data.clone(),
                    traits: *traits,
                })
            }
            HeapObject::LStringMut { data, traits } => {
                unsafe { &mut *outbox }.alloc(HeapObject::LStringMut {
                    data: data.clone(),
                    traits: *traits,
                })
            }
            HeapObject::LBytes { data, traits } => {
                // Deep-copy bytes into the outbox's bump arena.
                let bytes = data.as_slice();
                let new_slice = if bytes.is_empty() {
                    crate::value::inline_slice::InlineSlice::empty()
                } else {
                    unsafe { &mut *outbox }.alloc_inline_slice::<u8>(bytes)
                };
                unsafe { &mut *outbox }.alloc(HeapObject::LBytes {
                    data: new_slice,
                    traits: *traits,
                })
            }
            HeapObject::LBytesMut { data, traits } => {
                unsafe { &mut *outbox }.alloc(HeapObject::LBytesMut {
                    data: data.clone(),
                    traits: *traits,
                })
            }
            HeapObject::LSet { data, traits } => {
                let elems: Vec<Value> = data.as_slice().to_vec();
                let traits = *traits;
                let elems: Vec<Value> = elems
                    .into_iter()
                    .map(|v| self.deep_copy_to_outbox(v))
                    .collect();
                let ob = unsafe { &mut *outbox };
                let slice = ob.alloc_inline_slice::<Value>(&elems);
                ob.alloc(HeapObject::LSet {
                    data: slice,
                    traits,
                })
            }
            HeapObject::LSetMut { data, traits } => {
                unsafe { &mut *outbox }.alloc(HeapObject::LSetMut {
                    data: data.clone(),
                    traits: *traits,
                })
            }
            HeapObject::NativeFn(f) => unsafe { &mut *outbox }.alloc(HeapObject::NativeFn(f)),
            HeapObject::Parameter {
                id,
                default,
                traits,
            } => unsafe { &mut *outbox }.alloc(HeapObject::Parameter {
                id: *id,
                default: *default,
                traits: *traits,
            }),
            HeapObject::ManagedPointer { addr, traits } => {
                unsafe { &mut *outbox }.alloc(HeapObject::ManagedPointer {
                    addr: std::cell::Cell::new(addr.get()),
                    traits: *traits,
                })
            }
            HeapObject::Fiber { handle, traits } => {
                unsafe { &mut *outbox }.alloc(HeapObject::Fiber {
                    handle: handle.clone(),
                    traits: *traits,
                })
            }
            HeapObject::Syntax { syntax, traits } => {
                unsafe { &mut *outbox }.alloc(HeapObject::Syntax {
                    syntax: syntax.clone(),
                    traits: *traits,
                })
            }
            HeapObject::External { obj, traits } => {
                unsafe { &mut *outbox }.alloc(HeapObject::External {
                    obj: obj.clone(),
                    traits: *traits,
                })
            }
            HeapObject::FFISignature(sig, cif) => {
                #[cfg(feature = "ffi")]
                let new_cif = RefCell::new(cif.borrow().clone());
                #[cfg(not(feature = "ffi"))]
                let new_cif = *cif;
                unsafe { &mut *outbox }.alloc(HeapObject::FFISignature(sig.clone(), new_cif))
            }
            HeapObject::FFIType(t) => unsafe { &mut *outbox }.alloc(HeapObject::FFIType(t.clone())),
            HeapObject::ThreadHandle { handle, traits } => {
                unsafe { &mut *outbox }.alloc(HeapObject::ThreadHandle {
                    handle: handle.clone(),
                    traits: *traits,
                })
            }
            HeapObject::LibHandle(id) => unsafe { &mut *outbox }.alloc(HeapObject::LibHandle(*id)),
        }
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
    ///
    /// Also tears down all owned shared allocators and nulls the
    /// shared_alloc pointer.
    pub fn clear(&mut self) {
        // Tear down owned shared allocators.
        for sa in &mut self.owned_shared {
            sa.teardown();
        }
        self.owned_shared.clear();
        self.shared_alloc = std::ptr::null_mut();

        // Tear down all owned outboxes. Only the parent fiber has non-empty
        // owned_outboxes; the child's is empty (it only held a borrowed pointer).
        for mut ob in self.owned_outboxes.drain(..) {
            ob.teardown();
        }
        self.outbox_ptr = std::ptr::null_mut();
        self.outbox_active = false;

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

        self.scope_marks.clear();
        self.alloc_error = None;
        self.pool.alloc_count = 0;
        self.peak_alloc_count = 0;
        self.scope_enters = 0;
        self.scope_dtors_run = 0;
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

            let outbox_entries: Vec<_> = self
                .owned_outboxes
                .iter()
                .flat_map(|ob| collect_pool_rc_inners(&ob.dtors))
                .collect();

            // Shared allocators also have their own pools.
            let shared_entries: Vec<_> = self
                .owned_shared
                .iter()
                .flat_map(|sa| collect_pool_rc_inners(&sa.pool.dtors))
                .collect();

            let all_entries: Vec<_> = private_entries
                .into_iter()
                .chain(outbox_entries)
                .chain(shared_entries)
                .collect();

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

        // Tear down all owned outboxes. Only the parent fiber has non-empty
        // owned_outboxes; the child's is empty (it only held a borrowed pointer).
        for mut ob in self.owned_outboxes.drain(..) {
            ob.teardown();
        }
        // Tear down owned shared allocators before our slab is dropped.
        for sa in &mut self.owned_shared {
            sa.teardown();
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
