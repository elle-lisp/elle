// audited: 2026-11-24
//! The unit-test build's allocator: the system allocator, counting each thread's allocations apart.
//!
//! docs/analysis/testing.md

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

/// The system allocator, with a count of the allocations each thread makes.
struct Counting;

thread_local! {
    /// The allocations this thread has made. A `const` cell with no destructor,
    /// so the allocator can read it while the thread is being torn down.
    static MADE: Cell<u64> = const { Cell::new(0) };
}

fn note() {
    let _ = MADE.try_with(|n| n.set(n.get() + 1));
}

// SAFETY: every method forwards to `System` unchanged; the count is the only
// addition, and it touches no memory the allocator hands out.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        System.alloc(layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note();
        System.alloc_zeroed(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size > layout.size() {
            note();
        }
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Run `f`, and answer what it returned with the number of allocations it made
/// on this thread, a growing `realloc` included.
pub(crate) fn allocations<R>(f: impl FnOnce() -> R) -> (R, u64) {
    let before = MADE.with(Cell::get);
    let out = f();
    (out, MADE.with(Cell::get) - before)
}

#[cfg(test)]
mod tests {
    use super::allocations;

    #[test]
    fn a_one_element_vec_is_one_allocation() {
        let (v, made) = allocations(|| vec![1u8]);
        assert_eq!(v, [1]);
        assert_eq!(made, 1, "the vector's buffer is the one allocation");
    }

    #[test]
    fn a_reused_buffer_allocates_nothing() {
        let mut v: Vec<u8> = Vec::with_capacity(4);
        let ((), made) = allocations(|| {
            v.clear();
            v.push(1);
        });
        assert_eq!(made, 0, "a push within capacity allocates nothing");
    }
}
