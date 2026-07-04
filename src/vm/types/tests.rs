//! Unit tests (`super` is the parent impl module).

use super::*;

// Sibling unchecked intrinsics must share one failure policy: a type
// mismatch reaching `%array-push`/`%bytes-push`/`%get`/`%length`/
// `%string-push` is a compiler bug (the intrinsic was emitted without
// the proof it requires) and panics. Routing this class of bug through
// `fiber.signal` as a catchable error would let code the compiler
// stamped signal-free set SIG_ERROR at runtime, making static signal
// inference disagree with dynamic behavior. The catchable-error path is
// --checked-intrinsics' NativeFn, not the unchecked instruction.
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
