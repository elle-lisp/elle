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
// struct as body data (docs/impl/image.md § Foundations) and what keeps a
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
