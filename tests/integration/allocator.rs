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
    let mut heap = Box::new(FiberHeap::new());
    heap.init_active_allocator();
    heap
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
    heap.alloc(HeapObject::LString { s: "hello".into(), traits: Value::NIL });

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

    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
    heap.alloc(HeapObject::LString { s: "test".into(), traits: Value::NIL });
    heap.alloc(HeapObject::Float(42.5));

    assert_eq!(alloc_count(), 3);
    assert_eq!(dealloc_count(), 0);

    heap.pop_custom_allocator();

    assert_eq!(dealloc_count(), 3);
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

    // Enter scope, allocate inside, then exit scope
    heap.push_scope_mark();
    heap.alloc(HeapObject::LString { s: "scoped".into(), traits: Value::NIL });
    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
    assert_eq!(alloc_count(), 3);
    assert_eq!(dealloc_count(), 0);

    heap.pop_scope_mark_and_release();

    // Scope exit should dealloc the 2 scoped objects
    assert_eq!(dealloc_count(), 2);
    assert_eq!(heap.len(), 1); // only the pre-scope object remains

    // Pop allocator should dealloc the remaining 1
    heap.pop_custom_allocator();
    assert_eq!(dealloc_count(), 3);
}

#[test]
fn test_form_exit_deallocs_remaining() {
    // pop_custom_allocator frees objects not covered by RegionExit.
    reset_counters();
    let mut heap = make_heap();

    let alloc = Rc::new(AllocatorBox::new(TlCountingAllocator));
    heap.push_custom_allocator(alloc);

    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
    heap.alloc(HeapObject::LString { s: "stays".into(), traits: Value::NIL });
    assert_eq!(alloc_count(), 2);

    heap.pop_custom_allocator();

    // Both should be deallocated
    assert_eq!(dealloc_count(), 2);
}

#[test]
fn test_clear_cleans_up_custom_allocators() {
    // FiberHeap::clear() frees all custom-allocated objects.
    reset_counters();
    let mut heap = make_heap();

    let alloc = Rc::new(AllocatorBox::new(TlCountingAllocator));
    heap.push_custom_allocator(alloc);

    heap.alloc(HeapObject::LString { s: "a".into(), traits: Value::NIL });
    heap.alloc(HeapObject::LString { s: "b".into(), traits: Value::NIL });
    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
    assert_eq!(alloc_count(), 3);

    heap.clear();

    assert_eq!(dealloc_count(), 3);
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

    // Push inner
    heap.push_custom_allocator(Rc::new(AllocatorBox::new(InnerAlloc)));
    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
    heap.alloc(HeapObject::LString { s: "inner".into(), traits: Value::NIL });
    assert_eq!(INNER_ALLOCS.with(|c| c.get()), 2);
    assert_eq!(OUTER_ALLOCS.with(|c| c.get()), 1); // outer unchanged

    // Pop inner
    heap.pop_custom_allocator();
    assert_eq!(INNER_DEALLOCS.with(|c| c.get()), 2);
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

    heap.alloc(HeapObject::LString { s: "will-drop".into(), traits: Value::NIL });
    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
    assert_eq!(alloc_count(), 2);

    drop(heap);
    assert_eq!(dealloc_count(), 2);
}

#[test]
fn test_no_custom_allocator_unchanged_behavior() {
    // Without custom allocator, behavior is unchanged.
    let mut heap = make_heap();
    let mark = heap.mark();
    heap.alloc(HeapObject::LString { s: "normal".into(), traits: Value::NIL });
    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
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

    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));

    heap.push_scope_mark(); // outer scope
    heap.alloc(HeapObject::LString { s: "outer-scoped".into(), traits: Value::NIL });

    heap.push_scope_mark(); // inner scope
    heap.alloc(HeapObject::LString { s: "inner-scoped".into(), traits: Value::NIL });
    assert_eq!(alloc_count(), 3);

    heap.pop_scope_mark_and_release(); // exit inner scope
    assert_eq!(dealloc_count(), 1); // inner-scoped freed

    heap.pop_scope_mark_and_release(); // exit outer scope
    assert_eq!(dealloc_count(), 2); // outer-scoped freed

    // Pop allocator frees the remaining pre-scope object
    heap.pop_custom_allocator();
    assert_eq!(dealloc_count(), 3);
}
