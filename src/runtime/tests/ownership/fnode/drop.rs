// audited: 2026-09-19
//! What the region free path discharges when a parked fiber's last reference drops.
//!
//! docs/impl/region/park.md

use super::*;

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
/// escape retain (docs/impl/region/park.md — the
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
