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
        let (handle, fiber_value) = child_fiber(unsafe { &mut *heap_ptr }, fiber_body_closure(bc));

        // The fiber's owned state: a pages-less node with one adopted member.
        let (_member, member_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let node = unsafe { &mut *heap_ptr }.new_runtime_region();
        unsafe { &mut *heap_ptr }.adopt_region(node, member_rid);
        handle.with_mut(|f| f.fiber_owner_node = Some(node));
        let gen_before = unsafe { &*heap_ptr }.generation_raw(member_rid.get());

        let (bits, _v) = vm.do_fiber_resume(&handle, fiber_value);
        assert!(bits.is_ok(), "the noop fiber body completes");
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
        bc.emit_u16(crate::value::fiber::SIG_YIELD.raw() as u16);
        bc.emit(Instruction::Return);
        let (handle, fiber_value) = child_fiber(heap, fiber_body_closure(bc));

        // The fiber's own owned state.
        let (_mf, rid_f) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let node_f = unsafe { &mut *heap_ptr }.new_runtime_region();
        unsafe { &mut *heap_ptr }.adopt_region(node_f, rid_f);
        handle.with_mut(|f| f.fiber_owner_node = Some(node_f));

        let gen_a = unsafe { &*heap_ptr }.generation_raw(rid_a.get());
        let gen_f = unsafe { &*heap_ptr }.generation_raw(rid_f.get());

        let (bits, _v) = vm.do_fiber_resume(&handle, fiber_value);
        assert!(
            bits.contains(crate::value::fiber::SIG_YIELD),
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
        let code2 = crate::value::Code::new(
            Rc::new(bc2.instructions),
            Rc::new(bc2.constants),
            Rc::new(crate::error::LocationMap::new()),
            Rc::new(vec![]),
        );
        let frame2 = BytecodeFrame::suspend(
            code2,
            Rc::new(vec![]),
            0,
            vec![],
            true,
            rustc_hash::FxHashMap::default(),
            Some(node_b),
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
        assert!(bits.is_ok(), "the resumed two-frame chain completes");
        assert_eq!(handle.with(|f| f.status), FiberStatus::Dead);
        let bumped_a = unsafe { &*heap_ptr }.generation_raw(rid_a.get()) > gen_a;
        let bumped_b = unsafe { &*heap_ptr }.generation_raw(rid_b.get()) > gen_b;
        let bumped_f = unsafe { &*heap_ptr }.generation_raw(rid_f.get()) > gen_f;
        assert!(
            bumped_a && bumped_b,
            "each parked frame's activation node frees at that frame's completion \
             (frame 1 freed: {bumped_a}, frame 2 freed: {bumped_b})",
        );
        assert!(
            bumped_f,
            "the fiber node must ride the parks and free at the fiber's completion",
        );

        release_fiber_value(unsafe { &mut *heap_ptr }, fiber_value);
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
        bc.emit_u16(crate::value::fiber::SIG_YIELD.raw() as u16);
        bc.emit(Instruction::Return);
        let (handle, fiber_value) = child_fiber(heap, fiber_body_closure(bc));

        let (_mf, rid_f) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let node_f = unsafe { &mut *heap_ptr }.new_runtime_region();
        unsafe { &mut *heap_ptr }.adopt_region(node_f, rid_f);
        handle.with_mut(|f| f.fiber_owner_node = Some(node_f));
        let gen_a = unsafe { &*heap_ptr }.generation_raw(rid_a.get());
        let gen_f = unsafe { &*heap_ptr }.generation_raw(rid_f.get());

        let (bits, _v) = vm.do_fiber_resume(&handle, fiber_value);
        assert!(
            bits.contains(crate::value::fiber::SIG_YIELD),
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
        assert!(bits.is_ok(), "cancelling a parked fiber succeeds");
        unsafe { &mut *heap_ptr }.decref_region_if_present(ctx_region);
        assert_eq!(handle.with(|f| f.status), FiberStatus::Error);
        assert!(
            handle.with(|f| f.suspended.is_none()),
            "the cancel consumed the parked chain"
        );
        let bumped_a = unsafe { &*heap_ptr }.generation_raw(rid_a.get()) > gen_a;
        let bumped_f = unsafe { &*heap_ptr }.generation_raw(rid_f.get()) > gen_f;
        assert!(
            bumped_a && bumped_f,
            "the cancel must free the parked frame's node AND the fiber node \
             (parked member freed: {bumped_a}, fiber member freed: {bumped_f})",
        );
        release_fiber_value(unsafe { &mut *heap_ptr }, fiber_value);

        // ── fiber/abort of a not-yet-started fiber ──
        let mut bc = Bytecode::new();
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Return);
        let (handle, fiber_value) = child_fiber(unsafe { &mut *heap_ptr }, fiber_body_closure(bc));
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
        assert!(bits.is_ok(), "aborting a :new fiber succeeds");
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
    bc.emit_u16(crate::value::fiber::SIG_YIELD.raw() as u16);
    bc.emit(Instruction::Return);
    let (handle, fiber_value) = child_fiber(unsafe { &mut *heap_ptr }, fiber_body_closure(bc));

    // The payload lives in its OWN region, held live past the fiber's free by
    // an extra test reference — the live-frontier condition under which the
    // missing edge/retain is observable.
    let (payload, rid_p) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
    crate::value::arena::incref_region(unsafe { &mut *heap_ptr }, Some(rid_p));
    let gen_p = unsafe { &*heap_ptr }.generation_raw(rid_p.get());

    let (bits, _v) = vm.do_fiber_resume(&handle, fiber_value);
    assert!(
        bits.contains(crate::value::fiber::SIG_YIELD),
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
    assert!(bits.is_ok(), "cancelling a parked fiber succeeds");
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
    fn adopt_yield_code(child: crate::value::Value) -> crate::value::Code {
        let mut bc = Bytecode::new();
        let idx = bc.add_constant(child);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        bc.emit(Instruction::AdoptIntoActivation);
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Emit);
        bc.emit_u16(crate::value::fiber::SIG_YIELD.raw() as u16);
        bc.emit(Instruction::Return);
        crate::value::Code::new(
            Rc::new(bc.instructions),
            Rc::new(bc.constants),
            Rc::new(crate::error::LocationMap::new()),
            Rc::new(vec![]),
        )
    }

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    // ── single-frame chain: park, then discard ──
    for _ in 0..50 {
        let (child, child_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let gen_before = unsafe { &*heap_ptr }.generation_raw(child_rid.get());
        let code = adopt_yield_code(child);

        let result = vm.execute_bytecode_saving_stack(&code, &Rc::new(vec![]));
        assert!(
            result.bits.contains(crate::value::fiber::SIG_YIELD),
            "the body parks at the yield"
        );

        vm.discard_suspended_frames();
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
        vm.discard_suspended_frames();
    }

    // ── multi-frame chain: two parked activations, one discard frees both ──
    for _ in 0..50 {
        let (child_a, rid_a) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let (child_b, rid_b) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let gen_a = unsafe { &*heap_ptr }.generation_raw(rid_a.get());
        let gen_b = unsafe { &*heap_ptr }.generation_raw(rid_b.get());

        let result = vm.execute_bytecode_saving_stack(&adopt_yield_code(child_a), &Rc::new(vec![]));
        assert!(result.bits.contains(crate::value::fiber::SIG_YIELD));
        let mut chain = vm.fiber.suspended.take().expect("first park");

        let result = vm.execute_bytecode_saving_stack(&adopt_yield_code(child_b), &Rc::new(vec![]));
        assert!(result.bits.contains(crate::value::fiber::SIG_YIELD));
        chain.extend(vm.fiber.suspended.take().expect("second park"));

        vm.fiber.suspended = Some(chain);
        vm.discard_suspended_frames();
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
        bc.emit_u16(crate::value::fiber::SIG_YIELD.raw() as u16);
        bc.emit(Instruction::Return);
        let (handle, fiber_value) = child_fiber(heap, fiber_body_closure(bc));

        // The fiber's own owned state (the fiber-node tier).
        let (_mf, rid_f) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let node_f = unsafe { &mut *heap_ptr }.new_runtime_region();
        unsafe { &mut *heap_ptr }.adopt_region(node_f, rid_f);
        handle.with_mut(|f| f.fiber_owner_node = Some(node_f));

        let gen_a = unsafe { &*heap_ptr }.generation_raw(rid_a.get());
        let gen_f = unsafe { &*heap_ptr }.generation_raw(rid_f.get());

        let (bits, _v) = vm.do_fiber_resume(&handle, fiber_value);
        assert!(
            bits.contains(crate::value::fiber::SIG_YIELD),
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
        bc.emit_u16(crate::value::fiber::SIG_YIELD.raw() as u16);
        bc.emit(Instruction::Return);
        let (handle, fiber_value) = child_fiber(heap, fiber_body_closure(bc));

        let gen_p = unsafe { &*heap_ptr }.generation_raw(rid_p.get());
        let (bits, _v) = vm.do_fiber_resume(&handle, fiber_value);
        assert!(
            bits.contains(crate::value::fiber::SIG_YIELD),
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
