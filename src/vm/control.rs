// audited: 2026-10-22
//! The control-flow opcodes: the jumps and the return.
//!
//! docs/impl/vm.md

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

    // A return unwraps a capture cell, which user code never sees. A user
    // box is a value and returns as it is.
    value.capture_cell_get().unwrap_or(value)
}
