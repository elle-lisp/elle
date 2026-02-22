use super::core::VM;
use crate::value::Value;

pub fn handle_jump(bytecode: &[u8], ip: &mut usize, vm: &VM) {
    let offset = vm.read_i16(bytecode, ip);
    *ip = ((*ip as i32) + (offset as i32)) as usize;
}

pub fn handle_jump_if_false(bytecode: &[u8], ip: &mut usize, vm: &mut VM) {
    let offset = vm.read_i16(bytecode, ip);
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on JumpIfFalse");
    if !val.is_truthy() {
        *ip = ((*ip as i32) + (offset as i32)) as usize;
    }
}

pub fn handle_jump_if_true(bytecode: &[u8], ip: &mut usize, vm: &mut VM) {
    let offset = vm.read_i16(bytecode, ip);
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on JumpIfTrue");
    if val.is_truthy() {
        *ip = ((*ip as i32) + (offset as i32)) as usize;
    }
}

pub fn handle_return(vm: &mut VM) -> Value {
    let value = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on return");

    // Unwrap Cell (internal cells for mutable captures)
    // User code should never see a Cell - it's an implementation detail
    if let Some(_cell_ptr) = value.as_heap_ptr() {
        if let Some(cell_val) = value.as_cell() {
            let inner = cell_val.borrow();
            *inner
        } else {
            value
        }
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
