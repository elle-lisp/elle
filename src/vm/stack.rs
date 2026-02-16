use super::core::VM;
use crate::value::Value;

pub fn handle_load_const(vm: &mut VM, bytecode: &[u8], ip: &mut usize, constants: &[Value]) {
    let idx = vm.read_u16(bytecode, ip) as usize;
    vm.stack.push(constants[idx].clone());
}

pub fn handle_load_local(vm: &mut VM, bytecode: &[u8], ip: &mut usize) -> Result<(), String> {
    let _depth = vm.read_u8(bytecode, ip); // depth (currently unused)
    let idx = vm.read_u8(bytecode, ip) as usize;
    let frame_base = vm.current_frame_base();
    let abs_idx = frame_base + idx;
    if abs_idx >= vm.stack.len() {
        return Err(format!(
            "Local variable index out of bounds: {} (frame_base={}, idx={}, stack_len={})",
            abs_idx,
            frame_base,
            idx,
            vm.stack.len()
        ));
    }
    let val = vm.stack[abs_idx].clone();
    vm.stack.push(val);
    Ok(())
}

pub fn handle_pop(vm: &mut VM) -> Result<(), String> {
    vm.stack.pop().ok_or("Stack underflow")?;
    Ok(())
}

pub fn handle_dup(vm: &mut VM) -> Result<(), String> {
    let val = vm.stack.last().ok_or("Stack underflow")?.clone();
    vm.stack.push(val);
    Ok(())
}

pub fn handle_dup_n(vm: &mut VM, bytecode: &[u8], ip: &mut usize) -> Result<(), String> {
    let offset = vm.read_u8(bytecode, ip) as usize;
    let stack_len = vm.stack.len();
    if offset >= stack_len {
        return Err(format!(
            "DupN offset {} out of bounds (stack size {})",
            offset, stack_len
        ));
    }
    let idx = stack_len - 1 - offset;
    let val = vm.stack[idx].clone();
    vm.stack.push(val);
    Ok(())
}
