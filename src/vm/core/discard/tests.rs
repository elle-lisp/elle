// audited: 2026-09-29
//! How a host that runs code on the current fiber ends a park of that code:
//! refused, or handed on as its own call's park.
//!
//! docs/impl/region/park.md

use crate::value::arena::with_test_region;
use crate::value::Value;
use crate::value::{SIG_ERROR, SIG_YIELD};
use crate::vm::VM;
use std::rc::Rc;

/// Minimal fixtures for `handle_primitive_signal`: (code, env).
fn test_fixtures() -> (crate::value::Code, Rc<Vec<Value>>) {
    (
        crate::value::ClosureTemplate::for_proto(
            unsafe { &mut *crate::value::arena::leaked_test_heap() },
            &Rc::new(crate::value::TemplateProto::new(
                Vec::new(),
                crate::value::Arity::Exact(0),
                Vec::new(),
            )),
        )
        .code(),
        Rc::new(vec![]),
    )
}

// -- a host that refuses a park ends it --

/// A host that runs code on the current fiber and refuses a park of it
/// (`eval`, `import`, `compile/run-on :jit`, the root driver) ends that park as
/// a squelch boundary does: the frames it parked never run again, so they
/// leave the fiber, and the park's funding goes with them. Counter-factual: a
/// refusal that only clears the ledger leaves the refused frames parked. The
/// host's error exit then parks no frame of the fiber's own, and a restart
/// replays the refused code.
#[test]
fn a_refused_park_takes_its_frames_off_the_fiber() {
    with_test_region(|| {
        let mut vm = VM::new();
        let (code, env) = test_fixtures();
        let mut ip = 0usize;

        vm.handle_primitive_signal(SIG_YIELD, Value::int(1), &code, &env, &mut ip);
        assert!(vm.fiber.suspended.is_some(), "the park parks its frame");

        vm.refuse_hosted_park(SIG_YIELD, vm.fiber.param_depth());
        assert!(
            vm.fiber.suspended.is_none(),
            "the refused frames leave the fiber",
        );
        assert!(
            !vm.fiber.delivery.resume_unfunded(),
            "and so does the refused park's funding",
        );
    })
}

/// No reader ever takes a refused park's payload out of the signal slot, so
/// the delivery retain the park took is the refusal's to release.
/// Counter-factual: a refusal that only clears the ledger strands the retain,
/// one region per refused park.
#[test]
fn a_refused_park_releases_its_delivery_retain() {
    with_test_region(|| {
        let mut vm = VM::new();
        let (code, env) = test_fixtures();
        let mut ip = 0usize;
        let (payload, region) = crate::value::arena::alloc_in_fresh_region(
            unsafe { &mut *vm.heap_ptr },
            crate::value::heap::HeapObject::Pair(crate::value::heap::Pair::new(
                Value::int(1),
                Value::NIL,
            )),
        );
        let before = vm.heap().region_rc(region);

        vm.handle_primitive_signal(SIG_YIELD, payload, &code, &env, &mut ip);
        assert_eq!(
            vm.heap().region_rc(region),
            before + 1,
            "the park takes the delivery retain",
        );

        vm.refuse_hosted_park(SIG_YIELD, vm.fiber.param_depth());
        assert_eq!(
            vm.heap().region_rc(region),
            before,
            "the refusal releases the retain no reader will consume",
        );
    })
}

/// An error is not a refused park. An `:error` fiber is resumable, so its
/// frames and funding stay for the restart.
#[test]
fn an_error_is_not_a_refused_park() {
    with_test_region(|| {
        let mut vm = VM::new();
        let (code, env) = test_fixtures();
        let mut ip = 0usize;

        vm.handle_primitive_signal(SIG_YIELD, Value::int(1), &code, &env, &mut ip);
        vm.refuse_hosted_park(SIG_ERROR, vm.fiber.param_depth());
        assert!(
            vm.fiber.suspended.is_some(),
            "the frames stay for the restart"
        );
        assert!(
            vm.fiber.delivery.resume_unfunded(),
            "and so does the funding the delivery funnel takes",
        );
    })
}

// -- a host that hands a thunk's park on ends the thunk's funding --

/// A host that hands a thunk's suspension on as its own call's park
/// (`arena/allocs`, `compile/run-on :bytecode`) ends the thunk's park, and its
/// funding must not survive into the park the host's call makes — the park
/// write would find it standing.
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
            "the thunk's funding is consumed by the abandonment",
        );
    })
}

/// The counter-factual: an error is not an abandonment. An `:error` fiber is
/// resumable and its payload-named records are identity-gated, so the
/// abandonment seam leaves an error's ledger alone.
#[test]
fn an_error_exit_abandons_no_funding() {
    with_test_region(|| {
        let mut vm = VM::new();
        let (code, env) = test_fixtures();
        let mut ip = 0usize;

        vm.handle_primitive_signal(SIG_YIELD, Value::int(1), &code, &env, &mut ip);
        vm.abandon_hosted_park(SIG_ERROR);
        assert!(
            vm.fiber.delivery.resume_unfunded(),
            "an error exit is not a refusal of the park — the funding stays for \
             the delivery funnel",
        );
    })
}
