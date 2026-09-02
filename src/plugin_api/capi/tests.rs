use super::*;
use crate::value::arena::region_of;
use crate::value::fiberheap::FiberHeap;

// Spec: a stable-ABI constructor allocates into the region carried by the
// `CallCtx` it is handed — not any ambient/thread-scoped default. This is the
// property the v3 ctx-threading buys over the former `PLUGIN_CALL_ALLOC`
// thread-local: the region is an explicit argument, so it cannot be stale,
// missing, or belong to a different call.
//
// Pin validity (fault-injection, per the project's counterfactual discipline):
// these asserts detonate if a constructor ignores its ctx. Making `with_ctx`
// mint a fresh region (`heap.new_runtime_region()`) instead of `cx.region`, or
// read a hardcoded region, makes `region_of` report the wrong region and every
// assert here fails — confirmed RED before being reverted.

fn ctx_over(heap: &mut FiberHeap, region: crate::hir::region::RuntimeRegion) -> CallCtx {
    CallCtx {
        region,
        heap: heap as *mut FiberHeap,
        symbols: std::ptr::null_mut(),
    }
}

#[test]
fn make_string_born_in_ctx_region() {
    let mut heap = FiberHeap::new();
    let region = heap.new_runtime_region();
    let s = "plugin-built";
    let v = {
        let mut ctx = ctx_over(&mut heap, region);
        unsafe { to_value(make_string(&mut ctx, s.as_ptr(), s.len())) }
    };
    assert!(v.is_string(), "make_string must return a string");
    assert_eq!(
        region_of(&heap, v),
        Some(region),
        "make_string must allocate into the region carried by its ctx",
    );
}

#[test]
fn make_array_and_struct_born_in_ctx_region() {
    let mut heap = FiberHeap::new();
    let region = heap.new_runtime_region();

    let elem = from_value(Value::int(7));
    let arr = {
        let mut ctx = ctx_over(&mut heap, region);
        unsafe { to_value(make_array(&mut ctx, [elem].as_ptr(), 1)) }
    };
    assert!(arr.is_array(), "make_array must return an array");
    assert_eq!(
        region_of(&heap, arr),
        Some(region),
        "make_array must allocate into the region carried by its ctx",
    );

    let key = "k";
    let kv = ElleKVRaw {
        key: key.as_ptr(),
        key_len: key.len(),
        value: from_value(Value::int(1)),
    };
    let st = {
        let mut ctx = ctx_over(&mut heap, region);
        unsafe { to_value(make_struct(&mut ctx, [kv].as_ptr(), 1)) }
    };
    assert!(st.is_struct(), "make_struct must return a struct");
    assert_eq!(
        region_of(&heap, st),
        Some(region),
        "make_struct must allocate into the region carried by its ctx",
    );
}

// The capability is per-call: two ctxs naming *different* regions on the same
// heap route their allocations to their own regions. Under the old single
// ambient slot, "the region" was whichever was installed last; an explicit
// per-call argument cannot be reached or overridden by a sibling call.
#[test]
fn distinct_ctxs_route_to_distinct_regions() {
    let mut heap = FiberHeap::new();
    let r1 = heap.new_runtime_region();
    let r2 = heap.new_runtime_region();
    assert_ne!(r1, r2, "test needs two distinct regions");

    let a = "a";
    let v1 = {
        let mut c = ctx_over(&mut heap, r1);
        unsafe { to_value(make_string(&mut c, a.as_ptr(), a.len())) }
    };
    let v2 = {
        let mut c = ctx_over(&mut heap, r2);
        unsafe { to_value(make_string(&mut c, a.as_ptr(), a.len())) }
    };

    assert_eq!(region_of(&heap, v1), Some(r1));
    assert_eq!(
        region_of(&heap, v2),
        Some(r2),
        "the second ctx's region must be honored, not overridden by an ambient slot",
    );
}
