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
//! ## Shared allocator for inter-fiber exchange
//!
//! `FiberHeap` owns zero or more `SharedAllocator`s (in `owned_shared: Vec<Box<SharedAllocator>>`)
//! and has a `shared_alloc: *mut SharedAllocator` pointer for routing.
//!
//! When `shared_alloc` is non-null, `alloc()` routes ALL allocations to the
//! shared allocator instead of the slab. This is set by `with_child_fiber`
//! for yielding child fibers and nulled on swap-back.
//!
//! Ownership model: the parent's FiberHeap owns the `Box<SharedAllocator>`;
//! the child receives a raw pointer. For root→child chains, the child owns it.
//! `Box` provides pointer stability — the raw pointer remains valid even when
//! `owned_shared` grows. Teardown happens on `clear()` or `Drop`.

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
    /// Current outbox pool for yield-bound allocations. Created by the
    /// parent via `install_outbox()` before child execution. Allocations
    /// between `OutboxEnter`/`OutboxExit` go here.
    outbox: Option<Box<SlabPool>>,
    /// Previous outbox pools from earlier yields. Kept alive so the parent
    /// can still read values from previous yields. Freed on fiber death.
    ///
    /// `Box<SlabPool>` is intentional: the outbox is handed off by raw
    /// pointer via `install_outbox`, and boxing stabilizes its address.
    #[allow(clippy::vec_box)]
    old_outboxes: Vec<Box<SlabPool>>,
    /// True when allocations should route to the outbox (between
    /// `OutboxEnter` and `OutboxExit` bytecodes).
    outbox_active: bool,
    /// Append-only list of slab pointers for trampoline rotation.
    /// Unlike `pool.allocs`, this is NOT truncated by scope exits
    /// (RegionExit/RegionExitCall). The trampoline drains it at each
    /// tail-call boundary to snapshot one iteration's allocations.
    pub(crate) rotation_log: Vec<*mut HeapObject>,
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
            outbox: None,
            old_outboxes: Vec::new(),
            outbox_active: false,
            rotation_log: Vec::new(),
        }
    }

    pub fn alloc(&mut self, obj: HeapObject) -> Value {
        // Outbox routing: when outbox is active (between OutboxEnter/OutboxExit),
        // allocations go to the outbox pool for yield-bound values.
        if self.outbox_active {
            if let Some(ref mut outbox) = self.outbox {
                self.shared_alloc_count += 1;
                let visible = self.pool.alloc_count + self.shared_alloc_count;
                if visible > self.peak_alloc_count {
                    self.peak_alloc_count = visible;
                }
                return outbox.alloc(obj);
            }
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
        // Append to rotation log (append-only, not affected by scope exits).
        self.rotation_log.push(self.pool.last_alloc_ptr());
        if self.pool.alloc_count > self.peak_alloc_count {
            self.peak_alloc_count = self.pool.alloc_count;
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
        if self.outbox_active {
            if let Some(ref mut outbox) = self.outbox {
                return outbox.alloc_inline_slice(items);
            }
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
            self.pool.allocs.len(),
            self.shared_alloc_count,
            Some(self.pool.mark().arena_mark),
        )
    }

    /// Release allocations back to a mark: run destructors, dealloc slab
    /// slots to the free list, and truncate tracking vecs.
    ///
    /// Called by `pop_scope_mark_and_release()` (RegionExit), which is
    /// gated by Tofte-Talpin region analysis — only scopes where no
    /// values escape get this call.
    pub fn release(&mut self, mark: ArenaMark) {
        self.pool.run_dtors(mark.dtor_len());
        self.pool.dtors.truncate(mark.dtor_len());

        let n_freed = self.pool.allocs.len() - mark.root_allocs_len();
        for i in (mark.root_allocs_len()..self.pool.allocs.len()).rev() {
            unsafe {
                self.pool.dealloc_slot(self.pool.allocs[i]);
            }
        }
        self.pool.allocs.truncate(mark.root_allocs_len());

        if let Some(state) = self.custom_alloc_stack.last_mut() {
            let start = mark.custom_ptrs_len();
            for &(ptr, size, align) in state.custom_ptrs[start..].iter().rev() {
                state.allocator.inner.dealloc(ptr, size, align);
            }
            state.custom_ptrs.truncate(start);
        }

        // Rewind bump arena for inline data (strings, arrays, bytes).
        // Only rewind when slab slots were actually freed — their inline
        // data is dead. When n_freed == 0, the call-site scope freed no
        // objects but the bump may contain live inline data from the
        // callee's return value.
        if n_freed > 0 {
            if let Some(bm) = mark.bump_mark() {
                self.pool.release_bump_to(bm);
            }
        }

        self.pool.alloc_count = mark.position();
        self.shared_alloc_count = mark.shared_alloc_count();
    }

    /// Refcount-aware release: free objects with refcount == 0, skip
    /// pinned objects (refcount > 0).
    ///
    /// Used by scope marks in while loops with outward mutations, where
    /// escape analysis cannot prove all values are dead but refcounting
    /// tracks which values are pinned by mutable collections/bindings.
    pub fn release_refcounted(&mut self, mark: ArenaMark) {
        // Phase 1: Propagate protection from pinned objects (rc > 0)
        // to their transitive children. This uses temporary increfs so
        // that children reachable from surviving objects are not freed.
        let scope_allocs = &self.pool.allocs[mark.root_allocs_len()..];
        let mut worklist: Vec<*mut HeapObject> = scope_allocs
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

        // Dealloc refcount-0 slab slots, keep pinned.
        let mut allocs_kept = mark.root_allocs_len();
        for i in mark.root_allocs_len()..self.pool.allocs.len() {
            let ptr = self.pool.allocs[i];
            if self.pool.refcount(ptr as *const HeapObject) == 0 {
                unsafe { self.pool.dealloc_slot(ptr) };
            } else {
                self.pool.allocs[allocs_kept] = ptr;
                allocs_kept += 1;
            }
        }
        self.pool.allocs.truncate(allocs_kept);

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
        self.pool.run_dtors(mark.dtor_len());
        self.pool.dtors.truncate(mark.dtor_len());
        self.pool.allocs.truncate(mark.root_allocs_len());

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

    /// Drain the rotation log and return all slab pointers accumulated
    /// since the last drain.  This is append-only and not affected by
    /// scope exits (RegionExit/RegionExitCall).
    pub fn drain_rotation_log(&mut self) -> Vec<*mut HeapObject> {
        std::mem::take(&mut self.rotation_log)
    }

    /// Dealloc a list of slab pointers directly.  Used by the trampoline
    /// to free a previous iteration's snapshot.  The pointers are removed
    /// from the internal allocs list by value (O(n) scan).  Objects whose
    /// refcount is nonzero are skipped — they are pinned by external
    /// references and must not be freed.
    pub fn dealloc_ptrs(&mut self, ptrs: &[*mut HeapObject]) {
        for &ptr in ptrs {
            if self.pool.refcount(ptr as *const _) > 0 {
                continue;
            }
            unsafe { self.pool.dealloc_slot(ptr) }
            // Remove from allocs list so future releases don't double-free.
            if let Some(pos) = self.pool.allocs.iter().position(|&p| p == ptr) {
                self.pool.allocs.swap_remove(pos);
            }
            // Keep alloc_count accurate: one fewer live object.
            self.pool.alloc_count = self.pool.alloc_count.saturating_sub(1);
        }
    }

    /// Push a scope mark onto the scope stack (called by `RegionEnter`).
    ///
    /// Records the current slab position so that `pop_scope_mark_and_release`
    /// can run destructors and deallocate slab slots for objects allocated
    /// within the scope. When a shared allocator is active (child fiber),
    /// also pushes a mark on the shared allocator.
    pub fn push_scope_mark(&mut self) {
        if !self.shared_alloc.is_null() {
            unsafe { &mut *self.shared_alloc }.push_mark();
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
        // Use release_no_dealloc: run dtors and reset alloc_count, but
        // don't free slab slots. Loop iteration values may chain across
        // generations (cons lists, etc.), making slot freeing unsafe.
        // Let-scope RegionExit still uses release() with dealloc.
        self.release_no_dealloc(prev);
        self.scope_dtors_run += dtors_before - self.pool.dtors.len();
        self.scope_marks.push(curr);
        self.scope_marks.push(self.mark());
        self.scope_enters += 1;
    }

    /// Like `rotate_scope_marks` but also deallocates slab slots.
    /// Only safe when no loop param's value references a previous
    /// iteration's alloc (no cons-chain pattern).
    pub fn rotate_scope_marks_dealloc(&mut self) {
        if self.scope_marks.len() < 2 {
            return;
        }
        if !self.shared_alloc.is_null() {
            unsafe { &mut *self.shared_alloc }.rotate_marks();
        }
        let curr = self.scope_marks.pop().unwrap();
        let dtors_before = self.pool.dtors.len();
        let prev = self
            .scope_marks
            .pop()
            .expect("RegionRotateDealloc: missing previous scope mark");
        self.release(prev);
        self.scope_dtors_run += dtors_before - self.pool.dtors.len();
        self.scope_marks.push(curr);
        self.scope_marks.push(self.mark());
        self.scope_enters += 1;
    }

    /// Refcount-aware rotation: like `rotate_scope_marks` but uses
    /// `release_refcounted` to skip pinned values (refcount > 0).
    /// Used by while loops with outward mutations that pass refcount
    /// eligibility but not escape-analysis eligibility.
    pub fn rotate_scope_marks_refcounted(&mut self) {
        if self.scope_marks.len() < 2 {
            return;
        }
        if !self.shared_alloc.is_null() {
            unsafe { &mut *self.shared_alloc }.rotate_marks();
        }
        let curr = self.scope_marks.pop().unwrap();
        let dtors_before = self.pool.dtors.len();
        let prev = self
            .scope_marks
            .pop()
            .expect("RegionRotateRefcounted: missing previous scope mark");
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
        let mark2 = self
            .scope_marks
            .pop()
            .expect("RegionExitCall: missing barrier mark");
        let mark1 = self
            .scope_marks
            .pop()
            .expect("RegionExitCall: missing region mark");

        // Run dtors in reverse for objects allocated between mark1 and mark2.
        for i in (mark1.dtor_len()..mark2.dtor_len()).rev() {
            unsafe {
                std::ptr::drop_in_place(self.pool.dtors[i]);
            }
        }
        let dtors_freed = mark2.dtor_len() - mark1.dtor_len();
        self.pool.dtors.drain(mark1.dtor_len()..mark2.dtor_len());
        self.scope_dtors_run += dtors_freed;

        // Dealloc slab slots for the range, then drain the entries.
        for i in (mark1.root_allocs_len()..mark2.root_allocs_len()).rev() {
            unsafe {
                self.pool.dealloc_slot(self.pool.allocs[i]);
            }
        }
        self.pool
            .allocs
            .drain(mark1.root_allocs_len()..mark2.root_allocs_len());

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
        self.pool.allocs.len()
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
    /// in bulk on fiber death (O(1) via clear/drop).
    pub(crate) fn install_outbox(&mut self, pool: SlabPool) {
        if let Some(old) = self.outbox.take() {
            self.old_outboxes.push(old);
        }
        self.shared_alloc_count = 0;
        self.outbox = Some(Box::new(pool));
        self.outbox_active = false;
    }

    /// Check whether an outbox is installed.
    pub fn has_outbox(&self) -> bool {
        self.outbox.is_some()
    }

    /// Enter outbox routing context. Allocations go to outbox until
    /// `outbox_exit()` is called. No-op if no outbox is installed.
    pub fn outbox_enter(&mut self) {
        if self.outbox.is_some() {
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
        if let Some(ref outbox) = self.outbox {
            if outbox.owns(ptr) {
                return false;
            }
        }
        for ob in &self.old_outboxes {
            if ob.owns(ptr) {
                return false;
            }
        }
        self.pool.owns(ptr)
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
        // If outbox exists and owns it, already safe.
        if self.outbox.as_ref().is_some_and(|ob| ob.owns(ptr)) {
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

    /// Allocate a copy of `obj` into the outbox. For Pair, recursively
    /// copies sub-values that are in the private pool.
    ///
    /// Panics if no outbox is installed. Callers must check `has_outbox()`
    /// before calling. When no outbox exists (silent fibers), private pool
    /// values are returned as-is — they live as long as the FiberHandle.
    fn rebuild_in_outbox(&mut self, obj: &HeapObject) -> Value {
        let outbox = self.outbox.as_mut().expect("rebuild_in_outbox: no outbox");
        match obj {
            HeapObject::Pair(c) => {
                let head = c.first;
                let tail = c.rest;
                let traits = c.traits;
                // Drop the borrow on self before recursing.
                let head = self.deep_copy_to_outbox(head);
                let tail = self.deep_copy_to_outbox(tail);
                let new_obj = HeapObject::Pair(crate::value::heap::Pair {
                    first: head,
                    rest: tail,
                    traits,
                });
                self.outbox.as_mut().unwrap().alloc(new_obj)
            }
            HeapObject::LString { s, traits } => {
                let new_obj = HeapObject::LString {
                    s: *s,
                    traits: *traits,
                };
                outbox.alloc(new_obj)
            }
            HeapObject::LStruct { data, traits } => {
                let entries: Vec<_> = data.iter().map(|(k, v)| (k.clone(), *v)).collect();
                let traits = *traits;
                let entries: Vec<_> = entries
                    .into_iter()
                    .map(|(k, v)| (k, self.deep_copy_to_outbox(v)))
                    .collect();
                self.outbox.as_mut().unwrap().alloc(HeapObject::LStruct {
                    data: entries,
                    traits,
                })
            }
            HeapObject::LArray { elements, traits } => {
                // Snapshot elements so we can drop the borrow on `self` before
                // recursing (deep_copy_to_outbox needs &mut self).
                let elems: Vec<Value> = elements.as_slice().to_vec();
                let traits = *traits;
                let elems: Vec<Value> = elems
                    .into_iter()
                    .map(|v| self.deep_copy_to_outbox(v))
                    .collect();
                let outbox = self.outbox.as_mut().unwrap();
                let slice = outbox.alloc_inline_slice::<Value>(&elems);
                outbox.alloc(HeapObject::LArray {
                    elements: slice,
                    traits,
                })
            }
            HeapObject::LBox { cell, traits } => outbox.alloc(HeapObject::LBox {
                // Share the backing cell.
                cell: cell.clone(),
                traits: *traits,
            }),
            HeapObject::CaptureCell { cell, traits } => outbox.alloc(HeapObject::CaptureCell {
                // Share the backing cell — mutations in a captured lambda
                // are visible to every fiber that holds the capture cell.
                cell: cell.clone(),
                traits: *traits,
            }),
            HeapObject::Float(f) => outbox.alloc(HeapObject::Float(*f)),
            HeapObject::Closure { closure, traits } => outbox.alloc(HeapObject::Closure {
                closure: closure.clone(),
                traits: *traits,
            }),
            HeapObject::LArrayMut { data, traits } => {
                // Share the backing Vec across the outbox copy: cloning
                // the Rc preserves the "mutable reference" semantics that
                // Elle users expect when a mutable array crosses a fiber
                // boundary via yield. Elements are Values (tag+ptr), so
                // they don't need deep-copy — the arena slots they point
                // to are shared already. If an element's slot does need
                // relocation (e.g. for a Fiber crossing outbox), that's
                // handled when the consumer iterates and deep-copies on
                // access; the shared `Rc` ensures they see live updates.
                outbox.alloc(HeapObject::LArrayMut {
                    data: data.clone(),
                    traits: *traits,
                })
            }
            HeapObject::LStructMut { data, traits } => {
                // Share the backing BTreeMap — see `LArrayMut` above for the
                // cross-fiber live-update rationale.
                outbox.alloc(HeapObject::LStructMut {
                    data: data.clone(),
                    traits: *traits,
                })
            }
            HeapObject::LStringMut { data, traits } => outbox.alloc(HeapObject::LStringMut {
                // Share the backing Vec<u8>.
                data: data.clone(),
                traits: *traits,
            }),
            HeapObject::LBytes { data, traits } => outbox.alloc(HeapObject::LBytes {
                data: *data,
                traits: *traits,
            }),
            HeapObject::LBytesMut { data, traits } => outbox.alloc(HeapObject::LBytesMut {
                // Share the backing Vec<u8>.
                data: data.clone(),
                traits: *traits,
            }),
            HeapObject::LSet { data, traits } => {
                // Snapshot elements and deep-copy each, then re-intern the
                // sorted slice into the outbox arena.
                let elems: Vec<Value> = data.as_slice().to_vec();
                let traits = *traits;
                let elems: Vec<Value> = elems
                    .into_iter()
                    .map(|v| self.deep_copy_to_outbox(v))
                    .collect();
                let outbox = self.outbox.as_mut().unwrap();
                let slice = outbox.alloc_inline_slice::<Value>(&elems);
                outbox.alloc(HeapObject::LSet {
                    data: slice,
                    traits,
                })
            }
            HeapObject::LSetMut { data, traits } => outbox.alloc(HeapObject::LSetMut {
                // Share the backing BTreeSet.
                data: data.clone(),
                traits: *traits,
            }),
            HeapObject::NativeFn(f) => outbox.alloc(HeapObject::NativeFn(f)),
            HeapObject::Parameter {
                id,
                default,
                traits,
            } => outbox.alloc(HeapObject::Parameter {
                id: *id,
                default: *default,
                traits: *traits,
            }),
            HeapObject::ManagedPointer { addr, traits } => {
                outbox.alloc(HeapObject::ManagedPointer {
                    addr: std::cell::Cell::new(addr.get()),
                    traits: *traits,
                })
            }
            HeapObject::Fiber { handle, traits } => outbox.alloc(HeapObject::Fiber {
                handle: handle.clone(),
                traits: *traits,
            }),
            HeapObject::Syntax { syntax, traits } => outbox.alloc(HeapObject::Syntax {
                syntax: syntax.clone(),
                traits: *traits,
            }),
            HeapObject::External { obj, traits } => outbox.alloc(HeapObject::External {
                obj: obj.clone(),
                traits: *traits,
            }),
            HeapObject::FFISignature(sig, cif) => {
                #[cfg(feature = "ffi")]
                let new_cif = RefCell::new(cif.borrow().clone());
                #[cfg(not(feature = "ffi"))]
                let new_cif = *cif;
                outbox.alloc(HeapObject::FFISignature(sig.clone(), new_cif))
            }
            HeapObject::FFIType(t) => outbox.alloc(HeapObject::FFIType(t.clone())),
            HeapObject::ThreadHandle { handle, traits } => outbox.alloc(HeapObject::ThreadHandle {
                handle: handle.clone(),
                traits: *traits,
            }),
            HeapObject::LibHandle(id) => outbox.alloc(HeapObject::LibHandle(*id)),
        }
    }

    /// Forward scope marks to the outbox when outbox is active.
    pub fn push_scope_mark_outbox(&mut self) {
        if self.outbox_active {
            if let Some(ref mut outbox) = self.outbox {
                // Push a mark on the outbox so RegionExit can release
                // scoped objects allocated in the outbox.
                let _mark = outbox.mark();
                // Note: outbox scope marks are managed through the main
                // scope_marks stack (which records shared_alloc_count).
            }
        }
    }

    // ── Refcounting ───────────────────────────────────────────────────

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
                return self.pool.decref(ptr as *const HeapObject);
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

    /// Decrement a value's refcount, and if it reaches 0, run its
    /// destructor and return its slab slot to the free list.
    ///
    /// Called at mutation points (put/push/set) when the old value is
    /// evicted from a collection. The old value is no longer referenced
    /// by any collection; if no other collection holds it (refcount 0),
    /// it can be freed immediately.
    pub fn decref_and_free(&mut self, val: Value) {
        if !val.is_heap() {
            return;
        }
        let ptr = match val.as_heap_ptr() {
            Some(p) => p,
            None => return,
        };
        if !self.pool.slab_owns(ptr) {
            return;
        }
        let typed = ptr as *mut HeapObject;
        let new_rc = self.pool.decref(typed as *const HeapObject);
        if new_rc == 0 {
            // Transitively decref the entire subtree to undo temporary
            // increfs from release_refcounted's protection phase.
            // Slot deallocation is deferred to release_refcounted to
            // avoid corrupting scope-mark-partitioned allocs/dtors lists.
            self.recursive_decref_contents(typed);
        }
    }

    /// Transitively decref all descendants of a dead object (rc==0).
    /// Does NOT free/dealloc any slots — children remain in allocs/dtors
    /// and will be collected by release_refcounted on the next rotation.
    fn recursive_decref_contents(&mut self, typed: *mut HeapObject) {
        let obj = unsafe { &*typed };
        let mut children = Vec::new();
        Self::collect_heap_children(obj, &mut children);
        for child_val in children {
            if let Some(child_ptr) = child_val.as_heap_ptr() {
                if self.pool.slab_owns(child_ptr) {
                    let child_typed = child_ptr as *mut HeapObject;
                    let old_rc = self.pool.refcount(child_typed as *const HeapObject);
                    if old_rc > 0 {
                        let new_rc = self.pool.decref(child_typed as *const HeapObject);
                        if new_rc == 0 {
                            // Child reached 0 — propagate decref to its
                            // children too (undoes their temporary increfs).
                            self.recursive_decref_contents(child_typed);
                        }
                    }
                }
            }
        }
    }

    /// Collect all heap-typed child Values from a HeapObject into `out`.
    fn collect_heap_children(obj: &HeapObject, out: &mut Vec<Value>) {
        // Helper: push traits if heap-allocated (permanent traitsets
        // won't match slab_owns, but user-attached traits will).
        let push_traits = |traits: &Value, out: &mut Vec<Value>| {
            if traits.is_heap() {
                out.push(*traits);
            }
        };
        match obj {
            HeapObject::LArrayMut { data, traits, .. } => {
                out.extend(data.borrow().iter().filter(|v| v.is_heap()).copied());
                push_traits(traits, out);
            }
            HeapObject::LStructMut { data, traits, .. } => {
                out.extend(data.borrow().values().filter(|v| v.is_heap()).copied());
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
                let v = *cell.borrow();
                if v.is_heap() {
                    out.push(v);
                }
                push_traits(traits, out);
            }
            HeapObject::LSet { data, traits, .. } => {
                out.extend(data.as_slice().iter().filter(|v| v.is_heap()).copied());
                push_traits(traits, out);
            }
            HeapObject::LSetMut { data, traits, .. } => {
                out.extend(data.borrow().iter().filter(|v| v.is_heap()).copied());
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

        // Tear down all outboxes (current and old).
        if let Some(mut outbox) = self.outbox.take() {
            outbox.teardown();
        }
        for mut ob in self.old_outboxes.drain(..) {
            ob.teardown();
        }
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
        self.pool.allocs.clear();
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
        // Tear down all outboxes (current and old).
        if let Some(mut outbox) = self.outbox.take() {
            outbox.teardown();
        }
        for mut ob in self.old_outboxes.drain(..) {
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
