// audited: 2026-09-09
// Freeing a root frees every region its subtree owns, however deep, and
// whether or not the root holds pages of its own.
//
// docs/impl/region/owner.md
//
// This is the structural reclamation a per-region count cannot do alone: an
// interior cycle holds every count above zero, and only a walk from the root
// reaches it. Each test here is a counter-factual against a link that records
// the edge and frees nothing through it.

use super::*;

/// Freeing the root subtree-drops its owned child: the root's single decref
/// frees the whole subtree. (RED against link-only: without subtree drop the
/// child survives the root's free.)
#[test]
fn subtree_drop_frees_owned_child_at_root() {
    let mut store = RegionStore::default();
    let parent = store.new_runtime_region();
    let child = store.new_runtime_region();
    store.alloc_obj(parent, cons_obj());
    store.alloc_obj(child, cons_obj());
    store.adopt_region(parent, child);

    store.decref(parent); // root rc 1→0 → free → subtree drop
    assert_eq!(store.region_obj_count(parent), 0, "root freed");
    assert_eq!(
        store.region_obj_count(child),
        0,
        "owned child freed by the root's subtree drop"
    );
}

/// Subtree drop is recursive: a grandchild owned through an interior child frees
/// with the root, so an arbitrarily deep owned subtree reclaims as a unit.
#[test]
fn subtree_drop_is_recursive() {
    let mut store = RegionStore::default();
    let root = store.new_runtime_region();
    let child = store.new_runtime_region();
    let grand = store.new_runtime_region();
    store.alloc_obj(root, cons_obj());
    store.alloc_obj(child, cons_obj());
    store.alloc_obj(grand, cons_obj());
    store.adopt_region(root, child);
    store.adopt_region(child, grand);

    store.decref(root);
    assert_eq!(store.region_obj_count(root), 0);
    assert_eq!(store.region_obj_count(child), 0);
    assert_eq!(
        store.region_obj_count(grand),
        0,
        "a grandchild frees with the root's subtree drop (recursive)"
    );
}

/// An interior cycle reclaims with the subtree drop: two owned children that
/// reference each other (the `(push a b)(push b a)` knot interior to one owned
/// subtree) free with the root, where the per-region RC cascade alone would
/// strand them (each holds the other at rc>0 forever). The cross-references make
/// the cascade fire; the frozen RC absorbs it and subtree drop frees both.
#[test]
fn subtree_drop_reclaims_interior_cycle() {
    let mut store = RegionStore::default();
    let root = store.new_runtime_region();
    let a = store.new_runtime_region();
    let b = store.new_runtime_region();
    // a holds b and b holds a — a mutable reference cycle interior to the subtree.
    let a_val = store.alloc_obj(a, cons_obj());
    let b_val = store.alloc_obj(b, cons_obj());
    store.alloc_obj(
        a,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![b_val])),
            traits: Value::NIL,
        },
    );
    store.alloc_obj(
        b,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![a_val])),
            traits: Value::NIL,
        },
    );
    store.adopt_region(root, a);
    store.adopt_region(root, b);

    store.decref(root);
    assert_eq!(
        store.region_obj_count(a),
        0,
        "interior cycle member a freed"
    );
    assert_eq!(
        store.region_obj_count(b),
        0,
        "interior cycle member b freed"
    );
}

/// Two-phase subtree drop: an owned interior cycle whose members ALSO reference a
/// **Shared** (non-member) region. The drop must reclaim the interior a↔b cycle AND
/// cascade-decref the genuinely-Shared frontier ref exactly once — `shared` survives
/// on its own scope ref, neither freed by the subtree drop nor double-decref'd.
///
/// This pins the four-phase order (unindex-all → scan-all-for-frontier → teardown-all →
/// cascade-frontier). A member's cross-ref scan must run while every sibling's pages are
/// still mapped: the prior one-member-at-a-time order read a freed sibling's returned
/// page — a use-after-free `--trace=guardfree` detonates (debug tolerated it as a
/// stale-but-mapped read, so the UAF's authoritative oracle is the guardfree run of the
/// interior-cycle Elle shape, not this debug-build pin). What this pin guards is that the
/// two-phase refactor still collects and cascades the Shared frontier (a regression that
/// dropped frontier refs would leave `rc(shared)` at 2).
#[test]
fn subtree_drop_cascades_shared_frontier_not_interior_cycle() {
    let mut store = RegionStore::default();
    let root = store.new_runtime_region();
    let a = store.new_runtime_region();
    let b = store.new_runtime_region();
    let shared = store.new_runtime_region(); // a Shared frontier region, NOT owned
    let shared_val = store.alloc_obj(shared, cons_obj()); // rc(shared)=1
    let a_val = store.alloc_obj(a, cons_obj());
    let b_val = store.alloc_obj(b, cons_obj());
    // a holds b (interior); b holds a (interior cycle) AND shared_val (frontier ref).
    store.alloc_obj(
        a,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![b_val])),
            traits: Value::NIL,
        },
    );
    store.alloc_obj(
        b,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![a_val, shared_val])),
            traits: Value::NIL,
        },
    );
    // b's array auto-increfs its cross-region refs: a (interior) and shared (frontier).
    assert_eq!(
        store.rc(shared),
        2,
        "b's array holds a frontier ref to shared"
    );
    store.adopt_region(root, a);
    store.adopt_region(root, b);

    store.decref(root); // subtree drop: free root, a, b as a unit
    assert_eq!(
        store.region_obj_count(a),
        0,
        "interior cycle member a freed"
    );
    assert_eq!(
        store.region_obj_count(b),
        0,
        "interior cycle member b freed"
    );
    assert_eq!(
        store.rc(shared),
        1,
        "the Shared frontier ref is cascade-decref'd exactly once (2→1); the interior \
         a↔b refs are reclaimed by the drop, never cascaded"
    );
    assert_eq!(
        store.region_obj_count(shared),
        1,
        "the Shared region is outside the subtree and survives on its scope ref"
    );
}

/// An owner NODE — a pages-less region minted purely as a forest root, with no
/// allocation ever targeting it (docs/impl/region/owner.md § "Owner nodes — an
/// activation as a forest root") — adopts members exactly as a pages-owning
/// parent does, and its single decref subtree-drops them all. Pins the owner-node
/// substrate: `adopt_region` `ensure`s the node's (empty) entry, the members move
/// `Counted → Owned` (count consumed), and the node's rc 1→0 free returns zero
/// pages of its own while reclaiming every member.
#[test]
fn pages_less_owner_node_subtree_drops_members() {
    let mut store = RegionStore::default();
    let node = store.new_runtime_region(); // never allocated into
    let a = store.new_runtime_region();
    let b = store.new_runtime_region();
    store.alloc_obj(a, cons_obj());
    store.alloc_obj(b, cons_obj());
    store.adopt_region(node, a);
    store.adopt_region(node, b);
    assert_eq!(store.rc(a), 0, "an adopted member is Owned — no count");
    assert_eq!(store.rc(b), 0, "an adopted member is Owned — no count");
    assert_eq!(
        store.region_obj_count(node),
        0,
        "the node owns no objects of its own"
    );

    store.decref(node); // node rc 1→0 → free: zero own pages + both members
    assert_eq!(
        store.region_obj_count(a),
        0,
        "member a freed by the node's subtree drop"
    );
    assert_eq!(
        store.region_obj_count(b),
        0,
        "member b freed by the node's subtree drop"
    );
    assert_eq!(
        store.rc(node),
        0,
        "the node's entry is consumed by its free"
    );
}

/// The region gauge sees a LIVE owner node. `active_region_count` — the backend
/// of `arena/region-count` — counts entries, so the node's `ensure`d entry is
/// counted for exactly as long as the node lives, even though the node holds no
/// object and claims no page (docs/impl/region/owner.md § "Owner nodes — an
/// activation as a forest root"). That inclusion is what makes the region gauge
/// the object gauge's dual: an activation's owner node is a strand
/// `arena/count` cannot see at all.
///
/// The counter-factual is a filter on object count or on pages, which is
/// representable and which every pin in this file survives: the drop pin
/// (`pages_less_owner_node_subtree_drops_members`) reads the node after it is
/// gone, and `reparent_degenerate_cases_are_noops` reads an id that never
/// adopted and so has no entry under either reading. Measured, each filter
/// detonates exactly one other test — the JIT helper's
/// `adopt_into_activation_adopts_into_lazily_minted_node`, whose subject is the
/// adopt rather than the gauge: it reads a release DELTA of 2 and blames a
/// failed adopt for a 1, so a maintainer who narrowed the gauge would go
/// hunting in the helper. Naming the reading here is what points at the gauge.
///
/// Each of the three assertions below is needed: the first fixes what the
/// entry-less baseline is, the second is the live reading a filter would blind,
/// and the third proves the reading tracks the node's demise rather than
/// standing at a constant.
#[test]
fn pages_less_owner_node_counts_in_active_region_count() {
    let mut store = RegionStore::default();
    let base = store.active_region_count();
    let node = store.new_runtime_region(); // never allocated into
    let member = store.new_runtime_region();
    store.alloc_obj(member, cons_obj());
    assert_eq!(
        store.active_region_count(),
        base + 1,
        "minting the node's id materializes no entry — only the member has one"
    );

    store.adopt_region(node, member);
    assert_eq!(
        store.region_obj_count(node),
        0,
        "the node owns no object of its own"
    );
    assert_eq!(
        store.active_region_count(),
        base + 2,
        "the adopt's `ensure` mints the node's entry and the gauge counts it while \
         both are LIVE: one entry for the member, one for the pages-less node"
    );

    store.decref(node); // rc 1→0 → subtree drop over node + member
    assert_eq!(
        store.active_region_count(),
        base,
        "the drop returns both entries, so the gauge tracks the node's whole life"
    );
}

/// An interior reference cycle whose members are adopted by a pages-less owner
/// node reclaims with the node's drop — the `(push a b)(push b a)` knot per-region
/// RC cannot collect (region/rules.md Rule 8), rooted at an owner that owns no
/// pages itself. The frozen member RCs absorb the cascade's interior decrefs and
/// the node's single decref frees the whole set.
#[test]
fn interior_cycle_in_owner_node_reclaims() {
    let mut store = RegionStore::default();
    let node = store.new_runtime_region(); // pages-less root
    let a = store.new_runtime_region();
    let b = store.new_runtime_region();
    // a holds b and b holds a — a mutable reference cycle interior to the node.
    let a_val = store.alloc_obj(a, cons_obj());
    let b_val = store.alloc_obj(b, cons_obj());
    store.alloc_obj(
        a,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![b_val])),
            traits: Value::NIL,
        },
    );
    store.alloc_obj(
        b,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![a_val])),
            traits: Value::NIL,
        },
    );
    store.adopt_region(node, a);
    store.adopt_region(node, b);

    store.decref(node);
    assert_eq!(
        store.region_obj_count(a),
        0,
        "interior cycle member a freed with the node"
    );
    assert_eq!(
        store.region_obj_count(b),
        0,
        "interior cycle member b freed with the node"
    );
}

/// Subtree drop bumps each freed child's generation, exactly as an ordinary
/// RC-zero free does — so a stale pointer into a subtree-dropped child detonates
/// at the next debug `region_of` (docs/impl/region/generations.md), not silently.
#[test]
fn subtree_drop_bumps_owned_child_generation() {
    let mut store = RegionStore::default();
    let parent = store.new_runtime_region();
    let child = store.new_runtime_region();
    store.alloc_obj(parent, cons_obj());
    store.alloc_obj(child, cons_obj());
    store.adopt_region(parent, child);
    assert_eq!(store.generation_raw(child.get()), 0);

    store.decref(parent); // subtree drop frees child
    assert_eq!(
        store.generation_raw(child.get()),
        1,
        "subtree drop must bump the owned child's generation"
    );
}
