//! Tests for FiberHeap.

use super::*;
use crate::hir::region::RuntimeRegion;
use crate::value::heap::{HeapObject, Pair};

/// Wrap a raw id as a `RuntimeRegion` for tests (panics on 0).
fn rr(n: u32) -> RuntimeRegion {
    RuntimeRegion::new(n).unwrap()
}

#[test]
fn test_fiber_heap_alloc_in_region() {
    let mut heap = FiberHeap::new();
    let v = heap.alloc_in_region(
        HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)),
        rr(2),
    );
    assert_eq!(heap.len(), 1);
    assert!(v.is_heap());
}

#[test]
fn test_fiber_heap_clear() {
    let mut heap = FiberHeap::new();
    heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)), rr(2));
    heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)), rr(2));
    heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)), rr(3));
    assert_eq!(heap.len(), 3);
    heap.clear();
    assert_eq!(heap.len(), 0);
    assert!(heap.is_empty());
}

#[test]
fn test_fiber_heap_needs_drop_exhaustive() {
    assert!(!needs_drop(HeapTag::Pair));
    assert!(!needs_drop(HeapTag::Float));
    assert!(!needs_drop(HeapTag::LibHandle));
    assert!(!needs_drop(HeapTag::ManagedPointer));
    assert!(!needs_drop(HeapTag::Parameter));

    assert!(needs_drop(HeapTag::LBox));
    assert!(needs_drop(HeapTag::CaptureCell));
    assert!(needs_drop(HeapTag::LString));
    assert!(needs_drop(HeapTag::LArrayMut));
    assert!(needs_drop(HeapTag::LStructMut));
    assert!(needs_drop(HeapTag::LStruct));
    assert!(needs_drop(HeapTag::Closure));
    assert!(needs_drop(HeapTag::LArray));
    assert!(needs_drop(HeapTag::LStringMut));
    assert!(needs_drop(HeapTag::LBytes));
    assert!(needs_drop(HeapTag::LBytesMut));
    assert!(needs_drop(HeapTag::Syntax));
    assert!(needs_drop(HeapTag::Fiber));
    assert!(needs_drop(HeapTag::ThreadHandle));
    assert!(needs_drop(HeapTag::FFISignature));
    assert!(needs_drop(HeapTag::FFIType));
    assert!(needs_drop(HeapTag::External));
    assert!(needs_drop(HeapTag::LSet));
    assert!(needs_drop(HeapTag::LSetMut));
}

#[test]
fn free_region_physical_frees_matching_slots() {
    let mut heap = FiberHeap::new();
    heap.alloc_in_region(
        HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)),
        rr(4),
    );
    let v2 = heap.alloc_in_region(
        HeapObject::Pair(Pair::new(Value::int(2), Value::NIL)),
        rr(2),
    );
    assert!(v2.is_heap());
    heap.decref_region_if_present(rr(4));
    heap.decref_region_if_present(rr(2));
}

#[test]
#[should_panic(expected = "stale region")]
fn region_of_panics_on_stale_value_in_debug() {
    // The arena-level funnel (docs/impl/region/generations.md § "Region generations"):
    // every runtime RC decision reads a value's region through
    // arena::region_of, so the generation check there converts a stale-id
    // deref from a silent wrong read into a deterministic panic.
    let mut heap = FiberHeap::new();
    let r = heap.new_runtime_region();
    let v = heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)), r);
    heap.decref_region(r); // rc 1→0: region freed, page cached stale-but-intact
    let _ = crate::value::arena::region_of(&heap, v);
}

#[test]
#[should_panic(expected = "stale region")]
fn pass_through_borrow_detonates_at_region_of() {
    // The pass-through borrow's check is `region_of` itself — NOT a
    // recorded-generation handle like the cross-fiber param snapshot
    // (docs/impl/region/generations.md § "Two borrow shapes"). The
    // `%first`/`%rest`/`%get` intrinsics (`LirInstr::First`/`Rest`/`Get`) hand back
    // a value that aliases into the *source* collection's region with NO incref —
    // an uncounted borrow (unlike a *native* `first`/`rest`/`get`, whose result the
    // pass-through retain in `dispatch_native_call` counts). Such a borrow is a
    // transient SSA value with a compile-time-bounded lifetime and no persistent
    // home to record a handle on; it is sound only while its source region outlives
    // it, which the region solver upholds by lifetime extension. This pins the
    // runtime backstop: a borrow held past its source region's free detonates at the
    // borrow's next `region_of` (the page-stamp generation check), deterministically
    // at the deref rather than as a later silent wrong read.
    //
    // Counterfactual: without the generation check, `region_of` would hand back the
    // freed region's id (recycled or stale) and the dangling borrow would read on.
    let mut heap = FiberHeap::new();
    // The collection and a co-located element share one region `r` — a `%pair`'s car
    // living in the pair's own region. Same-region allocations add no RC, so `r`'s
    // rc is 1: the collection's single owning reference.
    let r = heap.new_runtime_region();
    let _collection =
        heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)), r);
    // `(%get collection k)` / `(%first collection)` yields this co-located element as
    // an uncounted borrow into `r` — modelled by simply holding the Value: nothing
    // increfs `r`.
    let borrowed = heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::int(2), Value::NIL)), r);
    // The collection's owner releases `r` at its decref_point (rc 1→0: freed, pages
    // cached stale-but-intact). The borrow now dangles.
    heap.decref_region(r);
    // The dangling borrow's next region-classifying deref detonates here.
    let _ = crate::value::arena::region_of(&heap, borrowed);
}

// The `ensure_raw` backstop (docs/impl/region/generations.md § "Region
// generations"): a garbage region id must NOT drive the lazy region-table
// resize. A corrupt page-header read (the diagnosed cause was a misidentified
// page base — closed by the `region_of_ptr` ownership-validated walk and the
// page-header magic — but a stale/foreign read could also reach here) can hand
// back an id near `u32::MAX`. Without the backstop `ensure_raw` resizes the
// table to that id (~584 GB) and OOM-aborts far from the bug. With it, the
// implausible id detonates at the incref, naming the hazard, in every build (the
// failure observed as `elle test tests/elle/oracle.lisp` aborting with `memory
// allocation of 584115526960 bytes failed`).
//
// Counterfactual: pre-backstop this call resizes `regions` to ~u32::MAX entries
// (an OOM); post-backstop it panics before any resize.
#[test]
#[should_panic(expected = "physically implausible")]
fn incref_on_implausible_region_id_detonates_not_resizes() {
    let mut heap = FiberHeap::new();
    heap.incref_region(rr(u32::MAX));
}

// `RuntimeRegion` is mortal by construction: ids 0 and 1 are reserved and
// *unrepresentable* as a `RuntimeRegion`, so the runtime decref/alloc paths
// cannot be called with either — a compile error, not a runtime assert.
#[test]
fn region_zero_and_one_are_unrepresentable() {
    assert!(RuntimeRegion::new(0).is_none());
    assert!(
        RuntimeRegion::new(1).is_none(),
        "id 1 is reserved — never a freeable RuntimeRegion",
    );
    assert_eq!(RuntimeRegion::new(2).map(|r| r.get()), Some(2));
    assert_eq!(RuntimeRegion::new(7).map(|r| r.get()), Some(7));
}

// Regression: the macro-transformer-cache use-after-free demonstrated by
// `demos/fib/fib.lisp` — intermittent startup panic
// `Macro 'error': transformer is not a closure`.
//
// A transient region (the per-compilation/per-expansion scratch region built
// by `pipeline::compile::with_transient` and `expand_macro_call`) must mint its
// physical region id from the per-heap `new_runtime_region` pool — the single
// physical-region allocator that every runtime allocation uses — NOT from the
// global `new_static_region()` counter. The two counters (`NEXT_STATIC_REGION` in
// lir/lower vs `RegionStore::next_physical`) are independent yet both
// index the same `RegionStore`, so a transient's `new_static_region()`
// value can equal a LIVE runtime region's `new_runtime_region()` id. The
// transient's `decref_region_if_present` then frees that live region — a
// use-after-free that violates docs/impl/region/rules.md invariant #1 ("no freeing
// while RC > 0"). A cached macro transformer closure lives in such a
// runtime region; when a later macro expansion's transient collides with
// it, the cached closure's region is freed and recycled, and the next
// lookup derefs a non-closure.
//
// Counterfactual: force the EXACT collision. Make a live region whose id
// equals the value the next `new_static_region()` will return — i.e. the id
// a pre-fix transient mints. Pre-fix the transient frees this live region
// (its RC drops to 0); post-fix the transient draws from the per-heap
// `new_runtime_region` pool and cannot pick this id, so the live region
// survives. This is the fib UAF in miniature.
#[test]
fn transient_does_not_free_a_live_region_sharing_its_id() {
    let mut heap = FiberHeap::new();

    // `new_static_region()` returns the current global counter then advances,
    // so the *next* call returns `g + 1`. Nothing between here and the
    // transient calls it, so a pre-fix transient mints exactly `g + 1`.
    let g = crate::lir::lower::new_static_region();
    let colliding = rr(g.get() + 1);
    heap.alloc_in_region(
        HeapObject::Pair(Pair::new(Value::int(7), Value::NIL)),
        colliding,
    );
    assert_eq!(heap.region_rc(colliding), 1);

    // The transient minted from the per-heap pool, allocated into, then freed —
    // exactly what `with_transient` / `expand_macro_call` do now (no macro).
    {
        let rid = heap.new_runtime_region();
        heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)), rid);
        heap.decref_region_if_present(rid);
    }

    assert_eq!(
        heap.region_rc(colliding),
        1,
        "a transient region freed a LIVE region sharing its id — the \
         macro-transformer-cache UAF: a transient drew its id from the global \
         new_static_region() counter, colliding with a live runtime region \
         (docs/impl/region/rules.md invariant #1: no freeing while RC > 0)"
    );
}

// Allocator invariant: `new_runtime_region` must never hand out an id that
// already names a LIVE region. Physical ids reach the RegionStore from two
// independent sources — this per-heap counter AND raw `new_static_region()`
// static-slot ids that some paths use directly (or that `incref`/`ensure`
// re-animate after a premature free). The ranges overlap, so `next_physical`
// can climb onto a still-live region; issuing it would alias two logical
// regions onto one id → use-after-free (demos/fib/fib.lisp's torn-read abort
// during macro expansion). new_runtime_region must skip any live id.
#[test]
fn new_runtime_region_never_reissues_a_live_region() {
    let mut heap = FiberHeap::new();

    // A region created directly at an id `next_physical` (starts at 2) will
    // climb into — models a raw static-slot id used as a physical region.
    let raw = rr(64);
    heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)), raw);
    assert_eq!(heap.region_rc(raw), 1, "the raw region must be live");

    // Minting past `raw` must never re-issue it while it is live.
    for _ in 0..256 {
        let id = heap.new_runtime_region();
        assert_ne!(
            id, raw,
            "new_runtime_region re-issued LIVE region {raw} — aliasing two logical \
             regions onto one id is a use-after-free"
        );
    }
    // The live region is untouched.
    assert_eq!(heap.region_rc(raw), 1);
}

// Proof of the single-allocator property that makes the collision
// impossible: a transient must mint its physical id from the per-heap
// `new_runtime_region` pool, so a freed id is recycled and the next transient
// reuses it. Advancing the global counter first decouples it from the
// small per-heap id, making the counterfactual order-independent.
#[test]
fn transient_region_id_comes_from_heap_pool_not_global_counter() {
    let mut heap = FiberHeap::new();

    // Allocate into a physical region, then free it so its id is recycled
    // onto the heap's free-list (free_runtime_region_pages pushes ids >= 2).
    let recycled = heap.new_runtime_region();
    heap.alloc_in_region(
        HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)),
        recycled,
    );
    heap.decref_region_if_present(recycled);

    // Push the global counter well past the small per-heap id so a pre-fix
    // transient (drawing from it) cannot coincidentally equal `recycled`.
    for _ in 0..8 {
        let _ = crate::lir::lower::new_static_region();
    }

    // The transient must reuse the recycled id, proving it draws from the
    // per-heap physical pool. A pre-fix transient drawing a global
    // `new_static_region()` value would never equal the recycled id.
    let tid = heap.new_runtime_region();
    assert_eq!(
        tid, recycled,
        "a transient region must mint from the per-heap physical pool \
         (got {tid}, expected recycled heap id {recycled}); minting from the \
         global new_static_region() counter collides with live runtime regions"
    );
}

// A closure built by SHARING another closure's environment has its env
// RegionSlice *backing* in the SOURCE closure's region, not its own. The
// canonical producers are `squelch` and `attune` (src/primitives/meta.rs),
// which build `Closure { template, env: src.env, .. }` — the comment there
// notes "RegionSlice copy is a (ptr, len) pair", i.e. the backing stays put.
//
// That is a Rule-5 cross-region escape: the new closure (in region B) holds a
// reference into region A (the source's env backing). The alloc-time scan
// (`find_object_cross_refs`, Closure arm) MUST incref A — otherwise A is freed
// at its owning-scope decref while the new closure still reads its env. The
// observed symptom is the protect+squelch+nested-yield hang: `populate_env`
// reads a freed page on first fiber resume (b.lisp / signals.lisp), because
// `safe = (squelch outer …)` shares `outer`'s env and `outer`'s region is
// released at rc=1 right after the `def`.
//
// Counterfactual: pre-fix the Closure arm only scans the env *Values*, never
// the env backing, so `region_rc(A)` stays 1 after the closure alloc and the
// owning decref frees A out from under the live closure. The Fiber arm already
// does this (Fix 1's "EXPERIMENT"); closures need the same edge.
#[test]
fn closure_sharing_env_increfs_the_env_backing_region() {
    use crate::value::fiber::SignalBits;
    use crate::value::{Arity, Closure, ClosureTemplate};
    use std::rc::Rc;

    let mut heap = FiberHeap::new();
    let region_a = rr(2); // where the shared env backing lives (the "outer" region)
    let region_b = rr(3); // where the squelch-style closure that shares it lives

    // The env backing must be a real page in A (non-empty: an empty env uses a
    // dangling sentinel ptr that belongs to no region).
    let env = heap.alloc_region_slice_in_region(&[Value::int(1)], region_a);
    assert_eq!(
        heap.region_rc(region_a),
        1,
        "A owns its env backing slice (rc=1, the owning-scope ref)"
    );

    let template = Rc::new(ClosureTemplate::new(
        Rc::new(Vec::new()),
        Arity::Exact(0),
        Rc::new(Vec::new()),
    ));
    // Mirror prim_squelch: a NEW closure that SHARES the env (backed in A) but
    // is itself allocated into a different region B.
    let shared = Closure {
        template: crate::value::TemplateRef::new(template),
        env,
        squelch_mask: SignalBits::EMPTY,
    };
    heap.alloc_in_region(
        HeapObject::Closure {
            closure: shared,
            traits: Value::NIL,
        },
        region_b,
    );

    assert_eq!(
        heap.region_rc(region_a),
        2,
        "allocating a closure whose env backing lives in region A must incref A \
         (the env-sharing cross-region escape); without it A is freed under the \
         live closure — the squelch/protect env UAF"
    );

    // The owning-scope decref of A must then leave it alive (the closure in B
    // still references the env backing). Pre-fix this frees A (rc 1 → 0).
    heap.decref_region(region_a);
    assert!(
        heap.region_rc(region_a) >= 1,
        "region A freed while a live closure still shares its env backing — UAF"
    );
}
