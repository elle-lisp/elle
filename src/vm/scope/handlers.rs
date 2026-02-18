use super::ScopeType;
use crate::value::Value;
use crate::vm::core::VM;

/// Handle PushScope instruction
pub fn handle_push_scope(vm: &mut VM, scope_type_byte: u8) -> Result<(), String> {
    // Convert byte to ScopeType
    let scope_type = match scope_type_byte {
        0 => ScopeType::Global,
        1 => ScopeType::Function,
        2 => ScopeType::Block,
        3 => ScopeType::Loop,
        4 => ScopeType::Let,
        _ => return Err(format!("Invalid scope type: {}", scope_type_byte)),
    };

    vm.scope_stack.push(scope_type);
    Ok(())
}

/// Handle PopScope instruction
pub fn handle_pop_scope(vm: &mut VM) -> Result<(), String> {
    if !vm.scope_stack.pop() {
        return Err("Cannot pop global scope".to_string());
    }
    Ok(())
}

/// Handle DefineLocal instruction
pub fn handle_define_local(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    constants: &[Value],
) -> Result<(), String> {
    // Read symbol index from bytecode
    let high = bytecode[*ip] as u16;
    let low = bytecode[*ip + 1] as u16;
    *ip += 2;
    let sym_idx = (high << 8) | low;

    // Pop value from stack
    let value = vm.stack.pop().ok_or("Stack underflow")?;

    // Get the symbol ID from constants
    let sym_id = if let Some(id) = constants[sym_idx as usize].as_symbol() {
        id
    } else {
        return Err("Expected symbol in constants".to_string());
    };

    // Define in current scope
    // Note: ScopeStack always has at least the global scope, so we don't need to check
    vm.scope_stack.define_local(sym_id, value);

    // Push the value back on the stack to maintain expression semantics
    // This way (define x 10) returns 10, allowing it to be used in expression contexts
    vm.stack.push(value);

    Ok(())
}

/// Handle MakeCell instruction - wraps a value in a cell for shared mutable access
/// Pops value from stack, wraps it in a cell, pushes the cell
/// Idempotent: if the value is already a cell, it is not double-wrapped
///
/// Uses LocalCell (not Cell) because MakeCell is emitted by the compiler for
/// mutable captured variables, which should auto-unwrap on LoadUpvalue.
/// User-created cells via `box` use a different code path.
pub fn handle_make_cell(vm: &mut VM) -> Result<(), String> {
    let value = vm.stack.pop().ok_or("Stack underflow")?;
    if value.is_cell() {
        // Already a cell (e.g., locally-defined variable from outer lambda) — don't double-wrap
        vm.stack.push(value);
    } else {
        // Create a local cell for compiler-generated cells (mutable captures)
        // LocalCell is auto-unwrapped by LoadUpvalue
        let cell = Value::local_cell(value);
        vm.stack.push(cell);
    }
    Ok(())
}

/// Handle UnwrapCell instruction - extracts value from a cell
/// Pops cell from stack, unwraps it, pushes the value
pub fn handle_unwrap_cell(vm: &mut VM) -> Result<(), String> {
    let cell_val = vm.stack.pop().ok_or("Stack underflow")?;
    if cell_val.is_cell() {
        if let Some(cell_ref) = cell_val.as_cell() {
            let value = *cell_ref.borrow();
            vm.stack.push(value);
            Ok(())
        } else {
            Err("Failed to extract cell reference".to_string())
        }
    } else {
        Err(format!("Expected cell, got {}", cell_val.type_name()))
    }
}

/// Handle UpdateCell instruction - updates a cell's contents
/// Pops new_value, then cell from stack, updates cell, pushes new_value
pub fn handle_update_cell(vm: &mut VM) -> Result<(), String> {
    let new_value = vm.stack.pop().ok_or("Stack underflow")?;
    let cell_val = vm.stack.pop().ok_or("Stack underflow")?;
    if cell_val.is_cell() {
        if let Some(cell_ref) = cell_val.as_cell() {
            let mut cell_mut = cell_ref.borrow_mut();
            *cell_mut = new_value;
            vm.stack.push(new_value);
            Ok(())
        } else {
            Err("Failed to extract cell reference".to_string())
        }
    } else {
        Err(format!("Expected cell, got {}", cell_val.type_name()))
    }
}
