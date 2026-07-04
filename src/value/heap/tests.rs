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
