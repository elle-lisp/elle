// audited: 2026-09-29
//! The adopt and group-free helpers resolve their Values to runtime regions and do what the interpreter's handlers do.
//!
//! docs/impl/region/owner.md

use super::*;
use crate::value::arena::{alloc_in_fresh_region, region_rc};
use crate::value::heap::{HeapObject, Pair};

/// A pair in a fresh region of `vm`'s heap, and that region.
fn fresh_pair(vm: &VM) -> (Value, RuntimeRegion) {
    alloc_in_fresh_region(
        unsafe { &mut *vm.heap_ptr },
        HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)),
    )
}

/// How many regions `vm`'s heap holds live.
fn live_regions(vm: &VM) -> usize {
    unsafe { &*vm.heap_ptr }.active_region_count()
}

/// `elle_jit_adopt_region` mirrors the interpreter's `handle_adopt_region`: it
/// resolves the parent and child Values to their runtime regions and moves the
/// child `Counted → Owned`, freezing its RC, so the parent's later subtree drop
/// reclaims both. Pins both halves: the child's count is *consumed* (region_rc
/// reads 0, an Owned region has no count), and dropping the parent reclaims the
/// parent + the adopted child as one subtree. A no-op (broken) helper would leave
/// the child `Counted(1)` and free only the parent — both assertions catch it.
#[test]
fn adopt_region_freezes_child_and_subtree_drops_with_parent() {
    let mut vm = VM::new();
    let heap_ptr = vm.heap_ptr;
    // Parent and child, each a pair in its own fresh region.
    let (parent, parent_rid) = fresh_pair(&vm);
    let (child, child_rid) = fresh_pair(&vm);
    assert_ne!(parent_rid, child_rid);
    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, child_rid),
        1,
        "fresh child region starts Counted(1)"
    );
    let before = live_regions(&vm);

    elle_jit_adopt_region(
        parent.tag,
        parent.payload,
        child.tag,
        child.payload,
        vm_arg(&mut vm),
    );

    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, child_rid),
        0,
        "adopt moves the child Counted -> Owned, consuming its count (an Owned \
         region has no RC); a no-op helper would leave it at 1",
    );
    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, parent_rid),
        1,
        "the parent stays Counted(1) — only the child is frozen"
    );

    // Dropping the parent (rc 1 -> 0) subtree-drops the adopted child with it.
    unsafe { (*heap_ptr).decref_region(parent_rid) };
    assert_eq!(
        before - live_regions(&vm),
        2,
        "the parent's subtree drop must reclaim parent + adopted child (2 regions); \
         a failed adopt would free only the parent (delta 1)",
    );
}

/// `elle_jit_adopt_into_activation` mirrors the interpreter's
/// `handle_adopt_into_activation`: it resolves the child Value to its runtime
/// region, lazily mints the current activation's pages-less owner node, and
/// moves the child `Counted → Owned` (count consumed). Releasing the node
/// (`VM::release_activation_dues`, the completion free both tiers share)
/// then subtree-drops node + member as one unit. A no-op (broken) helper would
/// leave the child `Counted(1)` and the release would reclaim nothing — both
/// assertions catch it.
#[test]
fn adopt_into_activation_adopts_into_lazily_minted_node() {
    let mut vm = VM::new();
    let heap_ptr = vm.heap_ptr;
    let (child, child_rid) = fresh_pair(&vm);
    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, child_rid),
        1,
        "fresh child region starts Counted(1)"
    );

    elle_jit_adopt_into_activation(child.tag, child.payload, vm_arg(&mut vm));

    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, child_rid),
        0,
        "the adopt moves the child Counted -> Owned, consuming its count; a no-op \
         helper would leave it at 1",
    );

    // The node was lazily minted into the current (base) activation slot;
    // releasing it reclaims node + adopted member as one subtree.
    let before = live_regions(&vm);
    vm.release_activation_dues();
    assert_eq!(
        before - live_regions(&vm),
        2,
        "releasing the owner node must reclaim the node's entry + the adopted \
         member (2 regions); a failed adopt would reclaim only the node (delta 1)",
    );
}

/// An immediate child (no region) must adopt nothing AND mint no node — the
/// lazy mint fires only for a real member, so an activation whose adopts all
/// resolve to immediates pays nothing and its completion release is a no-op.
#[test]
fn adopt_into_activation_immediate_child_mints_no_node() {
    let mut vm = VM::new();
    let before = live_regions(&vm);
    let n = Value::int(42);
    elle_jit_adopt_into_activation(n.tag, n.payload, vm_arg(&mut vm));
    assert!(
        vm.take_activation_dues().is_empty(),
        "an immediate child must not mint an owner node"
    );
    assert_eq!(
        live_regions(&vm),
        before,
        "no region state may change for an immediate child"
    );
}

/// `elle_jit_free_region_group` mirrors the interpreter's
/// `handle_free_region_group`: it resolves each member Value to its runtime
/// region and frees the whole set as one unit, regardless of the members'
/// reference counts. Pins that both members' regions are reclaimed (active
/// region count falls by 2). The cycle reclamation of the underlying
/// `free_region_group` is pinned in
/// `runtime::tests::ownership::subtree::region_ownership_reclaims_bare_cycle_group`;
/// this pins the JIT helper's resolve-and-forward.
#[test]
fn free_region_group_reclaims_member_regions() {
    let mut vm = VM::new();
    let (a, _a_rid) = fresh_pair(&vm);
    let (b, _b_rid) = fresh_pair(&vm);
    let before = live_regions(&vm);

    // The compiled translate arm spills members to a contiguous stack slot as
    // Value pairs; a Rust array of Values is the same layout.
    let members = [a, b];
    elle_jit_free_region_group(members.as_ptr(), 2, vm_arg(&mut vm));

    assert_eq!(
        before - live_regions(&vm),
        2,
        "free_region_group must reclaim both members' regions as one unit",
    );
}
