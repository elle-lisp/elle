use super::*;

#[test]
fn result_region_mints_distinct_reclaimable_regions() {
    let mut vm = VM::new();
    let r1 = vm.result_region();
    let r2 = vm.result_region();
    // Each call mints its own region (no caching), and every `RuntimeRegion`
    // is reclaimable by type (id ≥ 2 — Rule 1).
    assert_ne!(r1, r2, "each result_region() call mints a distinct region");
    assert!(r1.get() >= 2 && r2.get() >= 2);
}

#[test]
fn escaping_error_is_born_in_a_fresh_distinct_region() {
    let mut vm = VM::new();
    // A reference region minted just before. Each escaping error must get its
    // OWN fresh, reclaimable region — never reusing a prior one — so that in a
    // native tail-return the error is not born in a region the tail-call is
    // already freeing.
    let reference = vm.result_region();
    let e1 = vm.escaping_error("test-error", "boom");
    let e2 = vm.escaping_error("test-error", "bang");
    let r1 = crate::value::arena::region_of(unsafe { &mut *vm.heap_ptr }, e1);
    let r2 = crate::value::arena::region_of(unsafe { &mut *vm.heap_ptr }, e2);
    assert!(
        r1.is_some() && r2.is_some(),
        "an escaping error is heap-allocated in a region",
    );
    assert_ne!(
        r1, r2,
        "each escaping error is born in its own fresh region"
    );
    assert_ne!(
        r1,
        Some(reference),
        "escaping_error mints a fresh region, never reusing a prior one",
    );
}
