// audited: 2026-11-12
//! A `TailCall` callee starts as a fresh activation does: unwritten locals read
//! NIL, and no region slot is mapped.
//!
//! docs/impl/region/rules.md
//! docs/impl/region/merging.md
//!
//! The compiler leans on the first: a branch-arm-bound ANF temp is
//! NIL-initialized only inside its own arm, yet its value-based release
//! (`LoadLocal slot; DecrefValueRegion`) runs unconditionally at the binding
//! scope's end. That is sound only if the untaken arm's slot reads NIL. A tail
//! call reuses the caller's operand stack, and the caller's locals at those
//! indices are arbitrary live or already-released values. A stale read there
//! turns the scope-end release into an over-free of a region the frame owns no
//! reference to.
//!
//! Each pin runs under both drivers that replace a frame by a tail call: a
//! child fiber's body runs through `trampoline_loop`, and a top-level body
//! through the root driver `execute_code`.

use super::*;
use crate::compiler::bytecode::{Bytecode, Instruction};
use crate::value::Value;

/// What runs a pin's caller body, and so replaces its frame at the tail call.
#[derive(Clone, Copy)]
enum Driver {
    /// A child fiber's body, which `trampoline_loop` drives.
    Fiber,
    /// A top-level body, which the root driver `execute_code` drives.
    Root,
}

/// Run `bc` as a body under `driver`, answering the value it returns.
fn run(vm: &mut crate::vm::VM, driver: Driver, bc: Bytecode) -> Value {
    let heap = unsafe { &mut *vm.heap_ptr };
    match driver {
        Driver::Fiber => {
            let (handle, fiber_value) = child_fiber(heap, bc);
            let (bits, result) = vm.do_fiber_resume(&handle, fiber_value);
            assert!(bits.is_empty(), "the tail-called body completes normally");
            result
        }
        Driver::Root => {
            let caller = crate::value::CodeBuilder::from_bytecode(bc).build(heap);
            vm.execute_code(caller.code(), None)
                .expect("the tail-called body completes normally")
        }
    }
}

/// The code `builder` builds, materialized as a heap closure value, so a caller
/// can name it as a constant, exactly as compiled code reaches a callee through
/// a slot.
fn callee_value(vm: &mut crate::vm::VM, builder: crate::value::CodeBuilder) -> Value {
    let heap = unsafe { &mut *vm.heap_ptr };
    let callee_region = heap.new_runtime_region();
    let callee = builder.build(heap);
    crate::vm::closure::materialize_closure_in_region(heap, &callee, &[], callee_region)
}

/// A zero-argument `TailCall` of the closure at constant `callee_idx`, then the
/// `Return` past it.
fn emit_zero_arg_tail_call(bc: &mut Bytecode, callee_idx: u16) {
    bc.emit(Instruction::LoadConst);
    bc.emit_u16(callee_idx);
    bc.emit(Instruction::TailCall);
    bc.emit_u16(0); // arg count
    bc.emit_u32(2); // static result-region slot (unused by a closure callee, must be nonzero)
    bc.emit_byte(0); // defer_callee_release: off — the deferral is not under test
    bc.emit_u32(0); // deferred_release_slot: none
    bc.emit_byte(0); // borrowed_arg_slots: none — a zero-arg call borrows nothing
    bc.emit_byte(0); // late_mints: none
    bc.emit(Instruction::Return);
}

/// The direct pin: a caller plants a marker in local slot 3 and tail-calls a
/// callee that reads slot 3 without ever writing it. The callee's prologue
/// pushes one bare NIL per local — on a fresh (empty) stack those land at the
/// slot indices, so the read yields NIL; the tail-call entry must be
/// indistinguishable. Reading the caller's marker instead is the stale-frame
/// leak-through this pin exists to catch.
fn delivers_nil_locals(driver: Driver) {
    let mut vm = crate::vm::VM::new();

    // Callee: 4 locals' worth of prologue nils, then read the unwritten slot 3.
    let mut callee_bc = Bytecode::new();
    for _ in 0..4 {
        callee_bc.emit(Instruction::Nil);
    }
    callee_bc.emit(Instruction::LoadLocal);
    callee_bc.emit_u16(3);
    callee_bc.emit(Instruction::Return);
    let callee = callee_value(&mut vm, crate::value::CodeBuilder::from_bytecode(callee_bc));

    // Caller: its own 4 locals, the marker in slot 3, then the tail call.
    let mut bc = Bytecode::new();
    let marker_idx = bc.add_constant(Value::int(42));
    let callee_idx = bc.add_constant(callee);
    for _ in 0..4 {
        bc.emit(Instruction::Nil);
    }
    bc.emit(Instruction::LoadConst);
    bc.emit_u16(marker_idx);
    bc.emit(Instruction::StoreLocal);
    bc.emit_u16(3);
    emit_zero_arg_tail_call(&mut bc, callee_idx);

    let result = run(&mut vm, driver, bc);
    assert!(
        result.is_nil(),
        "a tail-called frame's unwritten local slot must read NIL, exactly as \
         a fresh activation's — got the caller's stale slot value instead \
         ({:?})",
        result.as_int(),
    );
}

#[test]
fn tail_call_frame_delivers_nil_locals() {
    delivers_nil_locals(Driver::Fiber);
}

/// The counter-factual: the root driver ran the callee on the stack the root
/// body left, so slot 3 read the caller's marker, 42.
#[test]
fn root_tail_call_frame_delivers_nil_locals() {
    delivers_nil_locals(Driver::Root);
}

/// The region-map pin: a caller maps region slot 5 by building a pair there,
/// then tail-calls a callee whose own pair takes slot 5 as a merged slot. A
/// merged slot reuses a mapped entry, so the callee must find the slot empty
/// and mint its own region, exactly as a fresh activation does.
///
/// The counter-factual is the stale entry: the callee's pair lands in the
/// caller's region beside the caller's pair, and nothing reads wrong until a
/// release of one frees the other.
fn delivers_an_empty_region_map(driver: Driver) {
    use crate::value::heap::HeapTag;

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;

    // Callee: build `(nil . nil)` in merged slot 5 and return it.
    let mut callee_bc = Bytecode::new();
    callee_bc.emit(Instruction::Nil);
    callee_bc.emit(Instruction::Nil);
    callee_bc.emit(Instruction::Pair);
    callee_bc.emit_u32(5);
    callee_bc.emit(Instruction::Return);
    let callee = callee_value(
        &mut vm,
        crate::value::CodeBuilder::from_bytecode(callee_bc).merged_slots(vec![5]),
    );

    // Caller: build a pair in slot 5, which maps the slot, then the tail call.
    // The pair's release would sit past the call, so none is run.
    let mut bc = Bytecode::new();
    let callee_idx = bc.add_constant(callee);
    bc.emit(Instruction::Nil);
    bc.emit(Instruction::Nil);
    bc.emit(Instruction::Pair);
    bc.emit_u32(5);
    bc.emit(Instruction::Pop);
    emit_zero_arg_tail_call(&mut bc, callee_idx);

    let result = run(&mut vm, driver, bc);
    let heap = unsafe { &*heap_ptr };
    let region = crate::value::arena::region_of(heap, result).expect("the result is a heap pair");
    let pairs = heap
        .region_tags(region.get())
        .into_iter()
        .filter(|t| *t == HeapTag::Pair)
        .count();
    assert_eq!(
        pairs, 1,
        "the callee's merged pair must mint its own region, not share the \
         region the caller mapped to the same slot"
    );
}

#[test]
fn tail_call_frame_delivers_an_empty_region_map() {
    delivers_an_empty_region_map(Driver::Fiber);
}

#[test]
fn root_tail_call_frame_delivers_an_empty_region_map() {
    delivers_an_empty_region_map(Driver::Root);
}
