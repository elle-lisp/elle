// audited: 2026-09-14
// docs/impl/region/template.md
// docs/impl/image/sealing.md
//! What `MakeClosure` builds: a closure instance and the fresh header it
//! references, both in the instruction's own region.

use super::core::VM;
use crate::hir::region::{RuntimeRegion, StaticRegion};
use crate::value::arena;
use crate::value::closure::{materialize, ChildCode, TemplateRef};
use crate::value::fiber::SignalBits;
use crate::value::heap::{Closure, HeapObject};
use crate::value::Value;

/// Materialize a closure instance for `MakeClosure`, into `region_id`.
///
/// `child` is the code object the instruction indexes, from whichever side of
/// the enclosing header answers (docs/impl/image/sealing.md). We materialize a
/// FRESH `HeapObject::ClosureTemplate` header
/// into the SAME region as the instance (co-region → the instance→template edge
/// is a self-edge, no cross-region RC), build the captured env inline, and
/// allocate the instance referencing the header. The header is therefore an
/// ordinary region allocation reclaimed by region RC when the instance's region
/// frees (a heap literal is an ordinary, reclaimable allocation; closure
/// templates are no exception).
///
/// The header's *payload* — bytecode, constants, location table, region tables
/// — is not copied here. It is materialized once per blueprint into a payload
/// region of the heap's own and shared by every header
/// (docs/impl/region/template.md), so a closure built in a loop costs one
/// header allocation per iteration rather than a copy of its function's code.
/// A hydrated child's payload is already in the image's pages, so the fresh
/// header takes a counted reference to that region and copies nothing either.
/// Allocates through the VM's heap (`vm.heap_ptr`/`vm.heap()`), shared by the
/// interpreter and the JIT `MakeClosure` helper.
pub(crate) fn materialize_closure_in_region(
    heap: &mut crate::value::fiberheap::FiberHeap,
    child: ChildCode<'_>,
    captures: &[Value],
    region_id: RuntimeRegion,
) -> Value {
    // Materialize the header into the instance's region first, so the
    // instance's alloc-scan sees a live template Value (self-edge, filtered).
    let template_val = match child {
        ChildCode::Blueprint(blueprint) => materialize(heap, blueprint, region_id),
        ChildCode::Header(child) => arena::alloc_in_region(
            heap,
            HeapObject::ClosureTemplate(child.without_blueprint()),
            region_id,
        ),
    };
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
    code: &crate::value::Code,
    static_region: StaticRegion,
) {
    let idx = vm.read_u16(bytecode, ip) as usize;
    let num_upvalues = vm.read_u16(bytecode, ip) as usize;

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
    let region_id =
        vm.runtime_region_for_alloc_slot_maybe_merged(static_region, code.merged_slots());

    // `materialize_closure_in_region` allocates through the VM's heap
    // (`vm.heap_ptr`/`vm.heap()`), shared by interpreter and JIT.
    let val = materialize_closure_in_region(
        unsafe { &mut *vm.heap_ptr },
        code.child(idx),
        &captured,
        region_id,
    );
    vm.fiber.stack.push(val);
}
