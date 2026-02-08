use crate::compiler::scope::ScopeType;
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

/// Handle LoadScoped instruction
pub fn handle_load_scoped(_vm: &mut VM, bytecode: &[u8], ip: &mut usize) -> Result<(), String> {
    let depth = bytecode[*ip] as usize;
    *ip += 1;
    let index = bytecode[*ip] as usize;
    *ip += 1;

    // This instruction is for future use - currently variables use LoadUpvalue
    // For now, just treat as a no-op to avoid breaking existing code
    let _ = depth;
    let _ = index;
    Ok(())
}

/// Handle StoreScoped instruction
pub fn handle_store_scoped(vm: &mut VM, bytecode: &[u8], ip: &mut usize) -> Result<(), String> {
    let depth = bytecode[*ip] as usize;
    *ip += 1;
    let index = bytecode[*ip] as usize;
    *ip += 1;

    // Pop value from stack
    let value = vm.stack.pop().ok_or("Stack underflow")?;

    // Store to scope at the specified depth
    if !vm.scope_stack.set_at_depth(depth, index as u32, value) {
        return Err(format!(
            "Variable not found at depth {} index {}",
            depth, index
        ));
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
    let sym_id = if let Value::Symbol(id) = constants[sym_idx as usize] {
        id.0
    } else {
        return Err("Expected symbol in constants".to_string());
    };

    // Define in current scope
    vm.scope_stack.define_local(sym_id, value);

    Ok(())
}
