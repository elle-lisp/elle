// audited: 2026-11-11
//! A collection call mints a call-result region only where it may allocate: on a string or a set.
//!
//! docs/impl/region/model.md
//!
//! An array, struct or bytes call answers an element, a byte or the caller's
//! default, and allocates nothing. A mint it took anyway goes back to the free
//! list unused, so the count of mints is the only thing that tells the two
//! apart.

use super::*;
use crate::hir::region::RuntimeRegion;
use crate::primitives::ctx::Alloc;
use crate::value::types::TableKey;
use std::collections::BTreeMap;

/// The call-result regions `func` applied to `args` mints, and its answer.
fn minted_by(vm: &mut VM, func: Value, args: &[Value]) -> (u64, Value) {
    let slot = StaticRegion::new(2).expect("mortal slot");
    let before = VM::call_result_mints();
    let answer = vm
        .dispatch_collection_call(&func, args, slot)
        .expect("a callable collection")
        .expect("the call succeeds");
    (VM::call_result_mints() - before, answer)
}

/// A fresh region of `vm`'s heap, and a ctx that allocates into it.
fn region_of_vm(vm: &mut VM) -> (RuntimeRegion, Alloc<'static>) {
    let region = vm.heap().new_runtime_region();
    (
        region,
        Alloc::with_region(region, unsafe { &mut *vm.heap_ptr }),
    )
}

#[test]
fn an_indexed_read_mints_no_region() {
    let mut vm = VM::new();
    let (region, ctx) = region_of_vm(&mut vm);
    let elem = ctx.string("x");
    let key = Value::keyword("k");
    let entry = || BTreeMap::from([(TableKey::from_value(&key).expect("a key"), elem)]);
    let cases = [
        ("an array", ctx.array(vec![elem]), vec![Value::int(0)]),
        ("an @array", ctx.array_mut(vec![elem]), vec![Value::int(0)]),
        ("a struct", ctx.struct_from(entry()), vec![key]),
        ("an @struct", ctx.struct_mut_from(entry()), vec![key]),
        ("a bytes", ctx.bytes(vec![7]), vec![Value::int(0)]),
        ("an @bytes", ctx.bytes_mut(vec![7]), vec![Value::int(0)]),
        (
            "an array past its end",
            ctx.array(vec![elem]),
            vec![Value::int(5), Value::int(9)],
        ),
    ];
    for (what, func, args) in cases {
        let (minted, answer) = minted_by(&mut vm, func, &args);
        assert_eq!(minted, 0, "{what} call minted a call-result region");
        if answer == elem {
            // The read's pass-through retain, which the caller's release takes.
            vm.heap().decref_region_if_present(region);
        }
    }
    vm.heap().decref_region_if_present(region);
}

// The counter-factual for the mint the read skips: the element still needs
// the caller's own reference. Without the retain, the caller's release of the
// answer would take the reference the region's holder owns.
#[test]
fn an_indexed_read_retains_the_element_it_answers() {
    let mut vm = VM::new();
    let (region, ctx) = region_of_vm(&mut vm);
    let elem = ctx.string("x");
    let array = ctx.array(vec![elem]);
    let before = vm.heap().region_rc(region);
    let (_, answer) = minted_by(&mut vm, array, &[Value::int(0)]);
    assert_eq!(answer, elem);
    assert_eq!(
        vm.heap().region_rc(region),
        before + 1,
        "the read handed the caller no reference to the element"
    );
    vm.heap().decref_region_if_present(region);
    vm.heap().decref_region_if_present(region);
}

// The control: a string call builds a grapheme into the region it mints, and a
// set call may freeze its key into one, so the count does see a mint.
#[test]
fn a_string_or_set_call_mints_its_region() {
    let mut vm = VM::new();
    let (region, ctx) = region_of_vm(&mut vm);
    let string = ctx.string("abc");
    let set = ctx.set([Value::int(1)].into());
    let (minted, grapheme) = minted_by(&mut vm, string, &[Value::int(1)]);
    assert_eq!(minted, 1, "a string call minted no region");
    assert_eq!(grapheme.with_string(str::to_string).as_deref(), Some("b"));
    let fresh = crate::value::arena::region_of(vm.heap(), grapheme).expect("a heap grapheme");
    vm.heap().decref_region_if_present(fresh);
    let (minted, member) = minted_by(&mut vm, set, &[Value::int(1)]);
    assert_eq!(minted, 1, "a set call minted no region");
    assert_eq!(member, Value::TRUE);
    vm.heap().decref_region_if_present(region);
}
