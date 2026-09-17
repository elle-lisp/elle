//! audited: 2026-09-16
//! Custom-allocator stack and heap teardown.
//!
//! `with-allocator` pushes a `CustomAllocState` that tracks raw allocations and
//! their destructors; popping (or dropping the heap) runs those dtors and frees
//! the memory. Grouped with `clear`/`Drop` because both walk the same
//! custom-allocator teardown loop, kept out of the region-allocator surface.

use std::rc::Rc;

use crate::value::allocator::AllocatorBox;

use super::{CustomAllocState, FiberHeap};

impl FiberHeap {
    /// Push a custom allocator onto the stack.
    pub fn push_custom_allocator(&mut self, allocator: Rc<AllocatorBox>) {
        self.custom_alloc_stack.push(CustomAllocState {
            allocator,
            custom_ptrs: Vec::new(),
            dtors: Vec::new(),
        });
    }

    /// Pop the top custom allocator, run Drop for remaining objects,
    /// then dealloc all remaining custom memory.
    pub fn pop_custom_allocator(&mut self) -> bool {
        let state = match self.custom_alloc_stack.pop() {
            Some(s) => s,
            None => return false,
        };

        // Run destructors for objects that need Drop.
        for &ptr in state.dtors.iter().rev() {
            if !ptr.is_null() {
                unsafe { std::ptr::drop_in_place(ptr) };
            }
        }

        // Dealloc all custom-allocated memory.
        for &(ptr, size, align) in state.custom_ptrs.iter().rev() {
            state.allocator.inner.dealloc(ptr, size, align);
        }
        true
    }

    /// Whether a shared allocator is active. Always false: nothing sets one.
    pub fn has_shared_alloc(&self) -> bool {
        false
    }

    /// Bring every I/O backend still on this heap to a quiescent state, before
    /// the region sweep that would otherwise run its destructor. Both callers
    /// below run this first, and the order is the argument:
    /// docs/impl/io-inflight.md § "A hold is let go while its store is still
    /// there".
    ///
    /// `collect_external_data` hands back a clone of each backend's `Rc`, so the
    /// backend outlives a release that frees the region its own value sits in.
    fn quiesce_io_backends(&mut self) {
        for data in self.collect_external_data("io-backend") {
            if let Some(backend) = data.downcast_ref::<crate::io::AnyBackend>() {
                backend.0.quiesce();
            }
        }
    }

    /// Drop all tracked objects and reset.
    pub fn clear(&mut self) {
        self.quiesce_io_backends();

        // Tear down custom allocators.
        for state in self.custom_alloc_stack.drain(..) {
            for &ptr in state.dtors.iter().rev() {
                if !ptr.is_null() {
                    unsafe { std::ptr::drop_in_place(ptr) };
                }
            }
            for &(ptr, size, align) in state.custom_ptrs.iter().rev() {
                state.allocator.inner.dealloc(ptr, size, align);
            }
        }

        // Tear down all physical regions.
        self.region_store.teardown_all();

        self.alloc_error = None;
        self.alloc_count = 0;
        self.peak_alloc_count = 0;
    }
}

impl Drop for FiberHeap {
    fn drop(&mut self) {
        self.quiesce_io_backends();

        // Tear down physical regions first (run dtors, return pages).
        self.region_store.teardown_all();

        // Tear down custom allocators.
        for state in self.custom_alloc_stack.drain(..) {
            for &ptr in state.dtors.iter().rev() {
                if !ptr.is_null() {
                    unsafe { std::ptr::drop_in_place(ptr) };
                }
            }
            for &(ptr, size, align) in state.custom_ptrs.iter().rev() {
                state.allocator.inner.dealloc(ptr, size, align);
            }
        }
    }
}
