use super::*;

/// Release the per-cycle fiber VALUE's region (the `Alloc` ctx minted it), so
/// the bounded-count loops measure only what the teardown under test leaves.
fn release_fiber_value(
    heap: &mut crate::value::fiberheap::FiberHeap,
    fiber_value: crate::value::Value,
) {
    if let Some(r) = crate::value::arena::region_of(heap, fiber_value) {
        heap.decref_region_if_present(r);
    }
}

/// The FIBER owner node is freed at the fiber's normal completion
/// (docs/impl/region/owner.md § "Owner nodes" — "Fiber teardown frees everything
/// the fiber owns"). No production lowering targets the fiber node yet, so the
/// test stands in for the cross-fiber ownership cuts: it mints the node, adopts
/// a fresh-region member into it, and runs the fiber to completion
/// (`do_fiber_resume`). The `:dead` transition must free node + member — the
/// member's generation bumps — and the live region count stays bounded across
/// 50 fibers. The counterfactual is the adopt itself: the member is Owned (its
/// count consumed), so if the completion teardown does not fire, NOTHING
/// reclaims it and the count grows every cycle.
#[test]
fn fiber_owner_node_freed_at_fiber_completion() {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use crate::value::fiber::FiberStatus;

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    for _ in 0..50 {
        // The body: a noop thunk that completes immediately.
        let mut bc = Bytecode::new();
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Return);
        let (handle, fiber_value) = child_fiber(unsafe { &mut *heap_ptr }, bc);

        // The fiber's owned state: a pages-less node with one adopted member.
        let (_member, member_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let node = unsafe { &mut *heap_ptr }.new_runtime_region();
        unsafe { &mut *heap_ptr }.adopt_region(node, member_rid);
        handle.with_mut(|f| f.fiber_owner_node = Some(node));
        let gen_before = unsafe { &*heap_ptr }.generation_raw(member_rid.get());

        let (bits, _v) = vm.do_fiber_resume(&handle, fiber_value);
        assert!(bits.is_empty(), "the noop fiber body completes");
        assert_eq!(handle.with(|f| f.status), FiberStatus::Dead);
        let gen_after = unsafe { &*heap_ptr }.generation_raw(member_rid.get());
        assert!(
            gen_after > gen_before,
            "the fiber-node member's pages must be returned (generation bumped) \
             by the fiber's completion teardown (gen {gen_before} -> {gen_after})",
        );

        release_fiber_value(unsafe { &mut *heap_ptr }, fiber_value);
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "fiber node + member must be reclaimed at each fiber's completion — live \
         region count must not grow (baseline={baseline}, after 50 fibers={after})",
    );
}

/// The fiber owner node SURVIVES parks — it is fiber state, riding suspension
/// structurally — and is freed at the resumed fiber's completion, alongside a
/// MULTI-FRAME parked chain whose per-frame activation nodes each reclaim at
/// their own frame's completion (docs/impl/region/owner.md § "Owner nodes").
/// The body adopts a member into its ACTIVATION node and yields (frame 1); a
/// second hand-built frame carrying its own node + member is appended (the
/// outer-caller shape of a yield-through chain); the fiber node holds a third
/// member. Across the park all three stay live (Owned, RC frozen — no other
/// release route); the resume replays both frames to completion, freeing each
/// frame's node at that frame's completion and the FIBER node at `:dead`.
/// The counterfactual is the fiber-node half: without the completion teardown
/// its member's generation never bumps and the count grows per cycle.
#[test]
fn fiber_owner_node_survives_parks_and_frees_at_completion() {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use crate::value::fiber::FiberStatus;
    use crate::value::{BytecodeFrame, SuspendedFrame};
    use std::rc::Rc;

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    for _ in 0..50 {
        let heap = unsafe { &mut *heap_ptr };

        // Frame-1 body: adopt member_a into the activation node, yield, return
        // the resume value.
        let (member_a, rid_a) = alloc_in_fresh_region(heap, cons());
        let mut bc = Bytecode::new();
        let idx = bc.add_constant(member_a);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        bc.emit(Instruction::AdoptIntoActivation);
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Emit);
        bc.emit_signal_bits(crate::value::fiber::SIG_YIELD);
        bc.emit(Instruction::Return);
        let (handle, fiber_value) = child_fiber(heap, bc);

        // The fiber's own owned state.
        let (_mf, rid_f) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let node_f = unsafe { &mut *heap_ptr }.new_runtime_region();
        unsafe { &mut *heap_ptr }.adopt_region(node_f, rid_f);
        handle.with_mut(|f| f.fiber_owner_node = Some(node_f));

        let gen_a = unsafe { &*heap_ptr }.generation_raw(rid_a.get());
        let gen_f = unsafe { &*heap_ptr }.generation_raw(rid_f.get());

        let (bits, _v) = vm.do_fiber_resume(&handle, fiber_value);
        assert!(
            bits.intersects(crate::value::fiber::SIG_YIELD),
            "the body parks at the yield"
        );
        assert_eq!(handle.with(|f| f.status), FiberStatus::Paused);
        assert_eq!(
            unsafe { &*heap_ptr }.generation_raw(rid_a.get()),
            gen_a,
            "the parked frame's adopted member stays live across the park",
        );
        assert_eq!(
            unsafe { &*heap_ptr }.generation_raw(rid_f.get()),
            gen_f,
            "the fiber-node member stays live across the park",
        );

        // Frame 2: a hand-built outer activation parked with its own node +
        // member — the multi-frame chain of a yield through a call.
        let (_mb, rid_b) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let node_b = unsafe { &mut *heap_ptr }.new_runtime_region();
        unsafe { &mut *heap_ptr }.adopt_region(node_b, rid_b);
        let gen_b = unsafe { &*heap_ptr }.generation_raw(rid_b.get());
        let mut bc2 = Bytecode::new();
        bc2.emit(Instruction::Return);
        let code2 = crate::value::ClosureTemplate::for_proto(
            unsafe { &mut *heap_ptr },
            &Rc::new(bc2.into_proto()),
        )
        .code();
        let frame2 = BytecodeFrame::suspend(
            code2,
            Rc::new(vec![]),
            0,
            vec![],
            true,
            rustc_hash::FxHashMap::default(),
            crate::value::fiber::ActivationDues::with_owner_node(node_b),
            crate::value::Value::NIL,
            unsafe { &*heap_ptr },
        );
        handle.with_mut(|f| {
            f.suspended
                .as_mut()
                .expect("the yield parked a chain")
                .push(SuspendedFrame::Bytecode(frame2));
        });

        let (bits, _v) = vm.do_fiber_resume(&handle, fiber_value);
        assert!(bits.is_empty(), "the resumed two-frame chain completes");
        assert_eq!(handle.with(|f| f.status), FiberStatus::Dead);
        let bumped_b = unsafe { &*heap_ptr }.generation_raw(rid_b.get()) > gen_b;
        let bumped_f = unsafe { &*heap_ptr }.generation_raw(rid_f.get()) > gen_f;
        assert!(
            bumped_b,
            "a parked frame's activation node frees its member at that frame's \
             completion",
        );
        assert!(
            bumped_f,
            "the fiber node must ride the parks and free at the fiber's completion",
        );

        // The trap: frame 1's member reaches its body through the code object's
        // constant pool — the only channel hand-built bytecode has — and a code
        // object is a region citizen now (docs/impl/region/template.md), so its
        // region holds a live reference the node's drop must respect. The member
        // is therefore RESCUED at that drop and freed when the code object goes
        // (docs/impl/region/ownership.md § "The incoming edge table and the
        // external-reference rescue"). Frame 2's member, adopted from outside
        // the bytecode, is what pins the at-completion timing above.
        release_fiber_value(unsafe { &mut *heap_ptr }, fiber_value);
        assert!(
            unsafe { &*heap_ptr }.generation_raw(rid_a.get()) > gen_a,
            "a rescued member frees when the last reference to it goes",
        );
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "every node + member must be reclaimed by the parked-and-resumed fiber's \
         completion — live region count must not grow (baseline={baseline}, after \
         50 cycles={after})",
    );
}

/// A hard kill frees everything the fiber owns: `fiber/cancel` of a PARKED
/// fiber releases both the parked frame's activation owner node and the fiber
/// owner node (gathered under it by `reparent_owned_children` — one set-drop),
/// and `fiber/abort` of a not-yet-started fiber releases the fiber node
/// (docs/impl/region/owner.md § "Owner nodes" — "Fiber teardown frees
/// everything the fiber owns"). Both route through `kill_fiber`; before it, the
/// cancel arm dropped the chain bare (`suspended = None`), stranding every
/// parked node. The counterfactual is exactly that strand: without the
/// teardown, no generation bumps and the count grows per cycle.
#[test]
fn fiber_kill_frees_parked_and_fiber_owned() {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use crate::value::fiber::FiberStatus;

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let vm_ptr: *mut crate::vm::VM = &mut vm;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    for _ in 0..50 {
        // ── fiber/cancel of a parked fiber ──
        let heap = unsafe { &mut *heap_ptr };
        let (member_a, rid_a) = alloc_in_fresh_region(heap, cons());
        let mut bc = Bytecode::new();
        let idx = bc.add_constant(member_a);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        bc.emit(Instruction::AdoptIntoActivation);
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Emit);
        bc.emit_signal_bits(crate::value::fiber::SIG_YIELD);
        bc.emit(Instruction::Return);
        let (handle, fiber_value) = child_fiber(heap, bc);

        let (_mf, rid_f) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let node_f = unsafe { &mut *heap_ptr }.new_runtime_region();
        unsafe { &mut *heap_ptr }.adopt_region(node_f, rid_f);
        handle.with_mut(|f| f.fiber_owner_node = Some(node_f));
        let gen_a = unsafe { &*heap_ptr }.generation_raw(rid_a.get());
        let gen_f = unsafe { &*heap_ptr }.generation_raw(rid_f.get());

        let (bits, _v) = vm.do_fiber_resume(&handle, fiber_value);
        assert!(
            bits.intersects(crate::value::fiber::SIG_YIELD),
            "the body parks at the yield"
        );

        // Cancel through the primitive — the production hard-kill path.
        let ctx_region = unsafe { &mut *heap_ptr }.new_runtime_region();
        let (bits, _v) = {
            let mut ctx = crate::primitives::ctx::NativeCtx::with_region_vm(
                ctx_region,
                unsafe { &mut *heap_ptr },
                vm_ptr,
            );
            crate::primitives::fiber_introspect::prim_fiber_cancel(&mut ctx, &[fiber_value])
        };
        assert!(bits.is_empty(), "cancelling a parked fiber succeeds");
        unsafe { &mut *heap_ptr }.decref_region_if_present(ctx_region);
        assert_eq!(handle.with(|f| f.status), FiberStatus::Error);
        assert!(
            handle.with(|f| f.suspended.is_none()),
            "the cancel consumed the parked chain"
        );
        assert!(
            unsafe { &*heap_ptr }.generation_raw(rid_f.get()) > gen_f,
            "the cancel must free the fiber node",
        );
        // The parked frame's member reaches its body through the code object's
        // constant pool — the only channel hand-built bytecode has — so the code
        // object's region references it and the cancel's set drop rescues it
        // rather than freeing it (docs/impl/region/ownership.md § "The incoming
        // edge table and the external-reference rescue"). It frees with the code
        // object, which the fiber value's release takes.
        release_fiber_value(unsafe { &mut *heap_ptr }, fiber_value);
        assert!(
            unsafe { &*heap_ptr }.generation_raw(rid_a.get()) > gen_a,
            "the cancel must leave the parked frame's member with no surviving \
             reference: it frees as soon as the code object that rescued it goes",
        );

        // ── fiber/abort of a not-yet-started fiber ──
        let mut bc = Bytecode::new();
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Return);
        let (handle, fiber_value) = child_fiber(unsafe { &mut *heap_ptr }, bc);
        let (_mn, rid_n) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let node_n = unsafe { &mut *heap_ptr }.new_runtime_region();
        unsafe { &mut *heap_ptr }.adopt_region(node_n, rid_n);
        handle.with_mut(|f| f.fiber_owner_node = Some(node_n));
        let gen_n = unsafe { &*heap_ptr }.generation_raw(rid_n.get());

        let ctx_region = unsafe { &mut *heap_ptr }.new_runtime_region();
        let (bits, _v) = {
            let mut ctx = crate::primitives::ctx::NativeCtx::with_region_vm(
                ctx_region,
                unsafe { &mut *heap_ptr },
                vm_ptr,
            );
            crate::primitives::fiber_introspect::prim_fiber_abort(&mut ctx, &[fiber_value])
        };
        assert!(bits.is_empty(), "aborting a :new fiber succeeds");
        unsafe { &mut *heap_ptr }.decref_region_if_present(ctx_region);
        assert_eq!(handle.with(|f| f.status), FiberStatus::Error);
        assert!(
            unsafe { &*heap_ptr }.generation_raw(rid_n.get()) > gen_n,
            "aborting a never-started fiber must free its fiber node's members",
        );
        release_fiber_value(unsafe { &mut *heap_ptr }, fiber_value);
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "the hard kills must reclaim everything each fiber owned — live region \
         count must not grow (baseline={baseline}, after 50 cycles={after})",
    );
}

/// `kill_fiber` parks the cancel payload as the fiber's TERMINAL signal, so it
/// owes the SAME park-retain + recorded content edge the normal completion path
/// takes (`do_fiber_resume` step 6a): the fiber's free releases the payload's
/// region once (the recorded-edge cascade / the object scan's Fiber signal arm),
/// so without the pair a heap payload in a LIVE foreign region is (a) an
/// unrecorded content edge — the debug equivalence oracle detonates at the fiber
/// region's free — and (b) an over-free of the payload's region (a scan decref
/// with no matching incref). Historically masked by the borrowed tail-arg leak,
/// which pinned every cancelled fiber's region so the free never ran.
#[test]
fn fiber_kill_park_retains_terminal_payload() {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use crate::value::fiber::FiberStatus;

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let vm_ptr: *mut crate::vm::VM = &mut vm;

    // A body that parks at a yield, so the cancel takes the hard-kill arm.
    let mut bc = Bytecode::new();
    bc.emit(Instruction::Nil);
    bc.emit(Instruction::Emit);
    bc.emit_signal_bits(crate::value::fiber::SIG_YIELD);
    bc.emit(Instruction::Return);
    let (handle, fiber_value) = child_fiber(unsafe { &mut *heap_ptr }, bc);

    // The payload lives in its OWN region, held live past the fiber's free by
    // an extra test reference — the live-frontier condition under which the
    // missing edge/retain is observable.
    let (payload, rid_p) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
    crate::value::arena::incref_region(unsafe { &mut *heap_ptr }, Some(rid_p));
    let gen_p = unsafe { &*heap_ptr }.generation_raw(rid_p.get());

    let (bits, _v) = vm.do_fiber_resume(&handle, fiber_value);
    assert!(
        bits.intersects(crate::value::fiber::SIG_YIELD),
        "the body parks at the yield"
    );

    let rc_before = region_rc(unsafe { &*heap_ptr }, rid_p);
    let ctx_region = unsafe { &mut *heap_ptr }.new_runtime_region();
    let (bits, _v) = {
        let mut ctx = crate::primitives::ctx::NativeCtx::with_region_vm(
            ctx_region,
            unsafe { &mut *heap_ptr },
            vm_ptr,
        );
        crate::primitives::fiber_introspect::prim_fiber_cancel(&mut ctx, &[fiber_value, payload])
    };
    assert!(bits.is_empty(), "cancelling a parked fiber succeeds");
    unsafe { &mut *heap_ptr }.decref_region_if_present(ctx_region);
    assert_eq!(handle.with(|f| f.status), FiberStatus::Error);

    // The park-retain: exactly one owning reference for the parked terminal
    // signal (the counterfactual — kill_fiber without it leaves the rc flat,
    // and the fiber's free then underflows it).
    let rc_parked = region_rc(unsafe { &*heap_ptr }, rid_p);
    assert_eq!(
        rc_parked,
        rc_before + 1,
        "kill_fiber must park-retain the terminal payload's region"
    );

    // Free the fiber's region: the recorded-edge cascade releases the payload
    // exactly once (and the debug equivalence oracle asserts the recorded
    // table matches the content scan — an unrecorded signal edge aborts here).
    release_fiber_value(unsafe { &mut *heap_ptr }, fiber_value);
    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, rid_p),
        rc_before,
        "the fiber's free must release the park-retain exactly once"
    );
    assert_eq!(
        unsafe { &*heap_ptr }.generation_raw(rid_p.get()),
        gen_p,
        "the payload's region must survive the fiber (the test still holds it)"
    );

    // Drop the test's references: mint + extra — the payload frees only now.
    crate::value::arena::decref_if_present(unsafe { &mut *heap_ptr }, rid_p);
    crate::value::arena::decref_if_present(unsafe { &mut *heap_ptr }, rid_p);
    assert!(
        unsafe { &*heap_ptr }.generation_raw(rid_p.get()) > gen_p,
        "the payload's region frees once the test's references drop"
    );
}

/// A squelch/abort DISCARD frees the parked owner node
/// (docs/impl/region/owner.md § "Owner nodes" — "A discard frees the parked
/// node"). The hand-emitted body adopts a fresh-region member into the
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
    // (docs/impl/region/owner.md § "A boundary ends a park with no reader and
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

/// A parked fiber whose last counted reference DROPS — no terminal transition
/// ever runs — discharges its owned state at the region free
/// (docs/impl/region/owner.md § "The free-path fiber discharge"): the parked
/// frame's activation owner node and the fiber owner node are taken out of the
/// dying fiber (`Fiber::take_parked_state`) and fed to the free's cascade, so
/// their adopted members' pages return (generation bump) and the live count
/// stays bounded. The counterfactual is the discharge itself: without it, the
/// dropped fiber's `Fiber` object dies with the parked chain still holding the
/// nodes — Owned members with no other release route — and the count grows by
/// 4 per cycle.
#[test]
fn dropped_parked_fiber_discharges_owned_state() {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use crate::value::fiber::FiberStatus;

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    for _ in 0..50 {
        let heap = unsafe { &mut *heap_ptr };

        // The body adopts a member into the activation node and parks at a yield.
        let (member_a, rid_a) = alloc_in_fresh_region(heap, cons());
        let mut bc = Bytecode::new();
        let idx = bc.add_constant(member_a);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        bc.emit(Instruction::AdoptIntoActivation);
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Emit);
        bc.emit_signal_bits(crate::value::fiber::SIG_YIELD);
        bc.emit(Instruction::Return);
        let (handle, fiber_value) = child_fiber(heap, bc);

        // The fiber's own owned state (the fiber-node tier).
        let (_mf, rid_f) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let node_f = unsafe { &mut *heap_ptr }.new_runtime_region();
        unsafe { &mut *heap_ptr }.adopt_region(node_f, rid_f);
        handle.with_mut(|f| f.fiber_owner_node = Some(node_f));

        let gen_a = unsafe { &*heap_ptr }.generation_raw(rid_a.get());
        let gen_f = unsafe { &*heap_ptr }.generation_raw(rid_f.get());

        let (bits, _v) = vm.do_fiber_resume(&handle, fiber_value);
        assert!(
            bits.intersects(crate::value::fiber::SIG_YIELD),
            "the body parks at the yield"
        );
        assert_eq!(handle.with(|f| f.status), FiberStatus::Paused);

        // DROP: release the fiber value's region — the parked fiber's only
        // counted reference — with no teardown call. The free-path discharge
        // must take and release the parked node + fiber node.
        release_fiber_value(unsafe { &mut *heap_ptr }, fiber_value);

        let bumped_a = unsafe { &*heap_ptr }.generation_raw(rid_a.get()) > gen_a;
        let bumped_f = unsafe { &*heap_ptr }.generation_raw(rid_f.get()) > gen_f;
        assert!(
            bumped_a && bumped_f,
            "dropping a parked fiber must discharge its parked activation node \
             (freed: {bumped_a}) and fiber node (freed: {bumped_f})",
        );
        assert!(
            handle.with(|f| f.suspended.is_none() && f.fiber_owner_node.is_none()),
            "the discharge takes the parked state, so no second release path \
             can reach it",
        );
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "every dropped parked fiber's nodes + members must be reclaimed — live \
         region count must not grow (baseline={baseline}, after 50 cycles={after})",
    );
}

/// A dropped parked fiber's non-terminal SIGNAL value releases its one park
/// escape retain (docs/impl/region/owner.md § "Park/unpark symmetry" — the
/// third rule). The body yields a heap value: `handle_emit` retains its region
/// (`EmitEscape`) as it escapes into `fiber.signal`, and the resume path that
/// would consume the retain never runs. Dropping the fiber must release it —
/// afterwards the test's own alloc reference is the region's last, so one
/// decref frees it (generation bump). The counterfactual: without the
/// discharge the retain dangles and the region survives both releases.
#[test]
fn dropped_parked_fiber_releases_signal_escape_retain() {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use crate::value::fiber::FiberStatus;

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    for _ in 0..50 {
        let heap = unsafe { &mut *heap_ptr };

        // The body yields a heap value out of a region the TEST owns.
        let (payload, rid_p) = alloc_in_fresh_region(heap, cons());
        let mut bc = Bytecode::new();
        let idx = bc.add_constant(payload);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        bc.emit(Instruction::Emit);
        bc.emit_signal_bits(crate::value::fiber::SIG_YIELD);
        bc.emit(Instruction::Return);
        let (handle, fiber_value) = child_fiber(heap, bc);

        let gen_p = unsafe { &*heap_ptr }.generation_raw(rid_p.get());
        let (bits, _v) = vm.do_fiber_resume(&handle, fiber_value);
        assert!(
            bits.intersects(crate::value::fiber::SIG_YIELD),
            "the body parks at the yield"
        );
        assert_eq!(handle.with(|f| f.status), FiberStatus::Paused);

        // DROP the parked fiber, then release the test's own alloc reference.
        // The discharge released the EmitEscape retain, so this is the last.
        release_fiber_value(unsafe { &mut *heap_ptr }, fiber_value);
        unsafe { &mut *heap_ptr }.decref_region_if_present(rid_p);
        assert!(
            unsafe { &*heap_ptr }.generation_raw(rid_p.get()) > gen_p,
            "the yielded value's park escape retain must be released by the \
             dropped fiber's discharge — the region survives its last release \
             otherwise",
        );
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "every dropped parked fiber's yielded-value region must be reclaimed \
         (baseline={baseline}, after 50 cycles={after})",
    );
}

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
/// arrives carrying one owning reference (docs/impl/region/owner.md § "A delivery
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
