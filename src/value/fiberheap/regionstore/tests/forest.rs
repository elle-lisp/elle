use super::*;

// ── The ownership forest: adoption + subtree drop ───────────────────────────
//
// docs/impl/region/ownership.md § "Adoption and subtree drop". `adopt_region(parent,
// child)` links `child` as an Owned member of `parent`'s subtree, freezing the
// child's RC (owned ⇒ RC frozen). Freeing the root subtree-drops every owned
// member recursively — the structural reclamation the per-region RC cascade
// cannot do for an interior cycle. These pins are written from that spec: each
// behaviour is a counterfactual against the per-region-RC baseline (the
// `unadopted_*` control), and against the link-only intermediate (an adopt that
// records the edge but neither freezes nor subtree-drops).

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

// ── The external-reference rescue ────────────────────────────────────────────
//
// docs/impl/region/ownership.md § "The incoming edge table and the external-
// reference rescue". A subtree drop tears down only members that are externally
// unique AT THE DROP: a non-root member whose recorded `incoming` table names a
// source surviving the drop is rescued — moved `Owned → Counted` with a count
// rebuilt from its recorded incoming edges — and frees at its last referencer's
// release instead. The e2e witness is the guardfree fixture pin
// `region_capture_cell_member_cascade_uaf` (tests/integration/elle_scripts.rs).

/// A member referenced from OUTSIDE the dying subtree (a live container holds a
/// value in it — the capture-cell shape) survives the root's drop and frees at
/// the external holder's release. Counterfactual: without the rescue the drop
/// tears the member down under the live external reference (`region_obj_count`
/// reads 0 right after the root's decref) and the holder's later cascade decref
/// lands on freed/recycled state.
#[test]
fn externally_referenced_member_is_rescued_from_subtree_drop() {
    let mut store = RegionStore::default();
    let parent = store.new_runtime_region();
    let member = store.new_runtime_region();
    let cell = store.new_runtime_region(); // the external holder — NOT in the subtree
    let member_val = store.alloc_obj(member, cons_obj());
    // The external holder stores the member's value BEFORE the adopt (records
    // cell → member and increfs, exactly as the capture-store funnel does).
    store.alloc_obj(
        cell,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![member_val])),
            traits: Value::NIL,
        },
    );
    // The parent holds the member too (the closure-env shape), then adopts it.
    store.alloc_obj(
        parent,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![member_val])),
            traits: Value::NIL,
        },
    );
    store.adopt_region(parent, member);

    store.decref(parent); // subtree drop — must rescue the member, not tear it down
    assert_eq!(store.region_obj_count(parent), 0, "the root itself freed");
    assert_eq!(
        store.region_obj_count(member),
        1,
        "an externally-referenced member is rescued: it leaves the forest instead \
         of being freed under the live external reference"
    );
    assert_eq!(
        store.rc(member),
        1,
        "the rescued count is rebuilt from the recorded incoming edges: the dying \
         parent's edge is consumed by its own frontier decref in the same drop, \
         leaving exactly the external holder's reference"
    );

    store.decref(cell); // the external holder's release is now the member's last
    assert_eq!(
        store.region_obj_count(member),
        0,
        "the rescued member frees at the external holder's release (ordinary cascade)"
    );
}

/// The rescue covers the store-AFTER-adopt interleaving too: an external
/// reference recorded while the member is already `Owned` (the incref is inert on
/// the frozen mode, but the edge IS recorded) still rescues the member at the
/// drop. This is what an adopt-time-only guard could never see.
#[test]
fn external_reference_recorded_after_adopt_still_rescues() {
    let mut store = RegionStore::default();
    let parent = store.new_runtime_region();
    let member = store.new_runtime_region();
    let cell = store.new_runtime_region();
    let member_val = store.alloc_obj(member, cons_obj());
    store.alloc_obj(
        parent,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![member_val])),
            traits: Value::NIL,
        },
    );
    store.adopt_region(parent, member);
    // The external store happens AFTER the adopt: incref is a frozen no-op, the
    // content edge is recorded regardless.
    store.alloc_obj(
        cell,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![member_val])),
            traits: Value::NIL,
        },
    );

    store.decref(parent);
    assert_eq!(
        store.region_obj_count(member),
        1,
        "a post-adopt external reference rescues the member at the drop"
    );
    store.decref(cell);
    assert_eq!(
        store.region_obj_count(member),
        0,
        "the rescued member frees at the external holder's release"
    );
}

/// The rescued count excludes the member's OWN subtree's back-edges: a grandchild
/// that back-references the rescued member stays owned beneath it (it releases
/// only at the member's own drop, so counting it would self-sustain the count and
/// leak the pair). The external holder's release must free the member AND
/// subtree-drop the grandchild.
#[test]
fn rescued_count_excludes_own_subtree_back_edges() {
    let mut store = RegionStore::default();
    let root = store.new_runtime_region();
    let member = store.new_runtime_region();
    let grand = store.new_runtime_region();
    let cell = store.new_runtime_region();
    let member_val = store.alloc_obj(member, cons_obj());
    let grand_val = store.alloc_obj(grand, cons_obj());
    // member holds grand (the containment edge)…
    store.alloc_obj(
        member,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![grand_val])),
            traits: Value::NIL,
        },
    );
    // …and grand back-references member (the interior knot).
    store.alloc_obj(
        grand,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![member_val])),
            traits: Value::NIL,
        },
    );
    store.alloc_obj(
        root,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![member_val])),
            traits: Value::NIL,
        },
    );
    store.alloc_obj(
        cell,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![member_val])),
            traits: Value::NIL,
        },
    );
    store.adopt_region(root, member);
    store.adopt_region(member, grand);

    store.decref(root);
    assert_eq!(
        store.region_obj_count(member),
        2,
        "the member is rescued with its own subtree intact"
    );
    assert_eq!(
        store.region_obj_count(grand),
        2,
        "the rescued member's owned grandchild survives with it"
    );
    assert_eq!(
        store.rc(member),
        1,
        "the grandchild's back-edge is excluded from the rescued count — after the \
         dying root's frontier decref only the external holder's reference remains"
    );

    store.decref(cell);
    assert_eq!(
        store.region_obj_count(member),
        0,
        "the external holder's release frees the rescued member…"
    );
    assert_eq!(
        store.region_obj_count(grand),
        0,
        "…and its subtree drop reclaims the back-referencing grandchild with it"
    );
}

/// Rescue iterates to a fixpoint: rescuing one member makes its surviving edges
/// external for a sibling member, which must be rescued too — a sibling freed in
/// the same drop would strand the rescued member's live edge into it.
#[test]
fn rescue_cascades_to_sibling_referenced_by_rescued_member() {
    let mut store = RegionStore::default();
    let root = store.new_runtime_region();
    let m1 = store.new_runtime_region();
    let m2 = store.new_runtime_region();
    let cell = store.new_runtime_region();
    let m1_val = store.alloc_obj(m1, cons_obj());
    let m2_val = store.alloc_obj(m2, cons_obj());
    // m1 holds m2 (a sibling edge inside the subtree).
    store.alloc_obj(
        m1,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![m2_val])),
            traits: Value::NIL,
        },
    );
    store.alloc_obj(
        root,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![m1_val, m2_val])),
            traits: Value::NIL,
        },
    );
    // Only m1 is externally held.
    store.alloc_obj(
        cell,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![m1_val])),
            traits: Value::NIL,
        },
    );
    store.adopt_region(root, m1);
    store.adopt_region(root, m2);

    store.decref(root);
    assert_eq!(
        store.region_obj_count(m1),
        2,
        "the externally-held member is rescued"
    );
    assert_eq!(
        store.region_obj_count(m2),
        1,
        "the sibling the rescued member references is rescued transitively — \
         freeing it in the same drop would strand the survivor's live edge"
    );

    store.decref(cell); // frees m1, whose cascade then frees m2
    assert_eq!(
        store.region_obj_count(m1),
        0,
        "m1 freed at the cell's release"
    );
    assert_eq!(
        store.region_obj_count(m2),
        0,
        "m2 freed by the freed m1's frontier cascade"
    );
}

/// A `moves_out` extract (`%pop` of an adopted element) rebuilds the element's
/// count from its recorded incoming edges, not a bare 1: an element ALSO held by
/// an external container must survive that holder's release while the caller's
/// moves-out reference still stands. Counterfactual: `Counted(1)` lets the
/// external holder's cascade free the element under the caller.
#[test]
fn extract_owned_region_admits_external_references() {
    let mut store = RegionStore::default();
    let container = store.new_runtime_region();
    let elem = store.new_runtime_region();
    let cell = store.new_runtime_region(); // an external holder of the element
    let elem_val = store.alloc_obj(elem, cons_obj());
    store.alloc_obj(
        container,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![elem_val])),
            traits: Value::NIL,
        },
    );
    store.alloc_obj(
        cell,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![elem_val])),
            traits: Value::NIL,
        },
    );
    store.adopt_region(container, elem);

    // The pop funnel's order: un-record the container's edge, then extract.
    store.unrecord_outgoing(container.get(), elem.get());
    store.extract_owned_region(elem);

    store.decref(cell); // the external holder releases its edge
    assert_eq!(
        store.region_obj_count(elem),
        1,
        "the extracted element survives the external holder's release on the \
         caller's moves-out reference"
    );
    store.decref(elem); // the caller's release is the last
    assert_eq!(
        store.region_obj_count(elem),
        0,
        "freed at the caller's release"
    );
}

#[test]
#[should_panic(expected = "stale region")]
fn stale_owned_child_deref_panics_after_subtree_drop() {
    // The owned child's pages are returned by the root's subtree drop; a value
    // that outlived them is a stale deref, caught at region_of like any other.
    let mut store = RegionStore::default();
    let parent = store.new_runtime_region();
    let child = store.new_runtime_region();
    store.alloc_obj(parent, cons_obj());
    let cv = store.alloc_obj(child, cons_obj());
    let cptr = cv.as_heap_ptr().unwrap();
    store.adopt_region(parent, child);
    store.decref(parent); // subtree drop frees child; its page sits stale-but-intact
    let _ = store.region_of_ptr(cptr);
}
