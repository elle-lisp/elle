// audited: 2026-10-21
//! Unit tests for heap-object allocation, tags, size, and the region residence
//! an image dump depends on.
//!
//! docs/impl/values.md

use super::*;

#[test]
fn test_alloc_string() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let v = h.ctx().string("hello");
        assert!(v.is_heap());
        assert_eq!(v.with_string(|s| s.to_string()).unwrap(), "hello");
    });
}

#[test]
fn test_alloc_cons() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let v = h.ctx().pair(Value::NIL, Value::int(1));
        assert!(v.is_heap());
        unsafe {
            let obj = deref(v);
            match obj {
                HeapObject::Pair(c) => assert_eq!(c.rest.as_int(), Some(1)),
                _ => panic!("Expected Pair"),
            }
            drop_heap(v);
        }
    });
}

// An immutable struct's entries are page bytes of the struct's own region, the
// way an array's and a set's already are. That is what lets an image dump a
// struct as body data (docs/impl/image/foundations.md) and what keeps a
// struct off the Rust heap. Counter-factual: a `Vec` payload lives outside
// every region, so its address resolves to no region at all.
#[test]
fn immutable_struct_entries_live_in_the_struct_region() {
    let heap_ptr = crate::value::arena::leaked_test_heap();
    let heap = unsafe { &mut *heap_ptr };
    let region = heap.new_runtime_region();

    let mut fields = std::collections::BTreeMap::new();
    fields.insert(TableKey::keyword("a"), Value::int(1));
    fields.insert(TableKey::keyword("b"), Value::int(2));
    let built = crate::value::build::struct_from(heap, fields, region);

    let HeapObject::LStruct { data, .. } = (unsafe { deref(built) }) else {
        panic!("expected an immutable struct");
    };
    assert_eq!(
        unsafe { &*heap_ptr }.region_of_ptr(data.as_ptr() as *const ()),
        region.get(),
        "the entry slice must be backed by the struct's own region pages"
    );
}

#[test]
fn test_heap_tag() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let v = h.ctx().string("test");
        let s = unsafe { deref(v) };
        assert_eq!(s.tag(), HeapTag::LString);
        assert_eq!(s.type_name(), "string");
    });
}

// The syntax variant sets every object's size, in every build: one tag word,
// the node, and its `traits` (docs/impl/values.md). A larger payload goes
// behind a `Box`.
//
// The counter-factual is an FFI signature carrying its libffi CIF inline. That
// variant then sets the size, so the `ffi` feature decides how large every
// string, pair and closure is, and a build without it lays objects out
// differently from one with it.
#[test]
fn the_syntax_variant_sets_the_heap_object_size() {
    assert_eq!(
        std::mem::size_of::<HeapObject>(),
        std::mem::size_of::<u64>()
            + std::mem::size_of::<crate::syntax::Syntax>()
            + std::mem::size_of::<Value>(),
        "a variant larger than the syntax node sets the size of every heap object"
    );
}
