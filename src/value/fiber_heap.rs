//! Per-fiber heap ownership and thread-local current-heap routing.
//!
//! `FiberHeap` uses bumpalo for fast bump allocation. Destructor tracking
//! ensures that `HeapObject` variants with inner heap allocations (`Vec`, `Rc`,
//! `BTreeMap`, `Box<str>`, etc.) have their `Drop` impls called on release/clear.
//!
//! The bump itself is only fully reset on `clear()` (fiber death / reset).
//! Partial `release(mark)` runs destructors but does not reclaim bump memory
//! — bumpalo has no partial reset. The real memory savings come from Drop
//! freeing the inner allocations (which live on the global heap, not in the
//! bump).
//!
//! ## Scope marks
//!
//! `FiberHeap` maintains a stack of scope marks (`scope_marks: Vec<ArenaMark>`)
//! for `RegionEnter`/`RegionExit` bytecodes. `RegionEnter` pushes a mark;
//! `RegionExit` pops the mark and calls `release()` to run destructors for
//! objects allocated within the scope.
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
//! Currently always points to the fiber's root bump. The machinery supports
//! future scope-level allocator redirection (separate scope bumps).
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

use std::cell::Cell;
use std::rc::Rc;

use crate::value::allocator::AllocatorBox;
use crate::value::heap::{ArenaMark, HeapObject, HeapTag};
use crate::value::Value;

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
    /// Raw pointers to bump-allocated HeapObjects that need Drop.
    /// Ordered by allocation time (oldest first).
    dtors: Vec<*mut HeapObject>,
    /// Total number of objects allocated (including those not needing Drop).
    alloc_count: usize,
    /// Pointer to the bump allocator that new allocations should use.
    /// Currently always points to `self.bump` once initialized. Supports
    /// future scope-level allocator redirection (separate scope bumps).
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
}

impl FiberHeap {
    pub fn new() -> Self {
        FiberHeap {
            bump: bumpalo::Bump::new(),
            dtors: Vec::new(),
            alloc_count: 0,
            active_allocator: std::ptr::null(),
            scope_marks: Vec::new(),
            owned_shared: Vec::new(),
            shared_alloc: std::ptr::null_mut(),
            scope_enters: 0,
            scope_dtors_run: 0,
            custom_alloc_stack: Vec::new(),
        }
    }

    /// Set `active_allocator` to point to this FiberHeap's own bump.
    ///
    /// Must be called after the FiberHeap is in its final Box location
    /// (the pointer targets `&self.bump`, which must not move). Called by
    /// `with_child_fiber` when installing the child's heap.
    pub fn init_active_allocator(&mut self) {
        self.active_allocator = &self.bump as *const bumpalo::Bump;
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
                return Value::from_heap_ptr(typed as *const ());
            }
            // Fall through to bumpalo on null return
        }

        // Normal bumpalo path (unchanged)
        let needs_drop = needs_drop(obj.tag());
        let ptr: &mut HeapObject = self.bump.alloc(obj);
        let raw = ptr as *mut HeapObject;
        if needs_drop {
            self.dtors.push(raw);
        }
        self.alloc_count += 1;
        Value::from_heap_ptr(raw as *const ())
    }

    pub fn mark(&self) -> ArenaMark {
        let custom_ptrs_len = self
            .custom_alloc_stack
            .last()
            .map_or(0, |s| s.custom_ptrs.len());
        ArenaMark::new_full(self.alloc_count, self.dtors.len(), custom_ptrs_len)
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
    /// Records the current `(alloc_count, dtors.len())` so that
    /// `pop_scope_mark_and_release` can run destructors for objects
    /// allocated within the scope.
    pub fn push_scope_mark(&mut self) {
        self.scope_marks.push(self.mark());
        self.scope_enters += 1;
    }

    /// Pop the top scope mark and release objects allocated since it
    /// was pushed (called by `RegionExit`).
    ///
    /// Runs destructors for objects allocated within the scope, truncates
    /// the destructor list, and restores `alloc_count`. Does NOT reset
    /// the bump (bumpalo has no partial reset).
    ///
    /// Panics (debug) if the scope stack is empty.
    pub fn pop_scope_mark_and_release(&mut self) {
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
        self.bump.chunk_capacity()
    }

    /// Number of `RegionEnter` instructions executed (scope regions entered).
    pub fn scope_enters(&self) -> usize {
        self.scope_enters
    }

    /// Number of destructors run by `RegionExit` (objects freed at scope exit).
    pub fn scope_dtors_run(&self) -> usize {
        self.scope_dtors_run
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
    pub fn create_shared_allocator(&mut self) -> *mut crate::value::shared_alloc::SharedAllocator {
        let mut sa = Box::new(crate::value::shared_alloc::SharedAllocator::new());
        let ptr = &mut *sa as *mut crate::value::shared_alloc::SharedAllocator;
        self.owned_shared.push(sa);
        ptr
    }

    /// Current shared allocator pointer. Returns null if none is set.
    pub fn shared_alloc(&self) -> *mut crate::value::shared_alloc::SharedAllocator {
        self.shared_alloc
    }

    /// Set the shared allocator pointer for this fiber.
    /// When non-null, `alloc()` routes all allocations to the shared allocator.
    pub fn set_shared_alloc(&mut self, ptr: *mut crate::value::shared_alloc::SharedAllocator) {
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
        self.alloc_count = 0;
        self.scope_enters = 0;
        self.scope_dtors_run = 0;
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
        HeapTag::Cell => false,
        HeapTag::Float => false,
        HeapTag::NativeFn => false,
        HeapTag::LibHandle => false,
        HeapTag::ManagedPointer => false,
        HeapTag::Binding => false,
        // Inner heap allocations (Box<str>, Vec, Rc, BTreeMap, Arc, Cif, etc.)
        HeapTag::String => true,
        HeapTag::Array => true,
        HeapTag::Table => true,
        HeapTag::Struct => true,
        HeapTag::Closure => true,
        HeapTag::Tuple => true,
        HeapTag::Buffer => true,
        HeapTag::Bytes => true,
        HeapTag::Blob => true,
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

thread_local! {
    static CURRENT_FIBER_HEAP: Cell<*mut FiberHeap> =
        const { Cell::new(std::ptr::null_mut()) };
}

/// Install a fiber heap as the current thread's active heap.
///
/// # Safety
/// Caller must ensure the FiberHeap outlives the installation.
pub unsafe fn install_fiber_heap(heap: *mut FiberHeap) {
    CURRENT_FIBER_HEAP.with(|cell| cell.set(heap));
}

pub fn uninstall_fiber_heap() {
    CURRENT_FIBER_HEAP.with(|cell| cell.set(std::ptr::null_mut()));
}

pub fn is_fiber_heap_installed() -> bool {
    CURRENT_FIBER_HEAP.with(|cell| !cell.get().is_null())
}

/// Read the current fiber heap raw pointer (single TLS read).
/// Returns null if no heap is installed. Used by `heap::alloc()` to avoid
/// double TLS lookup (checking installed + dispatching are one operation).
pub fn current_heap_ptr() -> *mut FiberHeap {
    CURRENT_FIBER_HEAP.with(|cell| cell.get())
}

pub fn save_current_heap() -> *mut FiberHeap {
    CURRENT_FIBER_HEAP.with(|cell| cell.get())
}

/// Restore a previously saved heap pointer.
///
/// # Safety
/// Pointer must still be valid or null.
pub unsafe fn restore_saved_heap(saved: *mut FiberHeap) {
    CURRENT_FIBER_HEAP.with(|cell| cell.set(saved));
}

/// Save the current `active_allocator` pointer from the installed FiberHeap.
/// Returns null if no FiberHeap is installed (root fiber).
pub fn save_active_allocator() -> *const bumpalo::Bump {
    CURRENT_FIBER_HEAP.with(|cell| {
        let ptr = cell.get();
        if ptr.is_null() {
            std::ptr::null()
        } else {
            unsafe { (*ptr).active_allocator }
        }
    })
}

/// Restore a previously saved `active_allocator` pointer on the installed FiberHeap.
/// No-op if no FiberHeap is installed (root fiber).
pub fn restore_active_allocator(saved: *const bumpalo::Bump) {
    CURRENT_FIBER_HEAP.with(|cell| {
        let ptr = cell.get();
        if !ptr.is_null() {
            unsafe {
                (*ptr).active_allocator = saved;
            }
        }
    })
}

pub fn with_current_heap_mut<R>(f: impl FnOnce(&mut FiberHeap) -> R) -> Option<R> {
    CURRENT_FIBER_HEAP.with(|cell| {
        let ptr = cell.get();
        if ptr.is_null() {
            None
        } else {
            Some(f(unsafe { &mut *ptr }))
        }
    })
}

/// Push a scope mark on the current FiberHeap (called by VM `RegionEnter`).
/// No-op if no FiberHeap is installed (root fiber).
pub fn region_enter() {
    let ptr = current_heap_ptr();
    if !ptr.is_null() {
        unsafe { (*ptr).push_scope_mark() };
    }
}

/// Pop a scope mark and release scoped objects on the current FiberHeap
/// (called by VM `RegionExit`).
/// No-op if no FiberHeap is installed (root fiber).
pub fn region_exit() {
    let ptr = current_heap_ptr();
    if !ptr.is_null() {
        unsafe { (*ptr).pop_scope_mark_and_release() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::heap::{Cons, HeapObject};

    #[test]
    fn test_fiber_heap_alloc() {
        let mut heap = FiberHeap::new();
        heap.init_active_allocator();
        let v = heap.alloc(HeapObject::String("hello".into()));
        assert_eq!(heap.len(), 1);
        assert!(v.is_heap());
        unsafe {
            let obj = crate::value::heap::deref(v);
            match obj {
                HeapObject::String(s) => assert_eq!(&**s, "hello"),
                _ => panic!("Expected String"),
            }
        }
    }

    #[test]
    fn test_fiber_heap_mark_release() {
        let mut heap = FiberHeap::new();
        heap.init_active_allocator();
        let mark = heap.mark();
        heap.alloc(HeapObject::String("a".into()));
        heap.alloc(HeapObject::String("b".into()));
        heap.alloc(HeapObject::String("c".into()));
        assert_eq!(heap.len(), 3);
        heap.release(mark);
        assert_eq!(heap.len(), 0);
    }

    #[test]
    fn test_fiber_heap_nested_mark_release() {
        let mut heap = FiberHeap::new();
        heap.init_active_allocator();
        let outer_mark = heap.mark();
        heap.alloc(HeapObject::String("outer".into()));
        let inner_mark = heap.mark();
        heap.alloc(HeapObject::String("inner".into()));
        assert_eq!(heap.len(), 2);
        heap.release(inner_mark);
        assert_eq!(heap.len(), 1);
        heap.release(outer_mark);
        assert_eq!(heap.len(), 0);
    }

    #[test]
    fn test_fiber_heap_clear_runs_destructors() {
        let mut heap = FiberHeap::new();
        heap.init_active_allocator();
        heap.alloc(HeapObject::String("a".into()));
        heap.alloc(HeapObject::String("b".into()));
        heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        assert_eq!(heap.len(), 3); // 3 total objects allocated
        assert_eq!(heap.dtors.len(), 2); // 2 need Drop (Strings)
        heap.clear();
        assert_eq!(heap.len(), 0);
        assert!(heap.is_empty());
    }

    #[test]
    fn test_clear_resets_scope_counters() {
        let mut heap = FiberHeap::new();
        heap.init_active_allocator();
        // Simulate a scope region with an allocation
        heap.push_scope_mark();
        heap.alloc(HeapObject::String("scoped".into()));
        heap.pop_scope_mark_and_release();
        assert_eq!(heap.scope_enters(), 1);
        assert_eq!(heap.scope_dtors_run(), 1);
        // clear() must zero both counters
        heap.clear();
        assert_eq!(heap.scope_enters(), 0);
        assert_eq!(heap.scope_dtors_run(), 0);
    }

    #[test]
    fn test_fiber_heap_non_drop_types_not_tracked() {
        let mut heap = FiberHeap::new();
        heap.init_active_allocator();
        heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        heap.alloc(HeapObject::Float(42.5));
        heap.alloc(HeapObject::Cell(std::cell::RefCell::new(Value::NIL), false));
        assert_eq!(heap.len(), 3); // 3 total objects
        assert_eq!(heap.dtors.len(), 0); // None need Drop tracking
    }

    #[test]
    fn test_fiber_heap_needs_drop_exhaustive() {
        // This test exists to document which tags need Drop.
        // If a new HeapTag variant is added, `needs_drop` won't compile
        // until a decision is made.
        assert!(!needs_drop(HeapTag::Cons));
        assert!(!needs_drop(HeapTag::Cell));
        assert!(!needs_drop(HeapTag::Float));
        assert!(!needs_drop(HeapTag::NativeFn));
        assert!(!needs_drop(HeapTag::LibHandle));
        assert!(!needs_drop(HeapTag::ManagedPointer));
        assert!(!needs_drop(HeapTag::Binding));

        assert!(needs_drop(HeapTag::String));
        assert!(needs_drop(HeapTag::Array));
        assert!(needs_drop(HeapTag::Table));
        assert!(needs_drop(HeapTag::Struct));
        assert!(needs_drop(HeapTag::Closure));
        assert!(needs_drop(HeapTag::Tuple));
        assert!(needs_drop(HeapTag::Buffer));
        assert!(needs_drop(HeapTag::Bytes));
        assert!(needs_drop(HeapTag::Blob));
        assert!(needs_drop(HeapTag::Syntax));
        assert!(needs_drop(HeapTag::Fiber));
        assert!(needs_drop(HeapTag::ThreadHandle));
        assert!(needs_drop(HeapTag::FFISignature));
        assert!(needs_drop(HeapTag::FFIType));
        assert!(needs_drop(HeapTag::External));
    }

    #[test]
    fn test_install_and_uninstall() {
        let mut heap = Box::new(FiberHeap::new());
        let ptr = &mut *heap as *mut FiberHeap;
        unsafe {
            install_fiber_heap(ptr);
        }
        assert!(is_fiber_heap_installed());
        assert!(with_current_heap_mut(|h| h.len()).is_some());
        uninstall_fiber_heap();
        assert!(!is_fiber_heap_installed());
    }

    #[test]
    fn test_no_heap_by_default() {
        // Ensure no heap is installed (may have been left by another test)
        uninstall_fiber_heap();
        assert!(!is_fiber_heap_installed());
        assert!(with_current_heap_mut(|h| h.len()).is_none());
    }

    #[test]
    fn test_save_restore() {
        let mut heap_a = Box::new(FiberHeap::new());
        let mut heap_b = Box::new(FiberHeap::new());
        heap_a.init_active_allocator();
        heap_b.init_active_allocator();
        heap_a.alloc(HeapObject::String("a".into()));
        heap_b.alloc(HeapObject::String("b1".into()));
        heap_b.alloc(HeapObject::String("b2".into()));

        let ptr_a = &mut *heap_a as *mut FiberHeap;
        let ptr_b = &mut *heap_b as *mut FiberHeap;

        unsafe {
            install_fiber_heap(ptr_a);
        }
        assert_eq!(with_current_heap_mut(|h| h.len()), Some(1));

        let saved = save_current_heap();
        unsafe {
            install_fiber_heap(ptr_b);
        }
        assert_eq!(with_current_heap_mut(|h| h.len()), Some(2));

        unsafe {
            restore_saved_heap(saved);
        }
        assert_eq!(with_current_heap_mut(|h| h.len()), Some(1));

        uninstall_fiber_heap();
    }

    #[test]
    fn test_active_allocator_starts_null() {
        let heap = FiberHeap::new();
        assert!(heap.active_allocator().is_null());
    }

    #[test]
    fn test_init_active_allocator_points_to_bump() {
        let mut heap = Box::new(FiberHeap::new());
        heap.init_active_allocator();
        let ptr = heap.active_allocator();
        assert!(!ptr.is_null());
        // The pointer should target the bump field inside the same FiberHeap.
        let bump_addr = &heap.bump as *const bumpalo::Bump;
        assert_eq!(ptr, bump_addr);
    }

    #[test]
    fn test_active_allocator_survives_clear() {
        let mut heap = Box::new(FiberHeap::new());
        heap.init_active_allocator();
        let ptr_before = heap.active_allocator();
        heap.alloc(HeapObject::String("x".into()));
        heap.clear();
        let ptr_after = heap.active_allocator();
        // Pointer should still be valid and point to the same bump.
        assert!(!ptr_after.is_null());
        assert_eq!(ptr_before, ptr_after);
    }

    #[test]
    fn test_save_restore_active_allocator() {
        let mut heap = Box::new(FiberHeap::new());
        heap.init_active_allocator();
        let heap_ptr = &mut *heap as *mut FiberHeap;

        unsafe { install_fiber_heap(heap_ptr) };

        let saved = save_active_allocator();
        assert!(!saved.is_null());

        // Simulate Package 5: active_allocator changes to something else
        restore_active_allocator(std::ptr::null());
        assert!(save_active_allocator().is_null());

        // Restore original
        restore_active_allocator(saved);
        assert_eq!(save_active_allocator(), saved);

        uninstall_fiber_heap();
    }

    #[test]
    fn test_save_active_allocator_no_heap_installed() {
        uninstall_fiber_heap();
        // No heap installed — returns null, no panic
        assert!(save_active_allocator().is_null());
    }

    #[test]
    fn test_restore_active_allocator_no_heap_installed() {
        uninstall_fiber_heap();
        // No heap installed — no-op, no panic
        let fake_ptr = 0x1234 as *const bumpalo::Bump;
        restore_active_allocator(fake_ptr);
        // Still no heap, so save returns null
        assert!(save_active_allocator().is_null());
    }

    // ── Scope mark stack tests ────────────────────────────────────

    #[test]
    fn test_scope_mark_push_pop_lifecycle() {
        let mut heap = FiberHeap::new();
        heap.init_active_allocator();
        heap.alloc(HeapObject::String("before".into()));
        assert_eq!(heap.len(), 1);

        heap.push_scope_mark();
        heap.alloc(HeapObject::String("scoped".into()));
        assert_eq!(heap.len(), 2);

        heap.pop_scope_mark_and_release();
        assert_eq!(heap.len(), 1); // back to pre-scope count
    }

    #[test]
    fn test_scope_mark_nested() {
        let mut heap = FiberHeap::new();
        heap.init_active_allocator();
        heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));

        heap.push_scope_mark();
        heap.alloc(HeapObject::String("outer".into()));

        heap.push_scope_mark();
        heap.alloc(HeapObject::String("inner".into()));
        assert_eq!(heap.len(), 3);

        heap.pop_scope_mark_and_release(); // pops inner
        assert_eq!(heap.len(), 2);

        heap.pop_scope_mark_and_release(); // pops outer
        assert_eq!(heap.len(), 1); // only the cons cell
    }

    #[test]
    fn test_scope_mark_runs_destructors() {
        let mut heap = FiberHeap::new();
        heap.init_active_allocator();
        assert_eq!(heap.dtors.len(), 0);

        heap.push_scope_mark();
        heap.alloc(HeapObject::String("a".into()));
        heap.alloc(HeapObject::String("b".into()));
        heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        assert_eq!(heap.dtors.len(), 2); // 2 Strings need Drop

        heap.pop_scope_mark_and_release();
        assert_eq!(heap.dtors.len(), 0); // destructors ran, list truncated
        assert_eq!(heap.len(), 0);
    }

    #[test]
    #[should_panic(expected = "RegionExit without matching RegionEnter")]
    fn test_scope_mark_pop_empty_panics() {
        let mut heap = FiberHeap::new();
        heap.pop_scope_mark_and_release();
    }

    #[test]
    fn test_clear_clears_scope_marks() {
        let mut heap = FiberHeap::new();
        heap.init_active_allocator();
        heap.push_scope_mark();
        heap.alloc(HeapObject::String("a".into()));
        heap.push_scope_mark();
        heap.alloc(HeapObject::String("b".into()));
        assert_eq!(heap.scope_marks.len(), 2);

        heap.clear();
        assert_eq!(heap.scope_marks.len(), 0);
        assert_eq!(heap.len(), 0);
    }

    // ── Shared allocator ownership tests ──────────────────────────────

    #[test]
    fn test_create_shared_allocator() {
        let mut heap = FiberHeap::new();
        let ptr = heap.create_shared_allocator();
        assert!(!ptr.is_null());
        assert_eq!(heap.owned_shared.len(), 1);
    }

    #[test]
    fn test_create_multiple_shared_allocators() {
        let mut heap = FiberHeap::new();
        let ptr1 = heap.create_shared_allocator();
        let ptr2 = heap.create_shared_allocator();
        assert!(!ptr1.is_null());
        assert!(!ptr2.is_null());
        assert_ne!(ptr1, ptr2);
        assert_eq!(heap.owned_shared.len(), 2);
    }

    #[test]
    fn test_shared_alloc_routing() {
        let mut heap = FiberHeap::new();
        heap.init_active_allocator();
        let sa_ptr = heap.create_shared_allocator();
        heap.set_shared_alloc(sa_ptr);

        // Allocate via FiberHeap — should route to shared
        heap.alloc(HeapObject::String("routed".into()));

        // Private bump should be untouched
        assert_eq!(heap.alloc_count, 0);
        assert_eq!(heap.dtors.len(), 0);

        // Shared allocator should have the allocation
        let sa = unsafe { &*sa_ptr };
        assert_eq!(sa.len(), 1);
    }

    #[test]
    fn test_private_alloc_when_no_shared() {
        let mut heap = FiberHeap::new();
        heap.init_active_allocator();
        // shared_alloc is null by default
        assert!(heap.shared_alloc.is_null());

        heap.alloc(HeapObject::String("private".into()));
        assert_eq!(heap.alloc_count, 1);
        assert_eq!(heap.dtors.len(), 1);
    }

    #[test]
    fn test_clear_tears_down_owned_shared() {
        let mut heap = FiberHeap::new();
        heap.init_active_allocator();
        let sa_ptr = heap.create_shared_allocator();
        heap.set_shared_alloc(sa_ptr);

        heap.alloc(HeapObject::String("shared-val".into()));
        assert_eq!(unsafe { &*sa_ptr }.len(), 1);

        heap.clear();
        assert!(heap.owned_shared.is_empty());
        assert!(heap.shared_alloc.is_null());
    }

    #[test]
    fn test_drop_tears_down_owned_shared() {
        // Create a FiberHeap with a shared allocator containing allocations,
        // then drop it. If Drop doesn't teardown, we'd leak inner heap allocs.
        let mut heap = FiberHeap::new();
        heap.init_active_allocator();
        let sa_ptr = heap.create_shared_allocator();
        heap.set_shared_alloc(sa_ptr);
        heap.alloc(HeapObject::String("will-be-dropped".into()));
        // Drop runs here — should not leak or panic.
        drop(heap);
    }

    // ── Shared allocator teardown lifecycle ────────────────────────────

    #[test]
    fn test_clear_tears_down_shared_alloc_dtors() {
        // Verify that clear() runs destructors in shared allocators.
        // String's inner Box<str> must be freed (dtors ran), verified
        // by checking the shared alloc's count goes to zero.
        let mut heap = FiberHeap::new();
        heap.init_active_allocator();
        let sa_ptr = heap.create_shared_allocator();
        heap.set_shared_alloc(sa_ptr);

        heap.alloc(HeapObject::String("str-a".into()));
        heap.alloc(HeapObject::String("str-b".into()));
        heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        {
            let sa = unsafe { &*sa_ptr };
            assert_eq!(sa.len(), 3);
        }

        // clear() should teardown the shared alloc (runs dtors, resets count)
        // then remove it from owned_shared.
        heap.clear();
        assert!(heap.owned_shared.is_empty());
        assert_eq!(heap.len(), 0);
    }

    #[test]
    fn test_multiple_shared_allocs_all_torn_down() {
        // Create 3 shared allocators, allocate into each, verify clear()
        // tears down all three.
        let mut heap = FiberHeap::new();
        heap.init_active_allocator();

        // Create 3 shared allocs, allocate strings into each
        let sa1 = heap.create_shared_allocator();
        heap.set_shared_alloc(sa1);
        heap.alloc(HeapObject::String("sa1-val".into()));
        heap.clear_shared_alloc();

        let sa2 = heap.create_shared_allocator();
        heap.set_shared_alloc(sa2);
        heap.alloc(HeapObject::String("sa2-val".into()));
        heap.clear_shared_alloc();

        let sa3 = heap.create_shared_allocator();
        heap.set_shared_alloc(sa3);
        heap.alloc(HeapObject::String("sa3-val".into()));
        heap.clear_shared_alloc();

        assert_eq!(heap.owned_shared.len(), 3);
        assert_eq!(unsafe { &*sa1 }.len(), 1);
        assert_eq!(unsafe { &*sa2 }.len(), 1);
        assert_eq!(unsafe { &*sa3 }.len(), 1);

        heap.clear();
        assert!(heap.owned_shared.is_empty());
        assert!(heap.shared_alloc.is_null());
    }

    #[test]
    fn test_shared_alloc_survives_private_clear() {
        // Shared allocs are NOT affected by private bump operations.
        // Private alloc_count/dtors are separate from shared.
        let mut heap = FiberHeap::new();
        heap.init_active_allocator();

        let sa_ptr = heap.create_shared_allocator();
        heap.set_shared_alloc(sa_ptr);
        heap.alloc(HeapObject::String("in-shared".into()));
        heap.clear_shared_alloc();

        // Allocate privately
        heap.alloc(HeapObject::String("in-private".into()));
        assert_eq!(heap.alloc_count, 1); // private count
        assert_eq!(unsafe { &*sa_ptr }.len(), 1); // shared count

        // Mark/release on private bump does not touch shared
        let mark = heap.mark();
        heap.alloc(HeapObject::String("scoped".into()));
        heap.release(mark);
        assert_eq!(heap.alloc_count, 1); // back to 1
        assert_eq!(unsafe { &*sa_ptr }.len(), 1); // shared unchanged
    }
}
