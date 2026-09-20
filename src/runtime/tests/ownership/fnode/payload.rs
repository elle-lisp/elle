// audited: 2026-09-19
//! That a park's payload region is worth exactly one retain, at every stage and on every route into the fiber.
//!
//! docs/impl/region/park.md

use super::*;

/// A fiber that RETURNS a freshly-allocated value reclaims its region.
///
/// The park retain (`incref_signal_region`, child.rs step 6a) pins the result
/// for a later `fiber/value`, and the fiber's free-time cross-ref scan releases
/// it. The pair must balance, or every completing fiber strands its result.
///
/// This is the discriminator for the emitting arm below: same shape, same
/// harness, no signal. It passes, so a failure there is the terminal-signal
/// path's own accounting rather than anything the harness does.
#[test]
fn a_returned_payload_region_is_reclaimed() {
    assert_eq!(
        payload_regions_stranded_over(50, crate::value::fiber::SIG_OK),
        0,
        "a fiber returning a fresh value must release its region",
    );
}

/// A fiber that leaves with a TERMINAL signal carrying a freshly-allocated
/// payload reclaims that payload's region.
///
/// Reaching `Emit` takes an `EmitEscape` retain on the payload's region
/// (`handle_emit`), covering the window until the compiler's `DecrefRegion` at
/// the emit's decref point fires. `with_child_fiber` then takes a second,
/// independent park retain on the same region so a later `fiber/value` can read
/// the result. Both retains must be discharged, exactly once each: the
/// free-time cross-ref scan releases the park retain, so the escape retain owes
/// a release of its own.
///
/// **This test fails.** One region survives per halted fiber, so the count is
/// the cycle count rather than zero. `Fiber::take_parked_state` reports a
/// parked signal's region only when the signal is non-terminal, which is what
/// leaves the escape retain outstanding here; reporting it regardless of the
/// bits measures correct in isolation but over-frees the corpus, so the escape
/// retain is already being consumed somewhere along the terminal teardown.
/// The `SIG_OK` discriminator above stays green either way.
#[test]
fn an_emitted_terminal_payload_region_is_reclaimed() {
    assert_eq!(
        payload_regions_stranded_over(50, crate::value::fiber::SIG_HALT),
        0,
        "an emitting fiber must release its payload's park escape retain",
    );
}

/// Run `n` fibers whose body puts a freshly-allocated value on the stack and
/// leaves with `bits` (a bare `Return` for `SIG_OK`, an `Emit` otherwise),
/// driving each through the resume path a halt takes. Returns the net live
/// region growth: the payload of every cycle should be gone by the end.
fn payload_regions_stranded_over(n: usize, bits: crate::value::SignalBits) -> i64 {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use crate::value::fiber::FiberStatus;

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;

    // Warm up one cycle so the baseline excludes first-run allocation.
    let mut baseline = 0i64;
    for i in 0..=n {
        let heap = unsafe { &mut *heap_ptr };
        let (payload, rid) = alloc_in_fresh_region(heap, cons());

        let mut bc = Bytecode::new();
        let idx = bc.add_constant(payload);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        if !bits.is_empty() {
            bc.emit(Instruction::Emit);
            bc.emit_signal_bits(bits);
        }
        bc.emit(Instruction::Return);
        let (handle, fiber_value) = child_fiber(heap, bc);

        let (result_bits, _v) = vm.do_fiber_resume(&handle, fiber_value);
        vm.finalize_if_halted(&handle, result_bits);
        if !bits.is_empty() {
            assert_eq!(result_bits, bits, "the body leaves with exactly its signal");
            assert_eq!(handle.with(|f| f.status), FiberStatus::Dead);
        }

        // Drop the fiber: its free-time scan is what owes the payload release.
        // The test's own alloc reference goes last, so the fiber's release is
        // the one that has to land for the region to reach rc 0.
        release_fiber_value(unsafe { &mut *heap_ptr }, fiber_value);
        drop(handle);
        unsafe { &mut *heap_ptr }.decref_region_if_present(rid);

        if i == 0 {
            baseline = unsafe { &*heap_ptr }.active_region_count() as i64;
        }
    }
    unsafe { &*heap_ptr }.active_region_count() as i64 - baseline
}

/// The payload region's refcount at each stage of a fiber that leaves with a
/// freshly-allocated result, for a bare `Return` (`SIG_OK`) and for an `Emit`
/// (`SIG_HALT`) alike:
///
/// | stage | rc |
/// |---|---|
/// | allocated (the test's own reference) | 1 |
/// | the code object built, its constant pool naming the payload | 2 |
/// | the fiber left, its result parked | 3 |
/// | the halt promoted the fiber to `:dead` | 3 |
/// | the fiber freed, its free-time signal scan run, its code object with it | 1 |
/// | the test released its own reference | 0 |
///
/// Hand-built bytecode can only name a heap value through the constant pool, so
/// the code object holds one reference of its own from the moment it is
/// materialized (docs/impl/region/template.md); it goes when the fiber value's
/// region does. Compiled code never puts a heap literal in a pool — it
/// materializes one per execution — so the extra reference is the harness's,
/// not a shape production produces.
///
/// Both arms leave the same way — the result is parked in `fiber.signal` for a
/// later `fiber/value` — so both keep the same ledger, at every stage. The park
/// is worth **exactly one** retain (`incref_signal_region`, child.rs step 6a),
/// whose sole release is the free-time signal scan.
///
/// An `Emit` also retains its payload as it escapes into the slot, covering the
/// window to the compiler's `DecrefRegion` at the emit's decref point — but a
/// `SIG_HALT` emit never reaches that decref (the dispatch loop leaves, and the
/// halt promotion makes the fiber unresumable), so that retain has no consumer
/// and must not be taken. Taking it is the leak
/// [`an_emitted_terminal_payload_region_is_reclaimed`] measures; the arms
/// diverge here first, at the park, one stage before the net count can show it.
#[test]
fn a_parked_terminal_payload_is_worth_one_retain_at_every_stage() {
    use crate::compiler::bytecode::{Bytecode, Instruction};

    for bits in [crate::value::fiber::SIG_OK, crate::value::fiber::SIG_HALT] {
        let mut vm = crate::vm::VM::new();
        let heap_ptr = vm.heap_ptr;
        let (payload, rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        macro_rules! rc {
            ($stage:expr, $want:expr) => {
                assert_eq!(
                    unsafe { &*heap_ptr }.region_rc(rid),
                    $want,
                    "{bits:?}: payload region rc {}",
                    $stage
                )
            };
        }
        rc!("as allocated — the test's own reference", 1);

        let mut bc = Bytecode::new();
        let idx = bc.add_constant(payload);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        if !bits.is_empty() {
            bc.emit(Instruction::Emit);
            bc.emit_signal_bits(bits);
        }
        bc.emit(Instruction::Return);
        let (handle, fiber_value) = child_fiber(unsafe { &mut *heap_ptr }, bc);
        rc!(
            "after the fiber is built — its code object names the payload",
            2
        );

        let (result_bits, _v) = vm.do_fiber_resume(&handle, fiber_value);
        assert_eq!(result_bits, bits, "the body leaves with exactly its signal");
        rc!(
            "after the fiber leaves — one park retain pins the result",
            3
        );

        vm.finalize_if_halted(&handle, result_bits);
        rc!("after the halt promotion — the result stays pinned", 3);

        release_fiber_value(unsafe { &mut *heap_ptr }, fiber_value);
        drop(handle);
        rc!(
            "after the fiber frees — its signal scan released the park and its \
             code object went with the region",
            1
        );

        unsafe { &mut *heap_ptr }.decref_region_if_present(rid);
        rc!("after the test's own release — nothing holds it", 0);
    }
}

/// A resume value delivered into a frame parked at a suspending PRIMITIVE call
/// arrives carrying one owning reference (docs/impl/region/park.md § "A delivery
/// into a replayed frame carries one owning reference").
///
/// The replayed frame re-enters at the parked call's continuation, which runs
/// that call's compiler-emitted result release. A bytecode callee funds the
/// reference that release consumes with its `Return` mint; a primitive that
/// suspends never returns, so the delivery mints it instead. Which shape a park
/// has is the classifier's answer, recorded in the delivery ledger, so the two
/// arms below park IDENTICAL frames and differ only in the ledger's
/// resume-funding fact — the record is the counter-factual. Both arms then
/// complete and park the same value
/// as their terminal result, which is worth its own single retain
/// (`a_parked_terminal_payload_is_worth_one_retain_at_every_stage`), so the whole
/// difference between the arms is the delivery's one reference.
#[test]
fn a_primitive_park_delivery_is_worth_one_retain() {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use crate::value::fiber::FiberStatus;
    use crate::value::{BytecodeFrame, SuspendedFrame};
    use std::rc::Rc;

    for unfunded in [false, true] {
        let mut vm = crate::vm::VM::new();
        let heap_ptr = vm.heap_ptr;
        let (payload, rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        macro_rules! rc {
            ($stage:expr, $want:expr) => {
                assert_eq!(
                    unsafe { &*heap_ptr }.region_rc(rid),
                    $want,
                    "unfunded={unfunded}: delivered region rc {}",
                    $stage
                )
            };
        }
        rc!("as allocated — the test's own reference", 1);

        // The parked continuation: the resume value is pushed as the suspended
        // call's result and returned, exactly as a body that binds nothing else
        // would.
        let mut body = Bytecode::new();
        body.emit(Instruction::Return);
        let (handle, fiber_value) = child_fiber(unsafe { &mut *heap_ptr }, body);
        let mut parked = Bytecode::new();
        parked.emit(Instruction::Return);
        let code = crate::value::ClosureTemplate::for_proto(
            unsafe { &mut *heap_ptr },
            &Rc::new(parked.into_proto()),
        )
        .code();
        let frame = BytecodeFrame::suspend(
            code,
            Rc::new(vec![]),
            0,
            vec![],
            true,
            rustc_hash::FxHashMap::default(),
            crate::value::fiber::ActivationDues::default(),
            crate::value::Value::NIL,
            unsafe { &*heap_ptr },
        );
        handle.with_mut(|f| {
            f.status = FiberStatus::Paused;
            f.suspended = Some(vec![SuspendedFrame::Bytecode(frame)]);
            // What `prim_fiber_resume` installs: the value to deliver, plus the
            // park shape the classifier recorded when the fiber suspended.
            f.signal = Some((crate::value::fiber::SIG_OK, payload));
            if unfunded {
                // The park's own payload is `nil` here: this face gauges the
                // resume funding, and an immediate payload's record names no
                // region, so it cannot move the counts below.
                f.delivery
                    .park_primitive(crate::value::fiber::SIG_YIELD, crate::value::Value::NIL);
            }
        });
        rc!("after the park is installed — the delivery has not run", 1);

        let (result_bits, v) = vm.do_fiber_resume(&handle, fiber_value);
        assert!(result_bits.is_empty(), "the replayed frame completes");
        assert_eq!(v, payload, "the resumed frame returns what it was handed");
        rc!(
            "after the resume — the terminal park retain, plus the delivery's \
             reference where the park owed one",
            if unfunded { 3 } else { 2 }
        );

        release_fiber_value(unsafe { &mut *heap_ptr }, fiber_value);
        drop(handle);
        unsafe { &mut *heap_ptr }.decref_region_if_present(rid);
        rc!(
            "after every holder releases — only an unconsumed delivery is left",
            if unfunded { 1 } else { 0 }
        );
    }
}
