use elle::value::allocator::{AllocatorBox, ElleAllocator};
use elle::value::fiberheap::FiberHeap;
use elle::value::heap::{Cons, HeapObject};
use elle::Value;
use std::cell::Cell;
use std::rc::Rc;

// ── Mock allocator ──────────────────────────────────────────────────

/// A test allocator that counts alloc/dealloc calls and delegates
/// to the global allocator.
struct CountingAllocator {
    allocs: Cell<usize>,
    deallocs: Cell<usize>,
}

impl CountingAllocator {
    fn new() -> Self {
        CountingAllocator {
            allocs: Cell::new(0),
            deallocs: Cell::new(0),
        }
    }
}

impl ElleAllocator for CountingAllocator {
    fn alloc(&self, size: usize, align: usize) -> *mut u8 {
        let layout = std::alloc::Layout::from_size_align(size, align).unwrap();
        let ptr = unsafe { std::alloc::alloc(layout) };
        if !ptr.is_null() {
            self.allocs.set(self.allocs.get() + 1);
        }
        ptr
    }

    fn dealloc(&self, ptr: *mut u8, size: usize, align: usize) {
        self.deallocs.set(self.deallocs.get() + 1);
        let layout = std::alloc::Layout::from_size_align(size, align).unwrap();
        unsafe { std::alloc::dealloc(ptr, layout) };
    }
}

/// An allocator that always returns null (forces bumpalo fallback).
struct NullAllocator;

impl ElleAllocator for NullAllocator {
    fn alloc(&self, _size: usize, _align: usize) -> *mut u8 {
        std::ptr::null_mut()
    }

    fn dealloc(&self, _ptr: *mut u8, _size: usize, _align: usize) {
        panic!("dealloc should not be called on NullAllocator");
    }
}

// ── Helper ──────────────────────────────────────────────────────────

/// Create an initialized FiberHeap ready for allocation.
fn make_heap() -> Box<FiberHeap> {
    Box::new(FiberHeap::new())
}

/// Allocate an LString HeapObject whose bytes live inline in `heap`'s arena.
/// After Phase 2, LString.s is an `InlineSlice<u8>` rather than a `Box<str>`,
/// so allocating a string is a two-step process: slice first, HeapObject next.
fn alloc_str(heap: &mut FiberHeap, text: &str) -> Value {
    let s = heap.alloc_inline_slice::<u8>(text.as_bytes());
    heap.alloc(HeapObject::LString {
        s,
        traits: Value::NIL,
    })
}

/// Allocate an LBytes HeapObject whose bytes live inline in `heap`'s arena.
fn alloc_bytes(heap: &mut FiberHeap, data: &[u8]) -> Value {
    let d = heap.alloc_inline_slice::<u8>(data);
    heap.alloc(HeapObject::LBytes {
        data: d,
        traits: Value::NIL,
    })
}

// ── Tests ───────────────────────────────────────────────────────────

#[test]
fn test_custom_alloc_dispatch() {
    // Allocations route through the custom allocator when installed.
    let mut heap = make_heap();

    let alloc = Rc::new(AllocatorBox::new(CountingAllocator::new()));
    heap.push_custom_allocator(alloc.clone());

    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
    alloc_str(&mut heap, "hello");

    // All 3 allocations should have gone through the custom allocator.
    assert_eq!(heap.len(), 3);

    // Pop the allocator — should dealloc all 3 objects.
    heap.pop_custom_allocator();
}

#[test]
fn test_custom_alloc_fallback_on_null() {
    // If the custom allocator returns null, bumpalo is used.
    let mut heap = make_heap();

    let alloc = Rc::new(AllocatorBox::new(NullAllocator));
    heap.push_custom_allocator(alloc);

    let v = heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
    assert_eq!(heap.len(), 1);
    // The value should be valid (allocated on bumpalo).
    assert!(v.is_heap());
    unsafe {
        let obj = elle::value::heap::deref(v);
        assert!(matches!(obj, HeapObject::Cons(_)));
    }
}

// ── Counting via thread-local to avoid AllocatorBox indirection ─────

// Use a thread-local counter to verify alloc/dealloc counts from tests.
thread_local! {
    static ALLOC_COUNT: Cell<usize> = const { Cell::new(0) };
    static DEALLOC_COUNT: Cell<usize> = const { Cell::new(0) };
}

struct TlCountingAllocator;

impl ElleAllocator for TlCountingAllocator {
    fn alloc(&self, size: usize, align: usize) -> *mut u8 {
        ALLOC_COUNT.with(|c| c.set(c.get() + 1));
        let layout = std::alloc::Layout::from_size_align(size, align).unwrap();
        unsafe { std::alloc::alloc(layout) }
    }

    fn dealloc(&self, ptr: *mut u8, size: usize, align: usize) {
        DEALLOC_COUNT.with(|c| c.set(c.get() + 1));
        let layout = std::alloc::Layout::from_size_align(size, align).unwrap();
        unsafe { std::alloc::dealloc(ptr, layout) };
    }
}

fn reset_counters() {
    ALLOC_COUNT.with(|c| c.set(0));
    DEALLOC_COUNT.with(|c| c.set(0));
}

fn alloc_count() -> usize {
    ALLOC_COUNT.with(|c| c.get())
}

fn dealloc_count() -> usize {
    DEALLOC_COUNT.with(|c| c.get())
}

#[test]
fn test_custom_alloc_counts() {
    reset_counters();
    let mut heap = make_heap();

    let alloc = Rc::new(AllocatorBox::new(TlCountingAllocator));
    heap.push_custom_allocator(alloc);

    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL))); // 1
    alloc_str(&mut heap, "test"); // 2 (inline bytes + HeapObject)
    alloc_bytes(&mut heap, &[1, 2, 3]); // 2

    assert_eq!(alloc_count(), 5);
    assert_eq!(dealloc_count(), 0);

    heap.pop_custom_allocator();

    assert_eq!(dealloc_count(), 5);
}

#[test]
fn test_scope_exit_deallocs_custom_objects() {
    // RegionExit calls dealloc for custom objects in the exiting scope.
    reset_counters();
    let mut heap = make_heap();

    let alloc = Rc::new(AllocatorBox::new(TlCountingAllocator));
    heap.push_custom_allocator(alloc);

    // Allocate outside scope
    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
    assert_eq!(alloc_count(), 1);

    // Enter scope, allocate inside, then exit scope.
    // alloc_str is 2 allocs (inline bytes + HeapObject); Cons is 1.
    heap.push_scope_mark();
    alloc_str(&mut heap, "scoped");
    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
    assert_eq!(alloc_count(), 4);
    assert_eq!(dealloc_count(), 0);

    heap.pop_scope_mark_and_release();

    // Scope exit should dealloc the 3 scoped allocations (bytes + LString + Cons)
    assert_eq!(dealloc_count(), 3);
    assert_eq!(heap.len(), 1); // only the pre-scope Cons HeapObject remains

    // Pop allocator should dealloc the remaining 1
    heap.pop_custom_allocator();
    assert_eq!(dealloc_count(), 4);
}

#[test]
fn test_form_exit_deallocs_remaining() {
    // pop_custom_allocator frees objects not covered by RegionExit.
    reset_counters();
    let mut heap = make_heap();

    let alloc = Rc::new(AllocatorBox::new(TlCountingAllocator));
    heap.push_custom_allocator(alloc);

    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL))); // 1
    alloc_str(&mut heap, "stays"); // 2 (inline bytes + HeapObject)
    assert_eq!(alloc_count(), 3);

    heap.pop_custom_allocator();

    // All three should be deallocated
    assert_eq!(dealloc_count(), 3);
}

#[test]
fn test_clear_cleans_up_custom_allocators() {
    // FiberHeap::clear() frees all custom-allocated objects.
    reset_counters();
    let mut heap = make_heap();

    let alloc = Rc::new(AllocatorBox::new(TlCountingAllocator));
    heap.push_custom_allocator(alloc);

    alloc_str(&mut heap, "a"); // 2 allocs: inline bytes + HeapObject
    alloc_str(&mut heap, "b"); // 2 allocs
    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL))); // 1 alloc
    assert_eq!(alloc_count(), 5);

    heap.clear();

    assert_eq!(dealloc_count(), 5);
    assert_eq!(heap.len(), 0);
}

#[test]
fn test_nested_allocators() {
    // Inner allocator gets allocations, outer doesn't.
    thread_local! {
        static OUTER_ALLOCS: Cell<usize> = const { Cell::new(0) };
        static OUTER_DEALLOCS: Cell<usize> = const { Cell::new(0) };
        static INNER_ALLOCS: Cell<usize> = const { Cell::new(0) };
        static INNER_DEALLOCS: Cell<usize> = const { Cell::new(0) };
    }

    struct OuterAlloc;
    impl ElleAllocator for OuterAlloc {
        fn alloc(&self, size: usize, align: usize) -> *mut u8 {
            OUTER_ALLOCS.with(|c| c.set(c.get() + 1));
            let layout = std::alloc::Layout::from_size_align(size, align).unwrap();
            unsafe { std::alloc::alloc(layout) }
        }
        fn dealloc(&self, ptr: *mut u8, size: usize, align: usize) {
            OUTER_DEALLOCS.with(|c| c.set(c.get() + 1));
            let layout = std::alloc::Layout::from_size_align(size, align).unwrap();
            unsafe { std::alloc::dealloc(ptr, layout) };
        }
    }

    struct InnerAlloc;
    impl ElleAllocator for InnerAlloc {
        fn alloc(&self, size: usize, align: usize) -> *mut u8 {
            INNER_ALLOCS.with(|c| c.set(c.get() + 1));
            let layout = std::alloc::Layout::from_size_align(size, align).unwrap();
            unsafe { std::alloc::alloc(layout) }
        }
        fn dealloc(&self, ptr: *mut u8, size: usize, align: usize) {
            INNER_DEALLOCS.with(|c| c.set(c.get() + 1));
            let layout = std::alloc::Layout::from_size_align(size, align).unwrap();
            unsafe { std::alloc::dealloc(ptr, layout) };
        }
    }

    OUTER_ALLOCS.with(|c| c.set(0));
    OUTER_DEALLOCS.with(|c| c.set(0));
    INNER_ALLOCS.with(|c| c.set(0));
    INNER_DEALLOCS.with(|c| c.set(0));

    let mut heap = make_heap();

    // Push outer
    heap.push_custom_allocator(Rc::new(AllocatorBox::new(OuterAlloc)));
    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
    assert_eq!(OUTER_ALLOCS.with(|c| c.get()), 1);

    // Push inner. Cons = 1 alloc, alloc_str = 2 allocs.
    heap.push_custom_allocator(Rc::new(AllocatorBox::new(InnerAlloc)));
    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
    alloc_str(&mut heap, "inner");
    assert_eq!(INNER_ALLOCS.with(|c| c.get()), 3);
    assert_eq!(OUTER_ALLOCS.with(|c| c.get()), 1); // outer unchanged

    // Pop inner
    heap.pop_custom_allocator();
    assert_eq!(INNER_DEALLOCS.with(|c| c.get()), 3);
    assert_eq!(OUTER_DEALLOCS.with(|c| c.get()), 0);

    // Allocate more on outer
    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
    assert_eq!(OUTER_ALLOCS.with(|c| c.get()), 2);

    // Pop outer
    heap.pop_custom_allocator();
    assert_eq!(OUTER_DEALLOCS.with(|c| c.get()), 2);
}

#[test]
fn test_drop_cleans_up_custom_allocators() {
    // FiberHeap Drop runs dealloc for custom objects.
    reset_counters();
    let mut heap = make_heap();

    let alloc = Rc::new(AllocatorBox::new(TlCountingAllocator));
    heap.push_custom_allocator(alloc);

    alloc_str(&mut heap, "will-drop"); // 2 (inline bytes + HeapObject)
    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL))); // 1
    assert_eq!(alloc_count(), 3);

    drop(heap);
    assert_eq!(dealloc_count(), 3);
}

#[test]
fn test_no_custom_allocator_unchanged_behavior() {
    // Without custom allocator, behavior is unchanged.
    let mut heap = make_heap();
    let mark = heap.mark();
    alloc_str(&mut heap, "normal");
    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
    // Two HeapObject allocs (LString + Cons); the inline bytes slice for the
    // string doesn't register as a HeapObject but does live in the arena.
    assert_eq!(heap.len(), 2);
    heap.release(mark);
    assert_eq!(heap.len(), 0);
}

#[test]
fn test_nested_scopes_with_custom_allocator() {
    // Nested scope marks with custom allocator correctly track custom_ptrs_len.
    reset_counters();
    let mut heap = make_heap();

    let alloc = Rc::new(AllocatorBox::new(TlCountingAllocator));
    heap.push_custom_allocator(alloc);

    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL))); // 1

    heap.push_scope_mark(); // outer scope
    alloc_str(&mut heap, "outer-scoped"); // 2 (inline bytes + HeapObject)

    heap.push_scope_mark(); // inner scope
    alloc_str(&mut heap, "inner-scoped"); // 2
    assert_eq!(alloc_count(), 5);

    heap.pop_scope_mark_and_release(); // exit inner scope
    assert_eq!(dealloc_count(), 2); // inner-scoped bytes + HeapObject freed

    heap.pop_scope_mark_and_release(); // exit outer scope
    assert_eq!(dealloc_count(), 4); // outer-scoped bytes + HeapObject freed

    // Pop allocator frees the remaining pre-scope Cons
    heap.pop_custom_allocator();
    assert_eq!(dealloc_count(), 5);
}
