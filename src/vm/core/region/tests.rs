//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::primitives::def::{PrimitiveDef, RegionEffect};
use crate::value::fiber::SignalBits;

fn returns_immediate(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (SignalBits::EMPTY, Value::int(7))
}
fn returns_fresh_pair(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    // A truthfully-fresh result: born in the CALL's own region via `ctx`
    // (RegionEffect::Fresh requires exactly that).
    (SignalBits::EMPTY, ctx.pair(Value::int(1), Value::NIL))
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

fn dispatch(def: &'static PrimitiveDef, args: &[Value]) -> Value {
    let mut vm = VM::new();
    let slot = StaticRegion::new(2).expect("mortal slot");
    let (_bits, value) = vm.dispatch_native_call(def, args, slot);
    value
}

#[test]
fn oracle_allows_truthful_immediate() {
    static TRUTHFUL: PrimitiveDef = def_with(
        "test/truthful-immediate",
        returns_immediate,
        RegionEffect::Immediate,
    );
    crate::value::arena::with_test_region(|| {
        assert_eq!(dispatch(&TRUTHFUL, &[]), Value::int(7));
    });
}

#[test]
fn oracle_allows_truthful_fresh() {
    static TRUTHFUL: PrimitiveDef = def_with(
        "test/truthful-fresh",
        returns_fresh_pair,
        RegionEffect::Fresh,
    );
    crate::value::arena::with_test_region(|| {
        let v = dispatch(&TRUTHFUL, &[]);
        assert!(v.as_pair().is_some());
    });
}

/// `VM::set_error` / `escaping_*` build their error in a FRESH region of
/// their own (docs/impl/region-ctx.md). Mint a
/// distinct live region `other` first; the error must land in neither it nor
/// any pre-existing region. The counterfactual: route `set_error` through a
/// shared region and `region_of(payload) == Some(other)` fires the assert.
#[test]
fn set_error_is_born_in_a_fresh_region() {
    crate::value::arena::with_test_region(|| {
        let mut vm = VM::new();
        let other = vm.heap().new_runtime_region();
        vm.set_error("type-error", "x");
        let (_, payload) = vm.fiber.signal.expect("set_error sets a signal");
        let r = crate::value::arena::region_of(unsafe { &mut *vm.heap_ptr }, payload);
        assert!(
            r.is_some(),
            "the error struct is a heap value with a region"
        );
        assert_ne!(
            r,
            Some(other),
            "set_error must mint its own fresh region, distinct from any other",
        );
    });
}

/// Twin of the above for [`VM::escaping_match_fail`] — the runtime
/// `:match-error` builder — pinning the same fresh-region contract.
#[test]
fn escaping_match_fail_is_born_in_a_fresh_region() {
    crate::value::arena::with_test_region(|| {
        let mut vm = VM::new();
        let other = vm.heap().new_runtime_region();
        let err = vm.escaping_match_fail(Value::int(7));
        assert_ne!(
            crate::value::arena::region_of(unsafe { &mut *vm.heap_ptr }, err),
            Some(other),
            "escaping_match_fail must mint its own fresh region",
        );
    });
}

// ── merged-slot mint-or-reuse (docs/impl/region-model.md § Merging) ──
//
// A merged slot is shared by ≥2 alloc instructions (child, then parent) after a
// builder-idiom merge: the child's alloc mints the physical region, the parent's
// alloc REUSES it, so both land in one region freed by the single `DecrefRegion`.
// A unique (unmerged) slot mints fresh every execution — the baseline. These pin
// the runtime primitive directly, with `merged_slots` supplied by hand (the
// lowerer populates it from the merge forest via `record_merged_slots`).

#[test]
fn merged_slot_reuses_after_first_mint() {
    crate::value::arena::with_test_region(|| {
        let mut vm = VM::new();
        vm.push_activation_region_map();
        let slot = StaticRegion::new(7).expect("nonzero slot");
        let merged: rustc_hash::FxHashSet<u32> = [slot.get()].into_iter().collect();
        // Child alloc: slot not yet mapped → mints.
        let child = vm.runtime_region_for_alloc_slot_maybe_merged(slot, &merged);
        // Parent alloc against the same MERGED slot: reuses the child's region.
        let parent = vm.runtime_region_for_alloc_slot_maybe_merged(slot, &merged);
        assert_eq!(
            child, parent,
            "a merged slot mint-or-reuses: the parent's alloc must reuse the \
             physical region the child's alloc minted"
        );
    });
}

#[test]
fn unmerged_slot_mints_fresh_each_execution() {
    crate::value::arena::with_test_region(|| {
        let mut vm = VM::new();
        vm.push_activation_region_map();
        let slot = StaticRegion::new(7).expect("nonzero slot");
        // The slot is NOT in the merged set — the unmerged baseline.
        let empty: rustc_hash::FxHashSet<u32> = rustc_hash::FxHashSet::default();
        let first = vm.runtime_region_for_alloc_slot_maybe_merged(slot, &empty);
        let second = vm.runtime_region_for_alloc_slot_maybe_merged(slot, &empty);
        assert_ne!(
            first, second,
            "an unmerged slot mints a fresh physical region on every execution \
             (byte-identical to the pre-merge baseline)"
        );
    });
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "RegionEffect::Immediate")]
fn oracle_panics_on_misdeclared_immediate() {
    static LYING: PrimitiveDef = def_with(
        "test/lying-immediate",
        returns_fresh_pair,
        RegionEffect::Immediate,
    );
    crate::value::arena::with_test_region(|| {
        dispatch(&LYING, &[]);
    });
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "RegionEffect::PassThrough")]
fn oracle_panics_on_misdeclared_passthrough() {
    static LYING: PrimitiveDef = def_with(
        "test/lying-passthrough",
        returns_fresh_pair,
        RegionEffect::PassThrough,
    );
    crate::value::arena::with_test_region(|| {
        dispatch(&LYING, &[]);
    });
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "RegionEffect::Fresh")]
fn oracle_panics_on_misdeclared_fresh() {
    static LYING: PrimitiveDef =
        def_with("test/lying-fresh", returns_first_arg, RegionEffect::Fresh);
    let mut vm = VM::new();
    // The arg is a heap value born on THIS heap but in a region the call did not
    // allocate (a fresh region, distinct from the call's slot region) — so a
    // Fresh claim, which requires the result in the call's OWN region, is a lie.
    // It must share the dispatching VM's heap so the oracle's `region_of` sees it.
    let arg = {
        use crate::value::heap::{HeapObject, Pair};
        let heap = unsafe { &mut *vm.heap_ptr };
        let r = heap.new_runtime_region();
        heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)), r)
    };
    let slot = StaticRegion::new(2).expect("mortal slot");
    vm.dispatch_native_call(&LYING, &[arg], slot);
}
