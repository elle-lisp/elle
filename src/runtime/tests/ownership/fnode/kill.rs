// audited: 2026-09-19
//! What a hard kill of a parked fiber frees, and the one payload it must retain instead.
//!
//! docs/impl/region/park.md

use super::*;

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
