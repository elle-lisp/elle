// audited: 2026-11-24
//! The two slices an array rest takes: each slices its own kind, and refuses the other.
//!
//! docs/impl/region/anchors.md
//!
//! The counter-factual: one handler that keeps its input's kind passes both
//! slicing cases and neither refusal, so a lowering that took the wrong slice
//! would go unnoticed.

use super::*;
use crate::primitives::ctx::TestHeap;
use crate::value::{sorted_struct_get, TableKey};

/// A slice handler, as the dispatch loop calls it.
type SliceHandler = fn(&mut VM, &[u8], &mut usize);

/// Run `handle` over the scrutinee `make` builds, with the index operand 1,
/// then hand `check` the value the handler pushed and the error kind it set.
fn slice_from_one(
    handle: SliceHandler,
    make: impl FnOnce(&TestHeap) -> Value,
    check: impl FnOnce(Value, Option<Value>),
) {
    crate::value::arena::with_test_region(|| {
        let h = TestHeap::new();
        let mut vm = VM::new();
        vm.fiber.stack.push(make(&h));
        let mut ip = 0;
        handle(&mut vm, &[0, 1], &mut ip);
        assert_eq!(ip, 2, "the handler reads its index operand");
        let pushed = vm.fiber.stack.pop().expect("the handler pushes a value");
        let kind = vm.fiber.signal.take().map(|(_, err)| {
            let fields = err.as_struct().expect("an error is a struct");
            *sorted_struct_get(fields, &TableKey::keyword("error")).expect("an :error field")
        });
        check(pushed, kind);
    });
}

/// The elements 1, 2 and 3.
fn one_two_three() -> Vec<Value> {
    vec![Value::int(1), Value::int(2), Value::int(3)]
}

#[test]
fn array_slice_from_slices_an_array() {
    slice_from_one(
        handle_array_slice_from,
        |h| h.ctx().array(one_two_three()),
        |slice, kind| {
            assert_eq!(kind, None, "an array is ArraySliceFrom's own kind");
            assert_eq!(slice.as_array(), Some(&[Value::int(2), Value::int(3)][..]));
        },
    );
}

#[test]
fn array_slice_from_refuses_an_at_array() {
    slice_from_one(
        handle_array_slice_from,
        |h| h.ctx().array_mut(one_two_three()),
        |_, kind| assert_eq!(kind, Some(Value::keyword("type-error"))),
    );
}

#[test]
fn array_mut_slice_from_slices_an_at_array() {
    slice_from_one(
        handle_array_mut_slice_from,
        |h| h.ctx().array_mut(one_two_three()),
        |slice, kind| {
            assert_eq!(kind, None, "an @array is ArrayMutSliceFrom's own kind");
            let read = slice.array_mut_read().expect("the slice is an @array");
            assert_eq!(&read[..], &[Value::int(2), Value::int(3)][..]);
        },
    );
}

#[test]
fn array_mut_slice_from_refuses_an_array() {
    slice_from_one(
        handle_array_mut_slice_from,
        |h| h.ctx().array(one_two_three()),
        |_, kind| assert_eq!(kind, Some(Value::keyword("type-error"))),
    );
}
