//! The fresh-frame invariant of a frame-replacing tail call: a callee entered
//! by `TailCall` sees every local slot it has not written as NIL, exactly as a
//! fresh activation does.
//!
//! The compiler leans on that invariant: a branch-arm-bound ANF temp is
//! NIL-initialized only inside its own arm, yet its value-based release
//! (`LoadLocal slot; DecrefValueRegion`) runs unconditionally at the binding
//! scope's end — sound only if the untaken arm's slot reads NIL (the release
//! no-ops). A tail call reuses the caller's operand stack, and the caller's
//! locals at those indices are arbitrary live or already-released values: a
//! stale read there turns the scope-end release into an over-free of a region
//! the frame owns no reference to.

use super::*;

/// The direct pin: a caller plants a marker in local slot 3 and tail-calls a
/// callee that reads slot 3 without ever writing it. The callee's prologue
/// pushes one bare NIL per local — on a fresh (empty) stack those land at the
/// slot indices, so the read yields NIL; the tail-call entry must be
/// indistinguishable. Reading the caller's marker instead is the stale-frame
/// leak-through this test exists to catch.
#[test]
fn tail_call_frame_delivers_nil_locals() {
    use crate::compiler::bytecode::{Bytecode, Instruction};

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;

    // Callee: 4 locals' worth of prologue nils, then read the unwritten slot 3.
    let mut callee_bc = Bytecode::new();
    for _ in 0..4 {
        callee_bc.emit(Instruction::Nil);
    }
    callee_bc.emit(Instruction::LoadLocal);
    callee_bc.emit_u16(3);
    callee_bc.emit(Instruction::Return);
    // Materialize the callee as a heap closure value so the caller can name it
    // as a constant, exactly as compiled code reaches a callee through a slot.
    let heap = unsafe { &mut *heap_ptr };
    let callee_region = heap.new_runtime_region();
    let callee_value = crate::vm::closure::materialize_closure_in_region(
        heap,
        &crate::value::ClosureTemplate::new(
            std::rc::Rc::new(callee_bc.instructions),
            crate::value::Arity::Exact(0),
            std::rc::Rc::new(callee_bc.constants),
        ),
        &[],
        callee_region,
    );

    // Caller: plant the marker in slot 3, then a zero-arg tail call.
    let mut bc = Bytecode::new();
    let marker_idx = bc.add_constant(crate::value::Value::int(42));
    let callee_idx = bc.add_constant(callee_value);
    bc.emit(Instruction::LoadConst);
    bc.emit_u16(marker_idx);
    bc.emit(Instruction::StoreLocal);
    bc.emit_u16(3);
    bc.emit(Instruction::LoadConst);
    bc.emit_u16(callee_idx);
    bc.emit(Instruction::TailCall);
    bc.emit_u16(0); // arg count
    bc.emit_u32(2); // static result-region slot (unused by a closure callee, must be nonzero)
    bc.emit_byte(0); // defer_callee_release: off — the deferral is not under test
    bc.emit_u32(0); // deferred_release_slot: none
    bc.emit_byte(0); // borrowed_arg_slots: none — a zero-arg call borrows nothing
    bc.emit(Instruction::Return);

    let (handle, fiber_value) = child_fiber(unsafe { &mut *heap_ptr }, fiber_body_closure(bc));
    let (bits, result) = vm.do_fiber_resume(&handle, fiber_value);
    assert!(bits.is_empty(), "the tail-called body completes normally");
    assert!(
        result.is_nil(),
        "a tail-called frame's unwritten local slot must read NIL, exactly as \
         a fresh activation's — got the caller's stale slot value instead \
         (tag={:#x})",
        result.tag,
    );
}
