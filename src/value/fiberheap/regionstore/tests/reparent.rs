// audited: 2026-09-09
// Reparenting moves a whole owned set to another owner, and re-stamps each one.
//
// docs/impl/region/ownership.md
//
// The move is what makes it a transfer rather than a copy: afterwards the old
// owner's drop frees only itself, and the new owner's frees every member. The
// re-stamp is what keeps the subtree walk's own check true through the new
// owner, so a transfer that moved the list alone detonates there.

use super::*;

/// `reparent_owned_children` hands the whole owned set to a new owner — the
/// ownership-TRANSFER primitive (docs/impl/region/ownership.md § "The runtime: a
/// reclamation typestate"). Move-only: after the transfer the old owner's drop
/// frees only itself (the members survive it), and the new owner's drop frees
/// every member. Each moved child is re-stamped to record the new owner, so the
/// subtree-drop walk's forward/back-edge debug assert holds through the NEW
/// owner — a transfer that moved the list without re-stamping detonates there.
#[test]
fn reparent_owned_children_moves_the_set() {
    let mut store = RegionStore::default();
    let from = store.new_runtime_region();
    let to = store.new_runtime_region();
    let a = store.new_runtime_region();
    let b = store.new_runtime_region();
    store.alloc_obj(from, cons_obj());
    store.alloc_obj(to, cons_obj());
    store.alloc_obj(a, cons_obj());
    store.alloc_obj(b, cons_obj());
    store.adopt_region(from, a);
    store.adopt_region(from, b);

    store.reparent_owned_children(from, to);

    store.decref(from);
    assert_eq!(
        store.region_obj_count(from),
        0,
        "the emptied old owner freed"
    );
    assert_eq!(
        store.region_obj_count(a),
        1,
        "member a moved — it survives the old owner's drop"
    );
    assert_eq!(
        store.region_obj_count(b),
        1,
        "member b moved — it survives the old owner's drop"
    );

    store.decref(to);
    assert_eq!(
        store.region_obj_count(a),
        0,
        "member a freed by the NEW owner's subtree drop"
    );
    assert_eq!(
        store.region_obj_count(b),
        0,
        "member b freed by the NEW owner's subtree drop"
    );
}

/// The terminal-fiber-teardown shape: two owner nodes' member sets gathered
/// under one pages-less node (the fiber node), each emptied node freed, and the
/// gathering node's single decref reclaiming the whole set as ONE subtree drop
/// (docs/impl/region/owner.md § "Owner nodes" — "Fiber teardown frees everything
/// the fiber owns").
#[test]
fn reparent_gathers_owned_sets_under_pages_less_node() {
    let mut store = RegionStore::default();
    let node_a = store.new_runtime_region(); // a parked activation's node
    let node_b = store.new_runtime_region(); // another parked activation's node
    let fnode = store.new_runtime_region(); // the fiber node — never allocated into
    let ma = store.new_runtime_region();
    let mb = store.new_runtime_region();
    store.alloc_obj(ma, cons_obj());
    store.alloc_obj(mb, cons_obj());
    store.adopt_region(node_a, ma);
    store.adopt_region(node_b, mb);

    store.reparent_owned_children(node_a, fnode);
    store.reparent_owned_children(node_b, fnode);
    store.decref(node_a);
    store.decref(node_b);
    assert_eq!(
        store.region_obj_count(ma),
        1,
        "a gathered member survives its old node's drop"
    );
    assert_eq!(
        store.region_obj_count(mb),
        1,
        "a gathered member survives its old node's drop"
    );

    store.decref(fnode);
    assert_eq!(
        store.region_obj_count(ma),
        0,
        "the gathering node's one drop frees the whole set"
    );
    assert_eq!(
        store.region_obj_count(mb),
        0,
        "the gathering node's one drop frees the whole set"
    );
}

/// Degenerate transfers are no-ops: an absent `from` (an id with no entry), an
/// empty child set, and a self-reparent each change nothing — and a transfer of
/// nothing must not `ensure` (mint an entry for) `to`, so a node id that never
/// adopted stays entry-less and its tolerant decref stays a no-op.
#[test]
fn reparent_degenerate_cases_are_noops() {
    let mut store = RegionStore::default();
    let absent = store.new_runtime_region(); // no entry
    let to = store.new_runtime_region(); // no entry either
    store.reparent_owned_children(absent, to);
    assert_eq!(
        store.active_region_count(),
        0,
        "a transfer from an absent region mints nothing"
    );

    let from = store.new_runtime_region();
    store.alloc_obj(from, cons_obj()); // an entry with no children
    store.reparent_owned_children(from, to);
    assert_eq!(
        store.active_region_count(),
        1,
        "a transfer of an empty child set does not ensure `to`"
    );

    let child = store.new_runtime_region();
    store.alloc_obj(child, cons_obj());
    store.adopt_region(from, child);
    store.reparent_owned_children(from, from); // self-reparent: no-op
    store.decref(from);
    assert_eq!(
        store.region_obj_count(child),
        0,
        "a self-reparent leaves the edge intact — the owner's drop still frees the child"
    );
}
