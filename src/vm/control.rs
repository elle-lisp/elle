use super::core::VM;
use crate::value::Value;

pub(crate) fn handle_jump(bytecode: &[u8], ip: &mut usize, vm: &VM) {
    let offset = vm.read_i32(bytecode, ip);
    *ip = ((*ip as i64) + (offset as i64)) as usize;
}

pub(crate) fn handle_jump_if_false(bytecode: &[u8], ip: &mut usize, vm: &mut VM) {
    let offset = vm.read_i32(bytecode, ip);
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on JumpIfFalse");
    if !val.is_truthy() {
        *ip = ((*ip as i64) + (offset as i64)) as usize;
    }
}

pub(crate) fn handle_jump_if_true(bytecode: &[u8], ip: &mut usize, vm: &mut VM) {
    let offset = vm.read_i32(bytecode, ip);
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on JumpIfTrue");
    if val.is_truthy() {
        *ip = ((*ip as i64) + (offset as i64)) as usize;
    }
}

pub(crate) fn handle_return(vm: &mut VM) -> Value {
    let value = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on return");

    // Unwrap CaptureCell (internal cells for mutable captures).
    // User code should never see a CaptureCell — it's an implementation detail.
    // LBox (user-facing box) must NOT be unwrapped here.
    if let Some(cell_ref) = value.as_capture_cell() {
        *cell_ref.borrow()
    } else {
        value
    }
}

// Call and TailCall are complex and need to stay in mod.rs because they call execute_bytecode recursively
// These functions are just placeholders for documentation purposes
pub mod note {
    //! Call, TailCall, and MakeClosure handle complex recursive execution logic
    //! and require access to the full execution context (execute_bytecode method).
    //! These remain in the main execution loop in mod.rs
}
