// audited: 2026-09-09
// A subtree drop spares a member something outside the subtree still refers to.
//
// docs/impl/region/ownership.md
//
// A drop tears down only the members that are externally unique at the moment
// it runs. One whose recorded incoming table names a source surviving the drop
// is rescued instead: moved from Owned back to Counted, with a count rebuilt
// from those edges, so it frees at its last referencer's release. The
// end-to-end witness is the guardfree fixture
// `region_capture_cell_member_cascade_uaf`.

use super::*;

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

// `region_of_ptr` runs the generation check under `cfg!(debug_assertions)`, so
// a release build returns the stale id instead of naming it. The test has to
// go where the check goes.
#[cfg(debug_assertions)]
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
