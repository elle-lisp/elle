// audited: 2026-09-29
//! That the value mint after a `JoinRegion` joins the partner's region, and that
//! a call gives back a join its result did not use.
//!
//! docs/impl/region/colocation.md

use super::*;
use crate::primitives::def::{PrimitiveDef, RegionEffect};
use crate::value::fiber::SignalBits;
use crate::value::heap::{HeapObject, Pair};

fn returns_fresh_pair(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (SignalBits::EMPTY, ctx.pair(Value::int(1), Value::NIL))
}

fn returns_immediate(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (SignalBits::EMPTY, Value::int(7))
}

fn returns_first_arg(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    (SignalBits::EMPTY, args[0])
}

const fn def_with(
    name: &'static str,
    func: crate::value::types::PrimFn,
    effect: RegionEffect,
) -> PrimitiveDef {
    PrimitiveDef {
        name,
        func,
        effect,
        ..PrimitiveDef::DEFAULT
    }
}

static FRESH: PrimitiveDef = def_with("test/fresh", returns_fresh_pair, RegionEffect::Fresh);
static IMMEDIATE: PrimitiveDef =
    def_with("test/immediate", returns_immediate, RegionEffect::Immediate);
static PASS_THROUGH: PrimitiveDef = def_with(
    "test/pass-through",
    returns_first_arg,
    RegionEffect::PassThrough,
);

fn slot(n: u32) -> StaticRegion {
    StaticRegion::new(n).expect("nonzero slot")
}

/// An @array in a region of its own, the region's one reference its holder's.
fn partner(vm: &mut VM) -> (Value, RuntimeRegion) {
    let r = vm.heap().new_runtime_region();
    let v = vm.heap().alloc_in_region(
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![])),
            traits: Value::NIL,
        },
        r,
    );
    (v, r)
}

fn pair_in(vm: &mut VM, r: RuntimeRegion) -> Value {
    vm.heap()
        .alloc_in_region(HeapObject::Pair(Pair::new(Value::int(2), Value::NIL)), r)
}

fn region_of(vm: &mut VM, v: Value) -> Option<RuntimeRegion> {
    crate::value::arena::region_of(vm.heap(), v)
}

fn call(vm: &mut VM, def: &'static PrimitiveDef, args: &[Value], at: StaticRegion) -> Value {
    vm.dispatch_native_call(def, args, at).1
}

#[test]
fn a_pending_join_puts_a_fresh_call_result_in_the_partners_region() {
    let mut vm = VM::new();
    let (container, r) = partner(&mut vm);

    vm.set_pending_join(slot(2), container);
    let v = call(&mut vm, &FRESH, &[], slot(2));

    assert_eq!(
        region_of(&mut vm, v),
        Some(r),
        "the result is born in the partner's region"
    );
    assert_eq!(
        vm.heap().region_rc(r),
        2,
        "the join's reference is the caller's reference to the result",
    );
}

#[test]
fn an_allocation_slot_mint_takes_a_pending_join() {
    let mut vm = VM::new();
    vm.push_activation_region_map();
    let (container, r) = partner(&mut vm);

    vm.set_pending_join(slot(3), container);
    let region = vm.runtime_region_for_alloc_slot(slot(3));

    assert_eq!(region, r, "the slot resolves to the partner's region");
    assert_eq!(
        vm.heap().region_rc(r),
        2,
        "the slot's release gives this back"
    );
    assert_eq!(
        vm.take_runtime_region_for_drop_slot(slot(3)),
        Some(r),
        "the slot's release finds the joined region",
    );
}

#[test]
fn a_pending_join_is_consumed_by_one_mint() {
    let mut vm = VM::new();
    let (container, r) = partner(&mut vm);

    vm.set_pending_join(slot(2), container);
    let joined = call(&mut vm, &FRESH, &[], slot(2));
    let fresh = call(&mut vm, &FRESH, &[], slot(2));

    assert_eq!(region_of(&mut vm, joined), Some(r));
    assert_ne!(
        region_of(&mut vm, fresh),
        Some(r),
        "the next mint is fresh again"
    );
    assert_eq!(vm.heap().region_rc(r), 2);
}

#[test]
fn a_joined_call_that_returns_an_immediate_gives_the_join_back() {
    let mut vm = VM::new();
    let (container, r) = partner(&mut vm);

    vm.set_pending_join(slot(2), container);
    call(&mut vm, &IMMEDIATE, &[], slot(2));

    assert_eq!(
        vm.heap().region_rc(r),
        1,
        "an immediate result names no region, so nothing gives the join back later",
    );
}

#[test]
fn a_joined_call_that_returns_a_value_from_elsewhere_gives_the_join_back() {
    let mut vm = VM::new();
    let (container, r) = partner(&mut vm);
    let s = vm.heap().new_runtime_region();
    let elsewhere = pair_in(&mut vm, s);

    vm.set_pending_join(slot(2), container);
    let v = call(&mut vm, &PASS_THROUGH, &[elsewhere], slot(2));

    assert_eq!(region_of(&mut vm, v), Some(s));
    assert_eq!(
        vm.heap().region_rc(s),
        2,
        "the caller's pass-through reference"
    );
    assert_eq!(
        vm.heap().region_rc(r),
        1,
        "the caller releases the result's region, not the joined one",
    );
}

#[test]
fn a_joined_pass_through_result_in_the_partners_region_keeps_the_join() {
    // The result is a value already in the partner's region. It reads as fresh
    // to the dispatcher, so the join's reference is the caller's reference, and
    // the declaration oracle must not read "fresh" as a broken pass-through.
    let mut vm = VM::new();
    let (container, r) = partner(&mut vm);
    let member = pair_in(&mut vm, r);

    vm.set_pending_join(slot(2), container);
    let v = call(&mut vm, &PASS_THROUGH, &[member], slot(2));

    assert_eq!(region_of(&mut vm, v), Some(r));
    assert_eq!(vm.heap().region_rc(r), 2);
}

#[test]
fn a_pending_join_for_another_slot_is_dropped() {
    let mut vm = VM::new();
    let (container, r) = partner(&mut vm);

    vm.set_pending_join(slot(5), container);
    let other = call(&mut vm, &FRESH, &[], slot(6));
    let later = call(&mut vm, &FRESH, &[], slot(5));

    assert_ne!(
        region_of(&mut vm, other),
        Some(r),
        "another slot's mint is fresh"
    );
    assert_ne!(
        region_of(&mut vm, later),
        Some(r),
        "the join does not wait past the mint it was placed before",
    );
    assert_eq!(vm.heap().region_rc(r), 1);
}

#[test]
fn a_pending_join_refuses_an_owned_partner() {
    let mut vm = VM::new();
    let (container, r) = partner(&mut vm);
    let owner = vm.heap().new_runtime_region();
    pair_in(&mut vm, owner);
    vm.heap().adopt_region(owner, r);

    vm.set_pending_join(slot(2), container);
    let v = call(&mut vm, &FRESH, &[], slot(2));

    assert_ne!(
        region_of(&mut vm, v),
        Some(r),
        "an Owned partner has no count to take"
    );
}

#[test]
fn a_pending_join_whose_partner_died_mints_fresh() {
    // The partner's id can name a new region by the time the mint runs. The join
    // reads the id's generation, so it never lands in the stranger.
    let mut vm = VM::new();
    let (container, r) = partner(&mut vm);
    vm.set_pending_join(slot(2), container);

    vm.heap().decref_region(r);
    let stranger = vm.heap().new_runtime_region();
    assert_eq!(stranger, r, "the freed id is reissued");
    pair_in(&mut vm, stranger);

    let v = call(&mut vm, &FRESH, &[], slot(2));
    assert_ne!(region_of(&mut vm, v), Some(stranger));
    assert_eq!(vm.heap().region_rc(stranger), 1);
}
