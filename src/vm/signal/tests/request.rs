// audited: 2026-09-29
//! Which suspending primitive parks a payload with no body reference: an io op's
//! own request, never an argument the call was handed.
//!
//! docs/impl/region/park.md

use super::*;

/// A portless `Sleep` request on the VM's own heap, built the way an io op
/// builds one: the park takes a retain on its region.
fn sleep_request(vm: &mut VM) -> Value {
    let ctx = crate::primitives::ctx::Alloc::new(unsafe { &mut *vm.heap_ptr });
    crate::io::request::IoRequest::test_sleep(&ctx)
}

/// An io op returns a request it built, and its body never names that value, so
/// the park records it for the install that displaces it (§ "A payload the
/// RUNTIME built is released by the install that displaces it"). The payload is
/// read here, at the park, because nothing after the park can tell it from a
/// relay's `(emit :io v)` of the same request.
#[test]
fn an_io_op_park_records_its_request() {
    with_test_region(|| {
        let mut vm = VM::new();
        let (code, env) = test_fixtures();
        let mut ip = 0usize;
        let request = sleep_request(&mut vm);

        let result =
            vm.handle_primitive_signal(SIG_IO, request, &[Value::int(0)], &code, &env, &mut ip);

        assert_eq!(result, Some(SIG_IO));
        assert_eq!(
            vm.fiber
                .delivery
                .bodyless()
                .map(|p| p.bit_identical(request)),
            Some(true),
            "the request an io op built has no body reference, so the park records it",
        );
        assert!(
            vm.fiber.delivery.resume_unfunded(),
            "an io op is a primitive park: its resume value owes a mint as well",
        );
    })
}

/// The tail-position mirror: the frame is parked later, by a driver that never
/// saw the op, so the record rides the fiber.
#[test]
fn a_tail_io_op_park_records_its_request() {
    with_test_region(|| {
        let mut vm = VM::new();
        let request = sleep_request(&mut vm);

        let result = vm.handle_primitive_signal_tail(SIG_IO, request, &[Value::int(0)]);

        assert_eq!(result, SIG_IO);
        assert_eq!(
            vm.fiber
                .delivery
                .bodyless()
                .map(|p| p.bit_identical(request)),
            Some(true),
            "a tail io op's request is recorded on the fiber too",
        );
    })
}

/// The counter-factual: `(emit kw v)` with a keyword the compiler cannot read
/// suspends through the `emit` primitive, and a relay hands it the child's
/// request as `v`. The bits and the payload's type are an io op's, but the
/// payload is the call's own argument and the relaying body owns a reference to
/// it. Recording it makes the install that answers the relay release the
/// child's request once more, and free the child's port and buffers under it.
#[test]
fn a_primitive_park_of_its_own_argument_records_nothing_to_release() {
    with_test_region(|| {
        let mut vm = VM::new();
        let (code, env) = test_fixtures();
        let mut ip = 0usize;
        let request = sleep_request(&mut vm);
        let args = [Value::keyword("io"), request];

        vm.handle_primitive_signal(SIG_IO, request, &args, &code, &env, &mut ip);

        assert!(
            vm.fiber.delivery.bodyless().is_none(),
            "a relayed request is body-owned, so the relay's park owes no release",
        );
        assert!(
            vm.fiber.delivery.resume_unfunded(),
            "the emit primitive still never returns, so its resume value owes a mint",
        );
    })
}

/// The tail mirror of the counter-factual, the shape `(defn raise [kw v] (emit
/// kw v))` takes.
#[test]
fn a_tail_primitive_park_of_its_own_argument_records_nothing_to_release() {
    with_test_region(|| {
        let mut vm = VM::new();
        let request = sleep_request(&mut vm);
        let args = [Value::keyword("io"), request];

        vm.handle_primitive_signal_tail(SIG_IO, request, &args);

        assert!(
            vm.fiber.delivery.bodyless().is_none(),
            "a relayed request is body-owned in tail position too",
        );
    })
}
