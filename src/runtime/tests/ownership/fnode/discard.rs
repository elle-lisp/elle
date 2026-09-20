// audited: 2026-09-19
//! What a discard frees: the parked activations' owner nodes, and the releases their abandoned frames still owed.
//!
//! docs/impl/region/mechanism.md

use super::*;

/// A squelch/abort DISCARD frees the parked owner node
/// (docs/impl/region/owner.md § "A discard runs what the abandoned frames
/// owed"). The hand-emitted body adopts a fresh-region member into the
/// activation's node and yields; instead of resuming, the park is abandoned
/// through the one discard chokepoint (`VM::discard_suspended_frames`, the
/// path `enforce_squelch` takes on a signal-violation). The discarded frame's
/// continuation never runs, so the completion release never fires — the
/// chokepoint must run it at the discard: node + member freed (generation
/// bump), live region count bounded across repeated park-discard cycles, and
/// a second discard is a no-op. The counterfactual is the discard itself: a
/// chokepoint that merely drops the frames strands the Owned member (no count
/// for any other release route to reach), the generation never bumps, and the
/// count grows by 2 per cycle. The multi-frame chain half pins the per-frame
/// loop: BOTH parked activations' nodes are freed, not just the first.
#[test]
fn discard_frees_parked_activation_owner_node() {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use std::rc::Rc;

    // The adopt-then-yield body every cycle parks (same shape as
    // `activation_owner_node_survives_yield_resume_completion`).
    fn adopt_yield_code(
        heap: &mut crate::value::fiberheap::FiberHeap,
        child: crate::value::Value,
    ) -> crate::value::Code {
        let mut bc = Bytecode::new();
        let idx = bc.add_constant(child);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        bc.emit(Instruction::AdoptIntoActivation);
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Emit);
        bc.emit_signal_bits(crate::value::fiber::SIG_YIELD);
        bc.emit(Instruction::Return);
        crate::value::ClosureTemplate::for_proto(heap, &Rc::new(bc.into_proto())).code()
    }

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    // ── single-frame chain: park, then discard ──
    for _ in 0..50 {
        let (child, child_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let gen_before = unsafe { &*heap_ptr }.generation_raw(child_rid.get());
        let code = adopt_yield_code(unsafe { &mut *heap_ptr }, child);

        let result = vm.execute_bytecode_saving_stack(&code, &Rc::new(vec![]));
        assert!(
            result.bits.intersects(crate::value::fiber::SIG_YIELD),
            "the body parks at the yield"
        );

        vm.discard_suspended_frames(crate::value::Value::NIL, None);
        assert!(
            vm.fiber.suspended.is_none(),
            "the discard consumed the parked chain"
        );
        let gen_after = unsafe { &*heap_ptr }.generation_raw(child_rid.get());
        assert!(
            gen_after > gen_before,
            "the adopted member's pages must be returned (generation bumped) by \
             the discard's subtree drop of the parked owner node \
             (gen {gen_before} -> {gen_after})",
        );
        // A second discard finds nothing — the release ran exactly once.
        vm.discard_suspended_frames(crate::value::Value::NIL, None);
    }

    // ── multi-frame chain: two parked activations, one discard frees both ──
    for _ in 0..50 {
        let (child_a, rid_a) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let (child_b, rid_b) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let gen_a = unsafe { &*heap_ptr }.generation_raw(rid_a.get());
        let gen_b = unsafe { &*heap_ptr }.generation_raw(rid_b.get());

        let result = vm.execute_bytecode_saving_stack(
            &adopt_yield_code(unsafe { &mut *heap_ptr }, child_a),
            &Rc::new(vec![]),
        );
        assert!(result.bits.intersects(crate::value::fiber::SIG_YIELD));
        let mut chain = vm.fiber.suspended.take().expect("first park");

        let result = vm.execute_bytecode_saving_stack(
            &adopt_yield_code(unsafe { &mut *heap_ptr }, child_b),
            &Rc::new(vec![]),
        );
        assert!(result.bits.intersects(crate::value::fiber::SIG_YIELD));
        chain.extend(vm.fiber.suspended.take().expect("second park"));

        vm.fiber.suspended = Some(chain);
        vm.discard_suspended_frames(crate::value::Value::NIL, None);
        let bumped_a = unsafe { &*heap_ptr }.generation_raw(rid_a.get()) > gen_a;
        let bumped_b = unsafe { &*heap_ptr }.generation_raw(rid_b.get()) > gen_b;
        assert!(
            bumped_a && bumped_b,
            "EVERY discarded frame's node must be freed, not just the first \
             (frame a freed: {bumped_a}, frame b freed: {bumped_b})",
        );
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "node + member must be reclaimed at each discard — live region count \
         must not grow (baseline={baseline}, after 100 park-discard cycles={after})",
    );
}

/// A squelch/abort DISCARD also runs the releases each abandoned frame still
/// owed, off the two tables its own `Code` records
/// (docs/impl/region/owner.md § "A discard runs what the abandoned frames
/// owed"). The frame is hand-built so both routes are present and each has a
/// neighbour the emitter did NOT record: slot 1 is a value route and slot 0 is
/// not, static region slot 9 is a slot route and slot 8 is not. The discard must
/// release exactly the two the tables name — running the untabled ones would
/// free a region whose release the frame's own machinery still answers for,
/// which is what a blanket release of the parked stack or the parked activation
/// map does.
///
/// The second half is the payload exemption: the value the exit leaves with
/// funds its reader's delivery out of the frame's own reference, so a table
/// entry naming its region stays owed.
#[test]
fn discard_runs_the_abandoned_frames_release_tables() {
    use crate::hir::region::{MappedRegion, RuntimeRegion};
    use crate::value::{
        Arity, BytecodeFrame, ClosureTemplate, SuspendedFrame, TemplateProto, Value,
    };
    use std::rc::Rc;

    /// A frame whose function releases value-route slot 1 and slot route 9, and
    /// nothing else.
    fn tabled_code(heap: &mut crate::value::fiberheap::FiberHeap) -> crate::value::Code {
        let proto = Rc::new(TemplateProto {
            frame_release_slots: vec![1],
            frame_release_regions: vec![9],
            ..TemplateProto::new(Vec::new(), Arity::Exact(0), Vec::new())
        });
        ClosureTemplate::for_proto(heap, &proto).code()
    }

    /// Park one frame holding `stack`, with `mapped` as its activation's
    /// static→physical remap.
    fn park(vm: &mut crate::vm::VM, stack: Vec<Value>, mapped: &[(u32, RuntimeRegion)]) {
        let map = mapped
            .iter()
            .map(|(slot, region)| {
                let gen = vm.heap().generation_raw(region.get());
                (*slot, MappedRegion::new(*region, gen))
            })
            .collect();
        let code = tabled_code(vm.heap());
        let frame = BytecodeFrame::suspend(
            code,
            Rc::new(vec![]),
            0,
            stack,
            true,
            map,
            crate::value::fiber::ActivationDues::default(),
            Value::NIL,
            vm.heap(),
        );
        vm.fiber.suspended = Some(vec![SuspendedFrame::Bytecode(frame)]);
    }

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;

    // ── both routes run, and only for the slots the tables name ──
    {
        let (untabled, untabled_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let (owed, owed_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        // Each slot-route region holds one object, so its birth reference is the
        // one the abandoned `DecrefRegion` would have dropped.
        let (_, mapped_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let (_, unmapped_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        park(
            &mut vm,
            vec![untabled, owed],
            &[(9, mapped_rid), (8, unmapped_rid)],
        );

        vm.discard_suspended_frames(Value::NIL, None);

        assert_eq!(
            vm.heap().region_rc(owed_rid),
            0,
            "the value route slot 1 names is a release the frame still owed",
        );
        assert_eq!(
            vm.heap().region_rc(mapped_rid),
            0,
            "the slot route static slot 9 names is one too — its receipt is the \
             mapping the release would have taken",
        );
        assert_eq!(
            vm.heap().region_rc(untabled_rid),
            1,
            "slot 0 is not in the value-route table, so the frame's reference to \
             what it holds stays standing",
        );
        assert_eq!(
            vm.heap().region_rc(unmapped_rid),
            1,
            "static slot 8 is not in the slot-route table, so its mapping is a \
             borrowed view the discard must not release",
        );
    }

    // ── the payload the exit leaves with is exempt ──
    {
        let (payload, payload_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        park(&mut vm, vec![Value::NIL, payload], &[]);

        vm.discard_suspended_frames(payload, None);

        assert_eq!(
            vm.heap().region_rc(payload_rid),
            1,
            "the skipped release is the delivery the payload's reader consumes",
        );
    }

    // ── unless the raise minted the delivery itself ──
    {
        let (payload, payload_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        park(&mut vm, vec![Value::NIL, payload], &[]);
        vm.fiber.delivery.record_mint(payload);

        vm.discard_suspended_frames(payload, None);

        assert_eq!(
            vm.heap().region_rc(payload_rid),
            0,
            "a recorded mint funds the delivery, so the frame's own reference is \
             reclaimed at the discard too",
        );
    }

    // ── the park's own references, beside the frames' ──
    // A body-allocated park: its payload sits in the frame's value-route slot,
    // so the table drops the body's reference and the chokepoint drops the
    // delivery retain the boundary left with no reader
    // (docs/impl/region/park.md § "A boundary ends a park with no reader and
    // no install"). The exit's own payload is a different value here — the
    // boundary's `signal-violation`, which never shares the park's region.
    {
        let (parked, parked_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let (violation, _) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        unsafe { &mut *heap_ptr }.incref_region(parked_rid);
        park(&mut vm, vec![Value::NIL, parked], &[]);
        vm.fiber
            .delivery
            .park_emit(crate::value::fiber::SIG_YIELD, parked);

        vm.discard_suspended_frames(violation, Some((crate::value::fiber::SIG_YIELD, parked)));

        assert_eq!(
            vm.heap().region_rc(parked_rid),
            0,
            "the frame's table drops the body's reference and the chokepoint the \
             delivery retain — two references, two seams the boundary cut",
        );
    }

    // A boundary that ends NO park releases nothing extra, whatever the ledger
    // last recorded. The two records decide together: the exit names the park it
    // is ending, and a record that does not name it is a park some other route
    // already ended.
    {
        let (stale, stale_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        park(&mut vm, vec![Value::NIL, Value::NIL], &[]);
        vm.fiber
            .delivery
            .park_emit(crate::value::fiber::SIG_YIELD, stale);

        vm.discard_suspended_frames(Value::NIL, None);

        assert_eq!(
            vm.heap().region_rc(stale_rid),
            1,
            "a stale record names a park this exit is not ending",
        );
    }
}
