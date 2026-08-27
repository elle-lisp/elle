use super::*;
use crate::error::LocationMap;
use crate::value::arena::with_test_region;
use crate::value::{SIG_DEBUG, SIG_IO, SIG_YIELD};

/// Create minimal test fixtures for handle_primitive_signal: (code, env).
type TestFixtures = (crate::value::Code, Rc<Vec<Value>>);
fn test_fixtures() -> TestFixtures {
    (
        crate::value::Code::new(
            Rc::new(vec![]),
            Rc::new(vec![]),
            Rc::new(LocationMap::new()),
            Rc::new(vec![]),
        ),
        Rc::new(vec![]),
    )
}

// -- handle_primitive_signal (Call position) --

#[test]
fn composed_error_io_treated_as_error() {
    with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let mut vm = VM::new();
        let (code, env) = test_fixtures();
        let mut ip = 0usize;
        let bits = SIG_ERROR | SIG_IO;

        let result = vm.handle_primitive_signal(bits, h.ctx().string("boom"), &code, &env, &mut ip);

        // Error path returns None
        assert!(result.is_none());
        let (sig, _) = vm.fiber.signal.take().unwrap();
        assert!(sig.intersects(SIG_ERROR));
        // NIL pushed (error convention)
        assert_eq!(vm.fiber.stack.pop(), Some(Value::NIL));
        // No suspended frame created
        assert!(vm.fiber.suspended.is_none());
    })
}

#[test]
fn unknown_signal_propagates() {
    with_test_region(|| {
        let mut vm = VM::new();
        let (code, env) = test_fixtures();
        let mut ip = 0usize;
        let bits = SIG_DEBUG; // not handled by any specific branch

        let result = vm.handle_primitive_signal(bits, Value::int(1), &code, &env, &mut ip);

        assert_eq!(result, Some(SIG_DEBUG));
        let (sig, _) = vm.fiber.signal.take().unwrap();
        assert_eq!(sig, SIG_DEBUG);
    })
}

// -- handle_primitive_signal_tail (TailCall position) --

#[test]
fn tail_composed_error_io_treated_as_error() {
    with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let mut vm = VM::new();
        let bits = SIG_ERROR | SIG_IO;

        let result = vm.handle_primitive_signal_tail(bits, h.ctx().string("boom"));

        // Should return the full composed bits
        assert!(result.intersects(SIG_ERROR));
        assert!(result.intersects(SIG_IO));
        let (sig, _) = vm.fiber.signal.take().unwrap();
        assert!(sig.intersects(SIG_ERROR));
        assert!(sig.intersects(SIG_IO));
    })
}

#[test]
fn tail_composed_yield_io_propagates() {
    with_test_region(|| {
        let mut vm = VM::new();
        let bits = SIG_YIELD | SIG_IO;

        let result = vm.handle_primitive_signal_tail(bits, Value::int(42));

        assert_eq!(result, SIG_YIELD | SIG_IO);
        let (sig, val) = vm.fiber.signal.take().unwrap();
        assert_eq!(sig, SIG_YIELD | SIG_IO);
        assert_eq!(val, Value::int(42));
    })
}

#[test]
fn tail_sig_ok_stores_ok() {
    with_test_region(|| {
        let mut vm = VM::new();

        let result = vm.handle_primitive_signal_tail(SIG_OK, Value::int(5));

        assert_eq!(result, SIG_OK);
        let (sig, val) = vm.fiber.signal.take().unwrap();
        assert_eq!(sig, SIG_OK);
        assert_eq!(val, Value::int(5));
    })
}

#[test]
fn tail_error_priority_over_yield() {
    with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let mut vm = VM::new();
        let bits = SIG_ERROR | SIG_YIELD;

        let result = vm.handle_primitive_signal_tail(bits, h.ctx().string("err"));

        assert!(result.intersects(SIG_ERROR));
        let (sig, _) = vm.fiber.signal.take().unwrap();
        assert!(sig.intersects(SIG_ERROR));
    })
}

// -- dispatch_query births its answer through the ctx's own region --

/// The SIG_QUERY answer is born in the call's ctx region — on the ctx's heap, in
/// the ctx's region. The answer struct can only land in the region the ctx
/// names, so the `(vm/config)` answer is born in the ctx's region by
/// construction.
#[test]
fn dispatch_query_answer_is_born_in_the_ctx_region() {
    let mut vm = VM::new();
    let answer_region = vm.result_region();
    let scratch = vm.result_region();
    // The query value `(:vm/config . nil)` — its pair on a scratch region.
    let query = crate::value::build::pair(
        unsafe { &mut *vm.heap_ptr },
        Value::keyword("vm/config"),
        Value::NIL,
        scratch,
    );
    let mut ctx =
        crate::primitives::ctx::Alloc::with_region(answer_region, unsafe { &mut *vm.heap_ptr });
    let (sig, answer) = vm.dispatch_query(&mut ctx, query);
    assert_eq!(sig, SIG_OK, "(vm/config) answers OK");
    assert!(answer.as_struct().is_some(), "(vm/config) answers a struct");
    assert_eq!(
        crate::value::arena::region_of(ctx.heap_mut(), answer),
        Some(answer_region),
        "the SIG_QUERY answer is born in the ctx's region, not the region TLS",
    );
}

// -- which parks owe their resume value a reference --

/// A suspending primitive's park owes its resume value one reference: the
/// primitive never returns, so the `Return` mint that would fund the parked
/// call's compiler-emitted result release never runs, and the resume value
/// stands in for that result (docs/impl/region/owner.md § "A delivery into a
/// replayed frame carries one owning reference"). The classifier is the only
/// place that can tell — by the delivery the frame is built and, for a tail
/// suspend, was built by a driver that never saw the primitive — so it records
/// the answer on the fiber for `do_fiber_resume_single` to take.
#[test]
fn a_suspending_primitive_park_owes_its_resume_value_a_reference() {
    with_test_region(|| {
        let mut vm = VM::new();
        let (code, env) = test_fixtures();
        let mut ip = 0usize;

        let result = vm.handle_primitive_signal(SIG_YIELD, Value::int(1), &code, &env, &mut ip);

        assert_eq!(result, Some(SIG_YIELD));
        assert!(
            vm.fiber.resume_value_unfunded,
            "the park at a suspending primitive call owes its resume value a reference",
        );
    })
}

/// The tail-position mirror: the suspend leaves no frame of its own, so the
/// obligation must survive on the fiber until whichever driver parks the frame
/// and, later, whichever route delivers into it.
#[test]
fn a_tail_suspending_primitive_park_owes_its_resume_value_a_reference() {
    with_test_region(|| {
        let mut vm = VM::new();

        let result = vm.handle_primitive_signal_tail(SIG_YIELD, Value::int(1));

        assert_eq!(result, SIG_YIELD);
        assert!(
            vm.fiber.resume_value_unfunded,
            "a tail suspend at a primitive owes its resume value a reference too",
        );
    })
}

/// A primitive that COMPLETES parks nothing, so nothing is owed. The
/// counter-factual for the two above: a classifier that set the flag on every
/// primitive result would mint against a resume the fiber never waits for.
#[test]
fn a_completing_primitive_owes_its_resume_value_nothing() {
    with_test_region(|| {
        let mut vm = VM::new();
        let (code, env) = test_fixtures();
        let mut ip = 0usize;

        let result = vm.handle_primitive_signal(SIG_OK, Value::int(1), &code, &env, &mut ip);

        assert!(
            result.is_none(),
            "a completing primitive continues dispatch"
        );
        assert!(
            !vm.fiber.resume_value_unfunded,
            "a primitive that returns funds its own result — nothing is owed",
        );
    })
}
