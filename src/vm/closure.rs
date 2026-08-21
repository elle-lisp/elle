use super::core::VM;
use crate::hir::region::{RuntimeRegion, StaticRegion};
use crate::value::arena;
use crate::value::closure::{ClosureTemplate, TemplateRef};
use crate::value::fiber::SignalBits;
use crate::value::heap::{Closure, HeapObject};
use crate::value::Value;

/// Materialize a closure instance for `MakeClosure`, into `region_id`.
///
/// The template `blueprint` is plain compile-time data (the enclosing code
/// object's `child_protos`). We materialize a FRESH `HeapObject::ClosureTemplate`
/// into the SAME region as the instance (co-region → the instance→template edge
/// is a self-edge, no cross-region RC), build the captured env inline, and
/// allocate the instance referencing the template. The template is therefore an
/// ordinary region allocation reclaimed by region RC when the instance's region
/// frees (a heap literal is an ordinary, reclaimable allocation; closure
/// templates are no exception). Allocates through the VM's heap
/// (`vm.heap_ptr`/`vm.heap()`), shared by the interpreter and the JIT
/// `MakeClosure` helper.
pub(crate) fn materialize_closure_in_region(
    heap: &mut crate::value::fiberheap::FiberHeap,
    blueprint: &ClosureTemplate,
    captures: &[Value],
    region_id: RuntimeRegion,
) -> Value {
    // Materialize the template heap object into the instance's region first, so
    // the instance's alloc-scan sees a live template Value (self-edge, filtered).
    let template_val = arena::alloc_in_region(
        heap,
        HeapObject::ClosureTemplate(blueprint.clone()),
        region_id,
    );
    let env = arena::alloc_region_slice_in_region::<Value>(heap, captures, region_id);
    let closure = Closure::new(TemplateRef::region(template_val), env, SignalBits::EMPTY);
    arena::alloc_in_region(
        heap,
        HeapObject::Closure {
            closure,
            traits: Value::NIL,
        },
        region_id,
    )
}

pub(crate) fn handle_make_closure(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    child_protos: &[std::rc::Rc<ClosureTemplate>],
    static_region: StaticRegion,
    merged_slots: &rustc_hash::FxHashSet<u32>,
) {
    let idx = vm.read_u16(bytecode, ip) as usize;
    let num_upvalues = vm.read_u16(bytecode, ip) as usize;

    // The nested-lambda blueprint registered for this MakeClosure at emit time.
    let blueprint = child_protos[idx].clone();

    // Collect captured values from stack
    let mut captured = Vec::with_capacity(num_upvalues);
    for _ in 0..num_upvalues {
        captured.push(
            vm.fiber
                .stack
                .pop()
                .expect("VM bug: Stack underflow on MakeClosure"),
        );
    }
    captured.reverse();

    // Resolve the closure's runtime region from its static slot, honouring the
    // closure-cycle merge's mint-or-reuse for a mutual-recursion SCC (`merged_slots`).
    // A self-recursive closure is cell-free — its self-reference resolves to the
    // executing closure, not a forward cell — so its region is an ordinary per-call
    // allocation reclaimed at its last use (the tail-call deferred release for a self-tail-loop,
    // `lir/lower/control/call.rs`).
    let region_id = vm.runtime_region_for_alloc_slot_maybe_merged(static_region, merged_slots);

    // `materialize_closure_in_region` allocates through the VM's heap
    // (`vm.heap_ptr`/`vm.heap()`), shared by interpreter and JIT.
    let val = materialize_closure_in_region(
        unsafe { &mut *vm.heap_ptr },
        &blueprint,
        &captured,
        region_id,
    );
    vm.fiber.stack.push(val);
}
