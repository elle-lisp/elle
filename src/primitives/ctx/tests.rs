//! Unit tests (`super` is the parent impl module).

use super::Alloc;
use crate::hir::region::RuntimeRegion;
use crate::rich_error;
use crate::value::heap::{HeapObject, HeapTag, Pair};
use crate::value::Value;

/// Run `f` with an `Alloc` over a fresh region on the root heap, passing
/// that captured `region` alongside so a test can assert "born in the ctx's
/// region" WITHOUT a region getter on the ctx (production code has none).
/// Releases the region afterward.
fn with_ctx<R>(f: impl FnOnce(&mut Alloc, RuntimeRegion) -> R) -> R {
    let heap_ptr = crate::value::arena::leaked_test_heap();
    let heap = unsafe { &mut *heap_ptr };
    let region = heap.new_runtime_region();
    let out = {
        let mut ctx = Alloc::with_region(region, unsafe { &mut *heap_ptr });
        f(&mut ctx, region)
    };
    heap.decref_region_if_present(region);
    out
}

#[test]
fn alloc_routes_into_ctx_region() {
    with_ctx(|ctx, expected| {
        let v = ctx.alloc(HeapObject::Pair(Pair {
            first: Value::int(1),
            rest: Value::NIL,
            traits: Value::NIL,
        }));
        assert_eq!(
            crate::value::arena::region_of(ctx.heap_mut(), v),
            Some(expected),
            "ctx.alloc must allocate into the ctx's own region (Rule 3)",
        );
    });
}

/// `rich_error!` builds a `(SIG_ERROR, {:error :message …})` whose every
/// field is born in the error's own region (docs/impl/region-errors.md): the
/// `:path` string field, written `path = ctx.string(...)`, must share the
/// error struct's region. The counterfactual is exact — make `error_extra`
/// build the struct in any region other than the one `ctx.string` used and
/// `region_of(path) != region_of(err)` makes the `assert_eq!` fire.
#[test]
fn rich_error_string_field_shares_the_error_region() {
    with_ctx(|ctx, ctx_region| {
        let (bits, err) = rich_error!(ctx, "io-error", "boom", path = ctx.string("/x"));
        assert_eq!(bits, crate::value::SIG_ERROR);
        let err_region = crate::value::arena::region_of(ctx.heap_mut(), err);
        assert_eq!(
            err_region,
            Some(ctx_region),
            "the error struct is born in the ctx's region",
        );
        let fields = err.as_struct().expect("error is a struct");
        let path_val = crate::value::types::sorted_struct_get(
            fields,
            &crate::value::heap::TableKey::Keyword("path".into()),
        )
        .copied()
        .expect(":path field present");
        assert_eq!(
            crate::value::arena::region_of(ctx.heap_mut(), path_val),
            err_region,
            "the :path string field must be born in the error's own region",
        );
    });
}

#[test]
fn alloc_slice_payload_shares_ctx_region() {
    with_ctx(|ctx, expected| {
        // Build a string the way Value::string does, but through the
        // ctx: payload slice + header object, both in the ctx region.
        let bytes = ctx.alloc_slice::<u8>(b"spec-pin");
        let traits =
            crate::primitives::traitregistry::default_traits_for(ctx.heap_mut(), HeapTag::LString);
        let v = ctx.alloc(HeapObject::LString { s: bytes, traits });
        assert_eq!(
            crate::value::arena::region_of(ctx.heap_mut(), v),
            Some(expected),
            "slice-backed object must live in the ctx's region",
        );
        assert_eq!(
            v.with_string(|s| s.to_string()).as_deref(),
            Some("spec-pin")
        );
    });
}

/// Run `f` with a ctx over its OWN region `ctx_region`, while a SEPARATE
/// region (`other`) also exists on the heap. The contract: every `ctx.*`
/// allocation lands in `ctx_region`. The counterfactual is now a `ctx.*`
/// that minted its own fresh region (like the region-free bare ctor) — its
/// value would land in neither `ctx_region` nor `other`, failing the
/// `Some(ctx_region)` assertion. (Every `ctx.*` lands in the ctx's own region.)
fn with_ctx_over_distinct_region<R>(f: impl FnOnce(&mut Alloc, RuntimeRegion) -> R) -> R {
    let heap_ptr = crate::value::arena::leaked_test_heap();
    let other = unsafe { (*heap_ptr).new_runtime_region() };
    let ctx_region = unsafe { (*heap_ptr).new_runtime_region() };
    assert_ne!(other, ctx_region);
    let out = {
        let mut ctx = Alloc::with_region(ctx_region, unsafe { &mut *heap_ptr });
        f(&mut ctx, ctx_region)
    };
    unsafe {
        (*heap_ptr).decref_region_if_present(other);
        (*heap_ptr).decref_region_if_present(ctx_region);
    }
    out
}

// The `ctx.*` ergonomic constructors (docs/impl/region-ctx.md "the
// body-migration surface") build into the ctx's OWN region — every value is
// born there, not in a fresh per-call region (the region-free bare ctor).
#[test]
fn ctx_string_is_born_in_ctx_region() {
    with_ctx_over_distinct_region(|ctx, ctx_region| {
        let v = ctx.string("ctx-seam");
        assert_eq!(
            crate::value::arena::region_of(ctx.heap_mut(), v),
            Some(ctx_region),
            "ctx.string must allocate into the ctx's own region",
        );
        assert_eq!(
            v.with_string(|s| s.to_string()).as_deref(),
            Some("ctx-seam")
        );
    });
}

#[test]
fn ctx_pair_is_born_in_ctx_region() {
    with_ctx_over_distinct_region(|ctx, ctx_region| {
        let v = ctx.pair(Value::int(1), Value::NIL);
        assert_eq!(
            crate::value::arena::region_of(ctx.heap_mut(), v),
            Some(ctx_region)
        );
    });
}

#[test]
fn ctx_array_is_born_in_ctx_region() {
    with_ctx_over_distinct_region(|ctx, ctx_region| {
        let v = ctx.array(vec![Value::int(1), Value::int(2)]);
        assert_eq!(
            crate::value::arena::region_of(ctx.heap_mut(), v),
            Some(ctx_region)
        );
    });
}

/// A `boundary` ctx (trait dispatch / WASM host) mints its OWN fresh result
/// region and allocates into it. Build one while a *distinct* region also
/// exists and assert the allocation lands in the ctx's own minted region.
#[test]
fn boundary_ctx_allocates_into_its_own_minted_region() {
    let heap_ptr = crate::value::arena::leaked_test_heap();
    let other = unsafe { (*heap_ptr).new_runtime_region() };
    let (own, v) = {
        let ctx = Alloc::boundary(unsafe { &mut *heap_ptr });
        let own = ctx.test_region();
        let v = ctx.array(vec![Value::int(1), Value::int(2)]);
        (own, v)
    };
    assert_ne!(
        own, other,
        "boundary must mint its own region, distinct from any other region"
    );
    assert_eq!(
        crate::value::arena::region_of(unsafe { &mut *heap_ptr }, v),
        Some(own),
        "boundary ctx.array must allocate into the ctx's own minted region",
    );
    unsafe {
        (*heap_ptr).decref_region_if_present(other);
        (*heap_ptr).decref_region_if_present(own);
    }
}

/// `ctx.*` births on the ctx's OWN heap. Install heap A as the root heap,
/// build a ctx over a DISTINCT heap B, and assert `ctx.string` grows B's
/// object count while A's is untouched — the contract that the ctx is the
/// heap capability: every `ctx.*` allocation lands on the ctx's own heap,
/// not whichever heap is installed as the root.
///
/// Object count is the heap-discriminating signal — `region_of_ptr` reads
/// the region id stamped in the page header and so cannot tell the two heaps
/// apart (both mint region id 2), but each heap counts only its own allocs.
#[test]
fn ctx_allocates_on_its_own_heap_not_the_tls_heap() {
    let heap_a = crate::value::arena::leaked_test_heap();
    let a_before = unsafe { (*heap_a).len() };
    let mut heap_b = crate::value::FiberHeap::new();
    let region_b = heap_b.new_runtime_region();
    let b_before = heap_b.len();
    {
        let ctx = Alloc::with_region(region_b, &mut heap_b);
        let _v = ctx.string("own-heap");
    }
    assert!(
        heap_b.len() > b_before,
        "ctx.string must allocate on the ctx's own heap B (B's count must grow)",
    );
    assert_eq!(
        unsafe { (*heap_a).len() },
        a_before,
        "ctx.string must NOT allocate on the installed root heap A (A unchanged)",
    );
    heap_b.decref_region_if_present(region_b);
}
