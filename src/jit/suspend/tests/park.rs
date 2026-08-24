//! The JIT yield side-exits PARK the activation's owner node
//! (docs/impl/region/owner.md § "Owner nodes" — "A park moves the node into
//! the suspended frame"), the compiled-tier twin of the interpreter's
//! `handle_emit` take. Each test runs exactly the helper sequence compiled
//! code emits — the prologue's region-map push, an `AdoptIntoActivation` that
//! lazily mints the node, the suspend helper, the side-exit's region-map pop —
//! then resumes through the interpreter (the template bytecode is a bare
//! `Return`) and asserts the completion frees node + member: the member's
//! generation bumps and the live region count stays bounded. A side-exit that
//! dropped the node instead of parking it strands the Owned member (its count
//! was consumed by the adopt, so NOTHING else reclaims it): the generation
//! never bumps and the count grows by 2 per park.

use super::*;
use crate::compiler::bytecode::Instruction;
use crate::jit::dispatch::{
    elle_jit_adopt_into_activation, elle_jit_pop_region_map, elle_jit_push_region_map,
};
use crate::value::arena::alloc_in_fresh_region;
use crate::value::heap::{HeapObject, Pair};

/// Allocate a member pair in its own fresh region on the VM's heap, returning
/// (value, region, generation-at-birth).
fn fresh_member(
    heap: &mut crate::value::fiberheap::FiberHeap,
) -> (Value, crate::hir::region::RuntimeRegion, u32) {
    let (child, child_rid) =
        alloc_in_fresh_region(heap, HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
    let gen = heap.generation_raw(child_rid.get());
    (child, child_rid, gen)
}

#[test]
fn jit_yield_parks_owner_node_for_resume_completion() {
    let yield_meta = YieldPointMeta {
        resume_ip: 0,
        num_spilled: 0,
        num_locals: 0,
        num_params: 0,
    };
    let (mut vm, closure_val) = setup_yield_test(
        vec![Instruction::Return as u8],
        vec![],
        vec![],
        vec![yield_meta],
    );
    let heap_ptr = vm.heap_ptr;
    let vm_ptr = &mut vm as *mut crate::vm::VM as *mut ();
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    for _ in 0..50 {
        // The compiled prologue: push this activation's region-map frame
        // (with its empty owner-node slot).
        elle_jit_push_region_map(vm_ptr);

        let (child, child_rid, gen_before) = fresh_member(unsafe { &mut *heap_ptr });
        elle_jit_adopt_into_activation(child.tag, child.payload, vm_ptr);

        let r = elle_jit_yield(
            Value::NIL.tag,
            Value::NIL.payload,
            std::ptr::null(),
            0,
            vm_ptr as u64,
            closure_val.tag,
            closure_val.payload,
            SIG_YIELD.raw(),
        );
        assert_eq!(r, YIELD_SENTINEL);
        // The side-exit's epilogue: pop the region-map frame AFTER the
        // suspend captured it (`emit_pop_then_return`).
        elle_jit_pop_region_map(vm_ptr);

        assert_eq!(
            unsafe { &*heap_ptr }.generation_raw(child_rid.get()),
            gen_before,
            "the adopted member must stay live while the fiber is parked",
        );

        let frames = vm.fiber.suspended.take().expect("the yield parked a frame");
        let bits = vm.resume_suspended(frames, Value::NIL);
        assert!(bits.is_empty(), "the resumed body completes normally");
        let gen_after = unsafe { &*heap_ptr }.generation_raw(child_rid.get());
        assert!(
            gen_after > gen_before,
            "a member adopted in a compiled body must be freed at the resumed \
             activation's completion — the JIT yield side-exit must park the \
             node (gen {gen_before} -> {gen_after})",
        );
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "node + member must be reclaimed at each parked-and-resumed compiled \
         activation's completion — live region count must not grow \
         (baseline={baseline}, after 50 parks={after})",
    );
}

#[test]
fn jit_yield_through_call_parks_owner_node_for_resume_completion() {
    use crate::jit::dispatch::CallSiteMeta;
    use std::sync::Arc;

    // setup_yield_test wires a yield-points JitCode; this test drives the
    // CALL-SITE helper, so re-key the cache entry with call-site metadata.
    let (mut vm, closure_val) =
        setup_yield_test(vec![Instruction::Return as u8], vec![], vec![], vec![]);
    let bytecode = closure_val
        .as_closure()
        .expect("setup builds a closure")
        .template
        .bytecode
        .clone();
    vm.install_jit_code(
        bytecode,
        Arc::new(crate::jit::JitCode::test_with_call_sites(vec![
            CallSiteMeta {
                resume_ip: 0,
                num_spilled: 0,
                num_locals: 0,
                num_params: 0,
            },
        ])),
    );
    let heap_ptr = vm.heap_ptr;
    let vm_ptr = &mut vm as *mut crate::vm::VM as *mut ();
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    for _ in 0..50 {
        elle_jit_push_region_map(vm_ptr);
        let (child, child_rid, gen_before) = fresh_member(unsafe { &mut *heap_ptr });
        elle_jit_adopt_into_activation(child.tag, child.payload, vm_ptr);

        // A callee suspended (its own park already in fiber.suspended in the
        // real flow — absent here, the helper starts the chain); the JIT
        // caller appends its continuation frame and unwinds.
        let r = elle_jit_yield_through_call(
            std::ptr::null(),
            0,
            vm_ptr as u64,
            closure_val.tag,
            closure_val.payload,
        );
        assert_eq!(r, YIELD_SENTINEL);
        elle_jit_pop_region_map(vm_ptr);

        assert_eq!(
            unsafe { &*heap_ptr }.generation_raw(child_rid.get()),
            gen_before,
            "the adopted member must stay live while the fiber is parked",
        );

        let frames = vm
            .fiber
            .suspended
            .take()
            .expect("the caller frame was parked");
        let bits = vm.resume_suspended(frames, Value::NIL);
        assert!(bits.is_empty(), "the resumed caller completes normally");
        let gen_after = unsafe { &*heap_ptr }.generation_raw(child_rid.get());
        assert!(
            gen_after > gen_before,
            "a member adopted in a compiled caller must be freed at the resumed \
             activation's completion — the yield-through-call side-exit must \
             park the node (gen {gen_before} -> {gen_after})",
        );
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "node + member must be reclaimed at each parked-and-resumed compiled \
         activation's completion — live region count must not grow \
         (baseline={baseline}, after 50 parks={after})",
    );
}
