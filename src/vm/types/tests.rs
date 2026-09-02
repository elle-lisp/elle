//! Unit tests (`super` is the parent impl module).

use super::*;

// Sibling intrinsic opcodes must share one failure policy: a type
// mismatch reaching `%array-push`/`%bytes-push`/`%get`/`%length`/
// `%string-push` is a compiler bug (the opcode was emitted without the
// operand proof its contract requires) and panics. Routing this class
// of bug through `fiber.signal` as a catchable error would let code the
// compiler stamped signal-free set SIG_ERROR at runtime, making static
// signal inference disagree with dynamic behavior. The catchable-error
// path is the registered NativeFn that validates dynamic value-position
// calls, not the instruction.
#[test]
#[should_panic(expected = "%string-push")]
fn intr_string_push_type_mismatch_panics_like_its_siblings() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let mut vm = VM::new();
        vm.fiber.stack.push(Value::int(7)); // collection: not a string
        vm.fiber.stack.push(h.ctx().string("x"));
        handle_intr_string_push(&mut vm);
    });
}

// An unhashable key (here a float) reaching the struct arm of `%get` is a
// compiler bug: nothing hashable enough to be a struct key could ever have
// been stored under it, so the lookup is nonsense, not an absent key. It must
// panic — silently returning nil would let a mis-emitted `%get` masquerade as
// a successful miss and hide the missing key proof its contract requires.
#[test]
#[should_panic(expected = "%get: unhashable key")]
fn intr_get_unhashable_key_on_immutable_struct_panics() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let mut vm = VM::new();
        let s = h.ctx().struct_from_sorted(vec![(
            crate::value::heap::TableKey::keyword("a"),
            Value::int(1),
        )]);
        vm.fiber.stack.push(s);
        vm.fiber.stack.push(Value::float(1.5)); // floats are not hashable keys
        handle_intr_get(&mut vm);
    });
}

#[test]
#[should_panic(expected = "%get: unhashable key")]
fn intr_get_unhashable_key_on_mutable_struct_panics() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let mut vm = VM::new();
        let mut entries = std::collections::BTreeMap::new();
        entries.insert(crate::value::heap::TableKey::keyword("a"), Value::int(1));
        let s = h.ctx().struct_mut_from(entries);
        vm.fiber.stack.push(s);
        vm.fiber.stack.push(Value::float(1.5)); // floats are not hashable keys
        handle_intr_get(&mut vm);
    });
}

// The counter-factual: a hashable key that simply isn't present stays a nil
// miss, so the panic above is specific to unhashable keys and not to misses.
#[test]
fn intr_get_absent_hashable_key_on_struct_is_nil() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let mut vm = VM::new();
        let s = h.ctx().struct_from_sorted(vec![(
            crate::value::heap::TableKey::keyword("a"),
            Value::int(1),
        )]);
        vm.fiber.stack.push(s);
        vm.fiber.stack.push(Value::keyword("missing"));
        handle_intr_get(&mut vm);
        assert_eq!(vm.fiber.stack.pop(), Some(Value::NIL));
    });
}
