// audited: 2026-09-19
//! The FIBER owner node: minted for a region the fiber itself owns, carried
//! across parks, and freed at the fiber's normal completion.
//!
//! docs/impl/region/owner.md

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

mod discard;
mod drop;
mod kill;
mod payload;
