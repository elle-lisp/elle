//! Per-fiber heap ownership and thread-local current-heap routing.
//!
//! `FiberHeap` uses bumpalo for fast bump allocation. Destructor tracking
//! ensures that `HeapObject` variants with inner heap allocations (`Vec`, `Rc`,
//! `BTreeMap`, `Box<str>`, etc.) have their `Drop` impls called on release/clear.
//!
//! `peak_alloc_count` tracks the high-water mark of `alloc_count` since the
//! last `clear()`. Updated on every `alloc()`. Queryable via `arena/peak`
//! and `arena/fiber-stats`.
//!
//! ## Per-scope bump allocators
//!
//! Each `RegionEnter` pushes a fresh `bumpalo::Bump` onto `scope_bumps`.
//! All allocations within the scope go to this scope bump. `RegionExit`
//! runs destructors for scoped objects, then pops and drops the scope bump,
//! reclaiming ALL bump memory allocated in that scope.
//!
//! The root `bump` is used when `scope_bumps` is empty (no active scope).
//! `clear()` drops all scope bumps before resetting the root bump.
//!
//! ## Scope marks
//!
//! `FiberHeap` maintains a stack of scope marks (`scope_marks: Vec<ArenaMark>`)
//! for `RegionEnter`/`RegionExit` bytecodes. `RegionEnter` pushes a mark;
//! `RegionExit` pops the mark and calls `release()` to run destructors for
//! objects allocated within the scope, then drops the scope bump.
//!
//! The lowerer gates `RegionEnter`/`RegionExit` emission on escape analysis
//! (`src/lir/lower/escape.rs`): only scopes where no allocated values can
//! escape get region instructions. The analysis checks: no captures, no
//! suspension, result is immediate, no outward mutation.
//!
//! ## Active allocator pointer
//!
//! `FiberHeap` carries an `active_allocator: *const bumpalo::Bump` pointer
//! that tracks which bump allocator the current execution context should use.
//! Points to the top of `scope_bumps` when non-empty, otherwise to the root
//! `bump`.
//!
//! The pointer is saved/restored:
//! - On **Call/Return** via `execute_bytecode_saving_stack` (Rust call stack)
//! - On **Yield/Resume** via the `SuspendedFrame.active_allocator` field
//! - On **Fiber swap** implicitly (each fiber owns its own `FiberHeap`)
//!
//! ## Shared allocator for inter-fiber exchange
//!
//! `FiberHeap` owns zero or more `SharedAllocator`s (in `owned_shared: Vec<Box<SharedAllocator>>`)
//! and has a `shared_alloc: *mut SharedAllocator` pointer for routing.
//!
//! When `shared_alloc` is non-null, `alloc()` routes ALL allocations to the
//! shared allocator instead of the private bump. This is set by `with_child_fiber`
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
    bump: bumpalo::Bump,
    /// Per-scope bump allocators. `RegionEnter` pushes a new `Bump`;
    /// `RegionExit` pops and drops it, reclaiming all bump memory for
    /// that scope. When empty, allocations go to the root `bump`.
    scope_bumps: Vec<bumpalo::Bump>,
    /// Raw pointers to bump-allocated HeapObjects that need Drop.
    /// Ordered by allocation time (oldest first).
    dtors: Vec<*mut HeapObject>,
    /// Total number of objects allocated (including those not needing Drop).
    alloc_count: usize,
    /// Peak number of objects allocated (high-water mark).
    peak_alloc_count: usize,
    /// Pointer to the bump allocator that new allocations should use.
    /// Points to the top of `scope_bumps` when non-empty, otherwise to
    /// the root `bump`.
    ///
    /// Starts as null; set via `init_active_allocator()` after the FiberHeap
    /// is in its final Box location (pointer stability requires this).
    active_allocator: *const bumpalo::Bump,
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
            bump: bumpalo::Bump::new(),
            scope_bumps: Vec::new(),
            dtors: Vec::new(),
            alloc_count: 0,
            peak_alloc_count: 0,
            active_allocator: std::ptr::null(),
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

    /// Set `active_allocator` to point to the active bump (top of
    /// `scope_bumps`, or root `bump` if empty).
    ///
    /// Must be called after the FiberHeap is in its final Box location
    /// (the pointer targets `&self.bump`, which must not move). Called by
    /// `with_child_fiber` when installing the child's heap.
    pub fn init_active_allocator(&mut self) {
        self.active_allocator = self.active_bump_ptr();
    }

    /// Return a pointer to the currently active bump allocator:
    /// the top of `scope_bumps` if non-empty, otherwise the root `bump`.
    fn active_bump_ptr(&self) -> *const bumpalo::Bump {
        self.scope_bumps
            .last()
            .map(|b| b as *const bumpalo::Bump)
            .unwrap_or(&self.bump as *const bumpalo::Bump)
    }

    /// Current active allocator pointer. Returns null if not yet initialized.
    pub fn active_allocator(&self) -> *const bumpalo::Bump {
        self.active_allocator
    }

    pub fn alloc(&mut self, obj: HeapObject) -> Value {
        debug_assert!(
            !self.active_allocator.is_null(),
            "FiberHeap::alloc called before init_active_allocator"
        );
        // When a shared allocator is installed (yielding child fiber),
        // route ALL allocations to it. This is conservative: some
        // allocations may not escape the fiber, but sending them to
        // shared is always safe. The shared allocator handles its own
        // destructor tracking.
        if !self.shared_alloc.is_null() {
            return unsafe { &mut *self.shared_alloc }.alloc(obj);
        }

        // Custom allocator: try Rust trait object before bumpalo.
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
                return Value::from_heap_ptr(typed as *const ());
            }
            // Fall through to bumpalo on null return
        }

        // Check object limit before allocating
        if let Some(limit) = self.object_limit {
            if self.alloc_count >= limit {
                self.alloc_error = Some((self.alloc_count, limit));
                return Value::NIL;
            }
        }

        // Allocate from the active bump (scope bump or root bump).
        // `active_allocator` uses interior mutability (`Cell`-based chunks),
        // so casting away const is safe — bumpalo::Bump::alloc takes &self.
        let needs_drop = needs_drop(obj.tag());
        let bump_ref = unsafe { &*self.active_allocator };
        let ptr: &mut HeapObject = bump_ref.alloc(obj);
        let raw = ptr as *mut HeapObject;
        if needs_drop {
            self.dtors.push(raw);
        }
        self.alloc_count += 1;
        if self.alloc_count > self.peak_alloc_count {
            self.peak_alloc_count = self.alloc_count;
        }
        Value::from_heap_ptr(raw as *const ())
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
            self.scope_bumps.len(),
        )
    }

    /// Run destructors for objects allocated after the mark, then truncate
    /// the destructor list. For custom-allocated objects, also calls dealloc
    /// to return memory to the user's allocator.
    pub fn release(&mut self, mark: ArenaMark) {
        self.run_dtors(mark.dtor_len());
        self.dtors.truncate(mark.dtor_len());

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
    /// Records the current `(alloc_count, dtors.len(), bump_depth)` so that
    /// `pop_scope_mark_and_release` can run destructors and reclaim bump
    /// memory for objects allocated within the scope.
    pub fn push_scope_mark(&mut self) {
        self.scope_marks.push(self.mark());
        self.scope_bumps.push(bumpalo::Bump::new());
        self.active_allocator = self.active_bump_ptr();
        self.scope_enters += 1;
    }

    /// Pop the top scope mark and release objects allocated since it
    /// was pushed (called by `RegionExit`).
    ///
    /// Runs destructors for objects allocated within the scope (dtors
    /// point into the scope bump, so they MUST run before the bump is
    /// dropped), then pops and drops the scope bump to reclaim all
    /// bump memory for this scope.
    ///
    /// Panics (debug) if the scope stack is empty.
    pub fn pop_scope_mark_and_release(&mut self) {
        let mark = self
            .scope_marks
            .pop()
            .expect("RegionExit without matching RegionEnter");
        let dtors_before = self.dtors.len();
        let bump_depth = mark.bump_depth();
        // Run dtors and dealloc custom objects FIRST — pointers are into
        // the scope bump which we're about to drop.
        self.release(mark);
        // Drop the scope bump — reclaims all bump memory for this scope.
        debug_assert_eq!(
            self.scope_bumps.len(),
            bump_depth + 1,
            "scope bump stack depth mismatch on RegionExit"
        );
        self.scope_bumps.pop(); // Drop reclaims memory
        self.active_allocator = self.active_bump_ptr();
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
        self.bump.chunk_capacity()
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

    /// Bytes consumed by all bump allocators (root + scope bumps).
    pub fn allocated_bytes(&self) -> usize {
        self.bump.allocated_bytes()
            + self
                .scope_bumps
                .iter()
                .map(|b| b.allocated_bytes())
                .sum::<usize>()
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

    /// Drop all tracked objects and reset the bump allocator.
    ///
    /// Also tears down all owned shared allocators and nulls the
    /// shared_alloc pointer. Resets `active_allocator` to point to the
    /// root bump. The pointer remains valid because `Bump::reset()`
    /// doesn't move the Bump struct.
    pub fn clear(&mut self) {
        // Tear down owned shared allocators first (their dtors may
        // reference data that is not in our private bump).
        for sa in &mut self.owned_shared {
            sa.teardown();
        }
        self.owned_shared.clear();
        self.shared_alloc = std::ptr::null_mut();

        // Run all destructors (Drop) first, then dealloc custom memory.
        // Order matters: Drop may access the object's fields (Box<str>,
        // Vec, Rc, etc.) which must still be valid during Drop.
        self.run_dtors(0);
        self.dtors.clear();

        // Dealloc all custom-allocated objects. Drop has already run
        // for those that needed it (via run_dtors above).
        for state in self.custom_alloc_stack.drain(..) {
            for &(ptr, size, align) in state.custom_ptrs.iter().rev() {
                state.allocator.inner.dealloc(ptr, size, align);
            }
            // Rc<AllocatorBox> dropped here
        }

        self.scope_marks.clear();
        self.alloc_error = None;
        self.alloc_count = 0;
        self.peak_alloc_count = 0;
        self.scope_enters = 0;
        self.scope_dtors_run = 0;
        // Drop all scope bumps before resetting the root bump.
        self.scope_bumps.clear();
        self.bump.reset();
        // Reset to root bump in case scope-level redirection was active.
        if !self.active_allocator.is_null() {
            self.active_allocator = &self.bump as *const bumpalo::Bump;
        }
    }
}

impl Drop for FiberHeap {
    fn drop(&mut self) {
        // Tear down owned shared allocators before our bump is dropped.
        for sa in &mut self.owned_shared {
            sa.teardown();
        }
        // Run destructors for all tracked objects before the bump deallocates.
        // Without this, inner heap allocations (Vec buffers, Rc refcounts,
        // BTreeMap nodes, etc.) would leak when the bump is dropped.
        self.run_dtors(0);
        // Dealloc custom-allocated objects. Drop has already run above.
        for state in self.custom_alloc_stack.drain(..) {
            for &(ptr, size, align) in state.custom_ptrs.iter().rev() {
                state.allocator.inner.dealloc(ptr, size, align);
            }
        }
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
        HeapTag::Binding => false,
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
