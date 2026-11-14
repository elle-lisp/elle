// audited: 2026-11-14
//! The interpreter's `DecrefPostponed`: a held postponed slot releases its value and stamps both slots, and an empty one changes nothing.
//!
//! docs/impl/region/operand.md

use super::*;
use crate::hir::region::RuntimeRegion;
use crate::value::arena::alloc_in_fresh_region;
use crate::value::heap::{HeapObject, Pair};
use crate::value::{Arity, CodeBuilder};

/// A pair in a region of its own, which the pair's one reference holds.
fn fresh_pair(vm: &mut VM) -> (Value, RuntimeRegion) {
    let pair = HeapObject::Pair(Pair::new(Value::NIL, Value::NIL));
    alloc_in_fresh_region(vm.heap(), pair)
}

/// Run `DecrefPostponed { slot, route }` on the frame at the stack's base.
fn run(vm: &mut VM, slot: u16, route: u16) {
    let template = CodeBuilder::new(Vec::new(), Arity::Exact(0), Vec::new()).build(vm.heap());
    let bytecode = [slot.to_be_bytes(), route.to_be_bytes()].concat();
    let mut ip = 0;
    handle_decref_postponed(vm, &bytecode, &mut ip, template.locations(), 0);
    assert_eq!(ip, bytecode.len(), "the handler reads both operands");
}

/// The arm copied the release's slot, local 0, into the postponed slot,
/// local 1. The instruction releases the value and stamps both slots, so the
/// release behind the call and the abandoned-frame walk each find `nil`.
///
/// The counter-factual: a handler that left the route standing, so the release
/// behind the call ran a second time on the native fall-through.
#[test]
fn a_held_postponed_slot_releases_its_value_and_stamps_both_slots() {
    let mut vm = VM::new();
    let (pair, region) = fresh_pair(&mut vm);
    let generation = vm.heap().generation_raw(region.get());
    vm.fiber.stack.extend([pair, pair]);
    run(&mut vm, 1, 0);
    assert_eq!(
        vm.fiber.stack.len(),
        2,
        "the handler leaves the operand stack as it found it"
    );
    assert!(
        vm.fiber.stack[0].is_nil(),
        "the route reads nil after the release"
    );
    assert!(
        vm.fiber.stack[1].is_nil(),
        "the postponed slot reads nil after the release"
    );
    assert_ne!(
        vm.heap().generation_raw(region.get()),
        generation,
        "the release frees the region the pair held alone"
    );
}

/// On a path through no arm that postpones the release, the postponed slot
/// reads `nil`. The instruction then releases nothing and leaves the route to
/// the release behind the call.
///
/// The counter-factual: a handler that stamped the route whatever the slot
/// held, so the release behind the call, the ownership move on this path,
/// found `nil` and the region stranded.
#[test]
fn an_empty_postponed_slot_leaves_the_route_to_the_release_behind_the_call() {
    let mut vm = VM::new();
    let (pair, region) = fresh_pair(&mut vm);
    let generation = vm.heap().generation_raw(region.get());
    vm.fiber.stack.extend([pair, Value::NIL]);
    run(&mut vm, 1, 0);
    assert_eq!(
        vm.fiber.stack.len(),
        2,
        "the handler leaves the operand stack as it found it"
    );
    assert!(
        (vm.fiber.stack[0].tag, vm.fiber.stack[0].payload) == (pair.tag, pair.payload),
        "the route still holds the value for the release behind the call"
    );
    assert_eq!(
        vm.heap().generation_raw(region.get()),
        generation,
        "nothing releases the region"
    );
    vm.heap().decref_region(region);
}
