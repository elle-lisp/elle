use super::core::VM;
use crate::value::Value;

pub fn handle_jump(bytecode: &[u8], ip: &mut usize, vm: &VM) {
    let offset = vm.read_i16(bytecode, ip);
    *ip = ((*ip as i32) + (offset as i32)) as usize;
}

pub fn handle_jump_if_false(bytecode: &[u8], ip: &mut usize, vm: &mut VM) -> Result<(), String> {
    let offset = vm.read_i16(bytecode, ip);
    let val = vm.stack.pop().ok_or("Stack underflow")?;
    if !val.is_truthy() {
        *ip = ((*ip as i32) + (offset as i32)) as usize;
    }
    Ok(())
}

pub fn handle_jump_if_true(bytecode: &[u8], ip: &mut usize, vm: &mut VM) -> Result<(), String> {
    let offset = vm.read_i16(bytecode, ip);
    let val = vm.stack.pop().ok_or("Stack underflow")?;
    if val.is_truthy() {
        *ip = ((*ip as i32) + (offset as i32)) as usize;
    }
    Ok(())
}

pub fn handle_return(vm: &mut VM) -> Result<Value, String> {
    let value = vm
        .stack
        .pop()
        .ok_or_else(|| "Stack underflow on return".to_string())?;

    // Unwrap LocalCell (internal cells for mutable captures)
    // User code should never see a LocalCell - it's an implementation detail
    match value {
        Value::LocalCell(cell_rc) => {
            let inner = cell_rc.borrow().clone();
            Ok(*inner)
        }
        _ => Ok(value),
    }
}

// Call and TailCall are complex and need to stay in mod.rs because they call execute_bytecode recursively
// These functions are just placeholders for documentation purposes
pub mod note {
    //! Call, TailCall, and MakeClosure handle complex recursive execution logic
    //! and require access to the full execution context (execute_bytecode method).
    //! These remain in the main execution loop in mod.rs
}
