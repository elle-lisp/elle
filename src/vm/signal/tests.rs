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
            vm.fiber.delivery.resume_unfunded(),
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
            vm.fiber.delivery.resume_unfunded(),
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
            !vm.fiber.delivery.resume_unfunded(),
            "a primitive that returns funds its own result — nothing is owed",
        );
    })
}

// -- which parks owe their own payload a release --

/// A capability denial parks a payload the VM built in place of a call that never
/// ran, so no body reference answers for it and the install that displaces it owes
/// its region one decref (docs/impl/region/owner.md § "A payload the RUNTIME built
/// is released by the install that displaces it"). Only the denial site can tell a
/// park has that shape, so it records the payload for
/// `release_displaced_denial_payload` to match against the live parked signal.
#[test]
fn a_capability_denial_park_records_the_payload_it_leaves_over() {
    with_test_region(|| {
        let mut vm = VM::new();
        let (code, env) = test_fixtures();
        let mut ip = 0usize;
        let blocked = SIG_IO;

        let result = vm.handle_capability_denial(
            &crate::primitives::def::NOOP_PRIM,
            blocked,
            &[],
            &code,
            &env,
            &mut ip,
        );

        assert_eq!(result, Some(blocked));
        let (_, payload) = vm.fiber.signal.expect("the denial parks its payload");
        assert_eq!(
            vm.fiber
                .delivery
                .bodyless()
                .map(|p| p.bit_identical(payload)),
            Some(true),
            "the denial records the very payload it parked, so the resume can \
             match the record against the live signal",
        );
    })
}

/// The tail-position mirror. A tail denial builds no frame of its own, so the
/// record — like the delivery obligation beside it — must ride the fiber.
#[test]
fn a_tail_capability_denial_park_records_the_payload_it_leaves_over() {
    with_test_region(|| {
        let mut vm = VM::new();
        let blocked = SIG_IO;

        let result =
            vm.handle_capability_denial_tail(&crate::primitives::def::NOOP_PRIM, blocked, &[]);

        assert_eq!(result, blocked);
        let (_, payload) = vm.fiber.signal.expect("the tail denial parks its payload");
        assert_eq!(
            vm.fiber
                .delivery
                .bodyless()
                .map(|p| p.bit_identical(payload)),
            Some(true),
            "a tail denial records its payload too — the frame it resumes into is \
             built by a driver that never saw the denied call",
        );
    })
}

/// The counter-factual for the two above: an ordinary suspend parks a payload the
/// resumed body releases itself. Recording it would make the displacing install
/// run a second release and free the value under every holder that outlives the
/// fiber.
#[test]
fn an_ordinary_suspend_records_no_payload_to_release() {
    with_test_region(|| {
        let mut vm = VM::new();
        let (code, env) = test_fixtures();
        let mut ip = 0usize;

        let result = vm.handle_primitive_signal(SIG_YIELD, Value::int(1), &code, &env, &mut ip);

        assert_eq!(result, Some(SIG_YIELD));
        assert!(
            vm.fiber.delivery.bodyless().is_none(),
            "a yielded payload is body-owned — the resume owes it no release",
        );
    })
}

// -- the raised delivery's identity gate --

/// A raised payload that IS one of the call's arguments gets the delivery minted
/// and recorded; a payload the native BUILT does not. Both positions raise through
/// this, so the gate is what keeps the mint off every ordinary native raise — a
/// fresh error struct funds its delivery with its birth reference, and a second
/// one is stranded per raised-and-caught error.
#[test]
fn a_raised_argument_takes_the_delivery_and_a_built_payload_does_not() {
    with_test_region(|| {
        let mut vm = VM::new();
        let (payload, region) = crate::value::arena::alloc_in_fresh_region(
            unsafe { &mut *vm.heap_ptr },
            crate::value::heap::HeapObject::Pair(crate::value::heap::Pair::new(
                Value::int(1),
                Value::NIL,
            )),
        );
        let before = vm.heap().region_rc(region);

        // Nobody's argument: the native built it, so its birth reference is the
        // delivery and a mint here would out-count the catcher's single release.
        vm.mint_raised_argument_delivery(&[Value::int(9)], payload);
        assert_eq!(
            vm.heap().region_rc(region),
            before,
            "a payload the native built funds its own delivery",
        );
        assert!(
            !vm.fiber.delivery.mint_names(payload),
            "an unminted delivery must leave the frames' payload exemption standing",
        );

        // The call's own argument handed back: the frame's references all answer
        // to the frame's own routes, so the catcher's read needs one of its own.
        vm.mint_raised_argument_delivery(&[Value::int(9), payload], payload);
        assert_eq!(
            vm.heap().region_rc(region),
            before + 1,
            "a raised argument's delivery is minted at the signal exit",
        );
        assert!(
            vm.fiber.delivery.mint_names(payload),
            "the record is what withdraws the walk's payload exemption",
        );
    })
}

/// An IMMEDIATE payload crosses no region, so there is nothing to fund — and
/// recording it would withdraw the exemption for a value no walk can release.
#[test]
fn an_immediate_raised_argument_takes_no_delivery() {
    with_test_region(|| {
        let mut vm = VM::new();
        vm.mint_raised_argument_delivery(&[Value::int(9)], Value::int(9));
        assert!(
            !vm.fiber.delivery.mint_names(Value::int(9)),
            "an immediate payload has no region for the record to speak for",
        );
    })
}

// -- a host that refuses a suspend-class signal abandons the park's funding --

/// A host driving a thunk on the current fiber (`eval`, `import`,
/// `compile/run-on`, the root driver) can refuse a suspend-class signal it
/// cannot host: the park is dead, and its funding must not survive into the
/// fiber's next park — a stale record there would mint a `ResumeDelivery`
/// retain no consumer ever releases, one region per refused park.
#[test]
fn a_refused_hosted_park_leaves_no_funding() {
    with_test_region(|| {
        let mut vm = VM::new();
        let (code, env) = test_fixtures();
        let mut ip = 0usize;

        vm.handle_primitive_signal(SIG_YIELD, Value::int(1), &code, &env, &mut ip);
        assert!(vm.fiber.delivery.resume_unfunded());

        vm.abandon_hosted_park(SIG_YIELD);
        assert!(
            !vm.fiber.delivery.resume_unfunded(),
            "the refused park's funding is consumed by the abandonment",
        );
    })
}

/// The counter-factual: an error is not an abandonment — an `:error` fiber is
/// resumable and its payload-named records are identity-gated — so the
/// abandonment seam leaves an error's ledger alone.
#[test]
fn an_error_exit_abandons_no_funding() {
    with_test_region(|| {
        let mut vm = VM::new();
        let (code, env) = test_fixtures();
        let mut ip = 0usize;

        vm.handle_primitive_signal(SIG_YIELD, Value::int(1), &code, &env, &mut ip);
        vm.abandon_hosted_park(crate::value::SIG_ERROR);
        assert!(
            vm.fiber.delivery.resume_unfunded(),
            "an error exit is not a refusal of the park — the funding stays for \
             the delivery funnel",
        );
    })
}
