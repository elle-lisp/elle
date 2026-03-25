//! Per-fiber heap ownership and thread-local current-heap routing.
//!
//! `FiberHeap` uses a slab allocator (`RootSlab`) for all allocations.
//! Destructor tracking ensures that `HeapObject` variants with inner heap
//! allocations (`Vec`, `Rc`, `BTreeMap`, `Box<str>`, etc.) have their `Drop`
//! impls called on release/clear.
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

use std::rc::Rc;

use crate::value::allocator::AllocatorBox;
use crate::value::arena::ArenaMark;
use crate::value::heap::{HeapObject, HeapTag};
use crate::value::Value;

mod routing;
pub use routing::*;

mod slab;
#[allow(unused_imports)]
pub(crate) use slab::RootSlab;

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
    /// Chunk-based slab for all allocations. Slots freed by
    /// `release()` are returned to the free list for reuse.
    root_slab: RootSlab,
    /// All root-slab allocations, in allocation order.
    /// Tracked so `release(mark)` can dealloc slots allocated after the mark.
    /// Does NOT include shared-allocator allocations.
    root_allocs: Vec<*mut HeapObject>,
    /// Raw pointers to HeapObjects that need Drop.
    /// Ordered by allocation time (oldest first).
    dtors: Vec<*mut HeapObject>,
    /// Total number of objects allocated (including those not needing Drop).
    alloc_count: usize,
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
    /// allocator instead of the private bump. Set by `with_child_fiber`
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
}

impl FiberHeap {
    pub fn new() -> Self {
        FiberHeap {
            root_slab: RootSlab::new(),
            root_allocs: Vec::new(),
            dtors: Vec::new(),
            alloc_count: 0,
            peak_alloc_count: 0,
            scope_marks: Vec::new(),
            owned_shared: Vec::new(),
            shared_alloc: std::ptr::null_mut(),
            scope_enters: 0,
            scope_dtors_run: 0,
            custom_alloc_stack: Vec::new(),
            object_limit: None,
            alloc_error: None,
        }
    }

    /// No-op kept for call-site compatibility (routing.rs and vm/fiber.rs call this).
    pub fn init_active_allocator(&mut self) {}

    pub fn alloc(&mut self, obj: HeapObject) -> Value {
        // When a shared allocator is installed (yielding child fiber),
        // route ALL allocations to it.  Track the count locally so that
        // arena/count reflects allocations made by this fiber even though
        // the storage lives on the parent's shared allocator.
        if !self.shared_alloc.is_null() {
            self.alloc_count += 1;
            if self.alloc_count > self.peak_alloc_count {
                self.peak_alloc_count = self.alloc_count;
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
                    self.dtors.push(typed);
                }
                self.alloc_count += 1;
                if self.alloc_count > self.peak_alloc_count {
                    self.peak_alloc_count = self.alloc_count;
                }
                return Value::from_heap_ptr(typed as *const (), value_tag);
            }
            // Fall through to slab on null return
        }

        // Check object limit before allocating
        if let Some(limit) = self.object_limit {
            if self.alloc_count >= limit {
                self.alloc_error = Some((self.alloc_count, limit));
                return Value::NIL;
            }
        }

        // Always allocate from the root slab.
        let needs_drop = needs_drop(obj.tag());
        let ptr = self.root_slab.alloc(obj);
        self.root_allocs.push(ptr);
        if needs_drop {
            self.dtors.push(ptr);
        }
        self.alloc_count += 1;
        if self.alloc_count > self.peak_alloc_count {
            self.peak_alloc_count = self.alloc_count;
        }
        Value::from_heap_ptr(ptr as *const (), value_tag)
    }

    pub fn mark(&self) -> ArenaMark {
        let custom_ptrs_len = self
            .custom_alloc_stack
            .last()
            .map_or(0, |s| s.custom_ptrs.len());
        ArenaMark::new_full(
            self.alloc_count,
            self.dtors.len(),
            custom_ptrs_len,
            self.root_allocs.len(),
        )
    }

    /// Run destructors for objects allocated after the mark, then truncate
    /// the destructor list. For custom-allocated objects, also calls dealloc
    /// to return memory to the user's allocator. For root-slab objects, returns
    /// slots to the slab free list.
    pub fn release(&mut self, mark: ArenaMark) {
        self.run_dtors(mark.dtor_len());
        self.dtors.truncate(mark.dtor_len());

        // Dealloc root-slab slots allocated after the mark.
        // Iterate in reverse (LIFO) to mirror dtor order, though slab dealloc
        // order doesn't affect correctness (dtors have already run).
        for &ptr in self.root_allocs[mark.root_allocs_len()..].iter().rev() {
            self.root_slab.dealloc(ptr);
        }
        self.root_allocs.truncate(mark.root_allocs_len());

        // Dealloc custom-allocated objects from the exiting scope.
        if let Some(state) = self.custom_alloc_stack.last_mut() {
            let start = mark.custom_ptrs_len();
            for &(ptr, size, align) in state.custom_ptrs[start..].iter().rev() {
                state.allocator.inner.dealloc(ptr, size, align);
            }
            state.custom_ptrs.truncate(start);
        }

        self.alloc_count = mark.position();
    }

    /// Run destructors in reverse order from `self.dtors[start..]`.
    ///
    /// # Safety
    /// Each pointer in `dtors` must be valid for `drop_in_place`.
    /// This is guaranteed as long as the bump arena hasn't been reset
    /// (which would deallocate the memory without calling destructors).
    fn run_dtors(&self, start: usize) {
        for i in (start..self.dtors.len()).rev() {
            unsafe {
                std::ptr::drop_in_place(self.dtors[i]);
            }
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
        let dtors_before = self.dtors.len();
        self.release(mark);
        self.scope_dtors_run += dtors_before - self.dtors.len();
    }

    /// Total number of objects allocated since last clear/release.
    pub fn len(&self) -> usize {
        self.alloc_count
    }

    pub fn is_empty(&self) -> bool {
        self.alloc_count == 0
    }

    pub fn capacity(&self) -> usize {
        self.root_slab.capacity_bytes()
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

    /// Bytes committed by root slab.
    pub fn allocated_bytes(&self) -> usize {
        self.root_slab.allocated_bytes()
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
        self.dtors.len()
    }

    /// Number of live slots in the root slab.
    pub(crate) fn root_live(&self) -> usize {
        self.root_slab.live_count()
    }

    /// Number of root allocations tracked for release().
    pub(crate) fn root_alloc_count(&self) -> usize {
        self.root_allocs.len()
    }

    /// Number of owned shared allocators.
    pub(crate) fn shared_count(&self) -> usize {
        self.owned_shared.len()
    }

    /// Active allocator discriminant as a keyword (always `"slab"` now).
    pub(crate) fn active_allocator_keyword(&self) -> &'static str {
        "slab"
    }

    /// Reset peak to current count. Returns previous peak.
    pub fn reset_peak(&mut self) -> usize {
        let prev = self.peak_alloc_count;
        self.peak_alloc_count = self.alloc_count;
        prev
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
            if let Some(pos) = self.dtors.iter().rposition(|&d| d == typed) {
                // SAFETY: The pointer is valid — it was allocated by the
                // custom allocator and has not been freed yet.
                unsafe { std::ptr::drop_in_place(typed) };
                self.dtors.swap_remove(pos);
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
    pub(crate) fn create_shared_allocator(
        &mut self,
    ) -> *mut crate::value::shared_alloc::SharedAllocator {
        let mut sa = Box::new(crate::value::shared_alloc::SharedAllocator::new());
        let ptr = &mut *sa as *mut crate::value::shared_alloc::SharedAllocator;
        self.owned_shared.push(sa);
        ptr
    }

    /// Return an existing shared allocator from `owned_shared`, or create one.
    ///
    /// Prevents the per-resume leak: without this, each `with_child_fiber`
    /// call pushes a new `SharedAllocator` that accumulates until the
    /// owner's `FiberHeap::clear()` runs. Reusing the last allocator keeps
    /// `owned_shared` at most length 1 for non-propagation cases.
    pub(crate) fn get_or_create_shared_allocator(
        &mut self,
    ) -> *mut crate::value::shared_alloc::SharedAllocator {
        if let Some(sa) = self.owned_shared.last_mut() {
            &mut **sa as *mut crate::value::shared_alloc::SharedAllocator
        } else {
            self.create_shared_allocator()
        }
    }

    /// Current shared allocator pointer. Returns null if none is set.
    pub(crate) fn shared_alloc(&self) -> *mut crate::value::shared_alloc::SharedAllocator {
        self.shared_alloc
    }

    /// Set the shared allocator pointer for this fiber.
    /// When non-null, `alloc()` routes all allocations to the shared allocator.
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

    /// Drop all tracked objects and reset the slab allocator.
    ///
    /// Also tears down all owned shared allocators and nulls the
    /// shared_alloc pointer.
    pub fn clear(&mut self) {
        // Tear down owned shared allocators first.
        for sa in &mut self.owned_shared {
            sa.teardown();
        }
        self.owned_shared.clear();
        self.shared_alloc = std::ptr::null_mut();

        // Run all destructors before clearing slab memory.
        self.run_dtors(0);
        self.dtors.clear();

        // Dealloc all custom-allocated objects.
        for state in self.custom_alloc_stack.drain(..) {
            for &(ptr, size, align) in state.custom_ptrs.iter().rev() {
                state.allocator.inner.dealloc(ptr, size, align);
            }
            // Rc<AllocatorBox> dropped here
        }

        // Clear root slab tracking and reset slab (keeps first chunk).
        self.root_allocs.clear();
        self.root_slab.clear();

        self.scope_marks.clear();
        self.alloc_error = None;
        self.alloc_count = 0;
        self.peak_alloc_count = 0;
        self.scope_enters = 0;
        self.scope_dtors_run = 0;
    }
}

impl Drop for FiberHeap {
    fn drop(&mut self) {
        // Tear down owned shared allocators before our slab is dropped.
        for sa in &mut self.owned_shared {
            sa.teardown();
        }
        // Run destructors for all tracked objects while slab memory is still valid.
        self.run_dtors(0);
        // Dealloc custom-allocated objects. Drop has already run above.
        for state in self.custom_alloc_stack.drain(..) {
            for &(ptr, size, align) in state.custom_ptrs.iter().rev() {
                state.allocator.inner.dealloc(ptr, size, align);
            }
        }
        // root_slab drops implicitly here. MaybeUninit slots do not call
        // HeapObject::drop — dtors have already run above.
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
        HeapTag::Cons => false,
        HeapTag::LBox => false,
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
