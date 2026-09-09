// audited: 2026-09-09
// Adoption links a child into a parent's subtree and freezes the child's count.
//
// docs/impl/region/ownership.md
//
// `adopt_region(parent, child)` makes `child` an Owned member of `parent`, and
// an owned region carries no count of its own. Each test here is a
// counter-factual against two baselines: the per-region count with no adoption
// at all, and a link that records the edge without freezing the count.

use super::*;

/// Control (counterfactual anchor): WITHOUT adoption, freeing one region leaves
/// an unrelated region untouched — the per-region-RC baseline. This is what
/// adoption changes; it must stay GREEN before and after the forest lands so the
/// coupling the forest adds is attributable to adoption alone.
#[test]
fn unadopted_region_survives_unrelated_free() {
    let mut store = RegionStore::default();
    let a = store.new_runtime_region();
    let b = store.new_runtime_region();
    store.alloc_obj(a, cons_obj());
    store.alloc_obj(b, cons_obj());
    store.decref(a); // frees a only; b is independent
    assert_eq!(store.region_obj_count(a), 0);
    assert_eq!(
        store.region_obj_count(b),
        1,
        "an unadopted region is independent"
    );
}

/// `owned ⇒ RC frozen`: once adopted, a direct decref of the child is a no-op —
/// the child is reclaimed only by its owner's subtree drop, never by its own RC.
/// (RED against the link-only adopt: without the frozen check, this decref takes
/// the child's rc 1→0 and frees it, so `region_obj_count` would be 0.)
#[test]
fn owned_child_rc_is_frozen() {
    let mut store = RegionStore::default();
    let parent = store.new_runtime_region();
    let child = store.new_runtime_region();
    store.alloc_obj(parent, cons_obj());
    store.alloc_obj(child, cons_obj());
    store.adopt_region(parent, child);

    store.decref(child); // frozen: a no-op
    assert_eq!(
        store.region_obj_count(child),
        1,
        "an owned child's RC is frozen — a direct decref does not free it"
    );
    assert_eq!(store.region_obj_count(parent), 1, "the owner is untouched");
}

/// A region's reclamation mode is a typestate: once adopted it is `Owned`, with
/// **no independent reference count** — the count is *consumed* by the move into the
/// owner's subtree, so "owned-and-RC'd" (a region a stray decref could free out from
/// under the owner's subtree drop) is unrepresentable, not merely guarded
/// (docs/impl/region/ownership.md § "The runtime: a reclamation typestate"). `rc` of an
/// `Owned` region therefore reads 0. Counterfactual: the prior `rc:u32 + owner:Option`
/// pair left the scope count (1) in place after adoption, so this read 1.
#[test]
fn adopted_region_carries_no_independent_count() {
    let mut store = RegionStore::default();
    let parent = store.new_runtime_region();
    let child = store.new_runtime_region();
    store.alloc_obj(parent, cons_obj());
    store.alloc_obj(child, cons_obj()); // child rc = 1 (its scope ref)
    assert_eq!(
        store.rc(child),
        1,
        "before adoption: an ordinary Counted region"
    );

    store.adopt_region(parent, child);
    assert_eq!(
        store.rc(child),
        0,
        "adoption moves the region Counted→Owned, consuming the count — an Owned \
         region has no independent RC, it is reclaimed solely by the owner's subtree drop"
    );
}

/// Adoption consumes **any** prior count, including a cross-reference count: a
/// region another region points at (rc=2) that is then adopted reports 0. This is
/// the dangerous case the typestate forecloses — the cross-ref's later cascade decref
/// cannot drive an independent free of an owned region, because the count it would
/// decrement no longer exists. Counterfactual: the prior pair left rc=2 after adoption
/// (frozen, decref-guarded), not consumed.
#[test]
fn adoption_consumes_cross_reference_count() {
    let mut store = RegionStore::default();
    let parent = store.new_runtime_region();
    let child = store.new_runtime_region();
    let holder = store.new_runtime_region();
    let child_val = store.alloc_obj(child, cons_obj()); // child rc = 1
                                                        // holder's array references child → alloc auto-increfs child to rc = 2.
    store.alloc_obj(
        holder,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![child_val])),
            traits: Value::NIL,
        },
    );
    store.alloc_obj(parent, cons_obj());
    assert_eq!(
        store.rc(child),
        2,
        "child held by holder's cross-region ref"
    );

    store.adopt_region(parent, child);
    assert_eq!(
        store.rc(child),
        0,
        "adoption consumes the cross-reference count too — Owned ⇒ no count at all, \
         so no decref of the cross-ref can ever independently free the owned region"
    );
}
