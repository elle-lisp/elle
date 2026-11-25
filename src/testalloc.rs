// audited: 2026-11-25
//! The unit-test build's allocator: the system allocator, counting each thread's allocations apart.
//!
//! docs/analysis/testing.md

use std::alloc::{GlobalAlloc, Layout, System};
use std::backtrace::Backtrace;
use std::cell::{Cell, RefCell};

/// The system allocator, with a count of the allocations each thread makes.
struct Counting;

thread_local! {
    /// The allocations this thread has made. A `const` cell with no destructor,
    /// so the allocator can read it while the thread is being torn down.
    static MADE: Cell<u64> = const { Cell::new(0) };
    /// Whether this thread records where each allocation came from.
    static RECORDING: Cell<bool> = const { Cell::new(false) };
    /// Set while the allocator captures a backtrace, whose own allocations
    /// it does not record.
    static CAPTURING: Cell<bool> = const { Cell::new(false) };
    /// The backtrace of each allocation made while this thread records.
    static SITES: RefCell<Vec<Backtrace>> = const { RefCell::new(Vec::new()) };
}

fn note() {
    let _ = MADE.try_with(|n| n.set(n.get() + 1));
    if RECORDING.try_with(Cell::get).unwrap_or(false) {
        record();
    }
}

/// Capture where this allocation came from. A panic hook captures and prints a
/// backtrace under a lock of its own, so a capture there would wait on that
/// lock: the allocator records nothing while the thread panics.
fn record() {
    if std::thread::panicking() || CAPTURING.try_with(Cell::get).unwrap_or(true) {
        return;
    }
    CAPTURING.with(|c| c.set(true));
    let site = Backtrace::force_capture();
    SITES.with(|s| s.borrow_mut().push(site));
    CAPTURING.with(|c| c.set(false));
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

/// Stops the recording when dropped, so a panic in the recorded closure leaves
/// the thread's allocator counting alone.
struct Recording;

impl Drop for Recording {
    fn drop(&mut self) {
        RECORDING.with(|r| r.set(false));
    }
}

/// Run `f`, and answer what it returned with the place each of its allocations
/// came from: the innermost frame in this crate's `src/`, as `file:line`.
/// Slower than [`allocations`] by a backtrace per allocation, so a test calls
/// it to say where a count it refused came from.
pub(crate) fn sites<R>(f: impl FnOnce() -> R) -> (R, Vec<String>) {
    SITES.with(|s| s.borrow_mut().clear());
    let out = {
        let _recording = Recording;
        RECORDING.with(|r| r.set(true));
        f()
    };
    let traces = SITES.with(|s| std::mem::take(&mut *s.borrow_mut()));
    (out, traces.iter().map(innermost_site).collect())
}

/// The innermost frame of `trace` in this crate's `src/` that called into the
/// allocator, or `?` where none is. A frame prints as its function's line, then
/// an `at path:line` line. The allocator's own frames sit in this file too, inner
/// to the `__rust_alloc` or `__rust_realloc` shim, so the walk starts past it.
fn innermost_site(trace: &Backtrace) -> String {
    let text = trace.to_string();
    let (mut in_shim, mut past_shim) = (false, false);
    for line in text.lines().map(str::trim) {
        let Some(at) = line.strip_prefix("at ") else {
            past_shim |= in_shim;
            in_shim = line.contains("__rust_alloc") || line.contains("__rust_realloc");
            continue;
        };
        if past_shim && at.starts_with("./src/") {
            return at.trim_start_matches("./").to_string();
        }
    }
    "?".to_string()
}

#[cfg(test)]
mod tests {
    use super::{allocations, sites};

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

    /// Allocate one boxed byte, and answer it with the line that allocated it.
    #[inline(never)]
    fn boxed() -> (Box<u8>, u32) {
        (Box::new(std::hint::black_box(1)), line!())
    }

    // The counter-factual is a site that names the allocator's own frame,
    // which sits in this file too: the line, not the file, tells them apart.
    #[test]
    fn a_site_names_the_line_that_allocated() {
        let ((b, line), at) = sites(boxed);
        assert_eq!(*b, 1);
        assert_eq!(at.len(), 1, "one allocation, one site: {at:?}");
        let expected = format!("src/testalloc.rs:{line}:");
        assert!(
            at[0].starts_with(&expected),
            "the site is {expected}, not {at:?}"
        );
    }
}
