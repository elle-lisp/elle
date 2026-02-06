use super::core::VM;
use crate::value::Value;

pub fn handle_load_global(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    constants: &[Value],
) -> Result<(), String> {
    let idx = vm.read_u16(bytecode, ip) as usize;
    if let Value::Symbol(sym_id) = constants[idx] {
        // First, check if variable exists in current scope (scope-aware lookup)
        if let Some(val) = vm.scope_stack.get(sym_id.0) {
            vm.stack.push(val);
            return Ok(());
        }

        // Fall back to global scope
        if let Some(val) = vm.globals.get(&sym_id.0) {
            vm.stack.push(val.clone());
        } else {
            return Err(format!("Undefined global variable: {:?}", sym_id));
        }
    } else {
        return Err("LoadGlobal expects symbol constant".to_string());
    }
    Ok(())
}

pub fn handle_store_global(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    constants: &[Value],
) -> Result<(), String> {
    let idx = vm.read_u16(bytecode, ip) as usize;
    let val = vm.stack.pop().ok_or("Stack underflow")?;
    if let Value::Symbol(sym_id) = constants[idx] {
        // Check if variable exists in globals or current scope
        if vm.globals.contains_key(&sym_id.0) {
            // Exists in global scope - update there
            vm.globals.insert(sym_id.0, val.clone());
        } else if vm.scope_stack.get(sym_id.0).is_some() {
            // Exists in scope stack somewhere - try to update it
            // Walk up scope chain to find where it's defined
            let mut found = false;
            for depth in 0..vm.scope_stack.depth() {
                if vm.scope_stack.set_at_depth(depth, sym_id.0, val.clone()) {
                    found = true;
                    break;
                }
            }
            if !found {
                // Shouldn't happen if get() succeeded, but be defensive
                vm.scope_stack.define_local(sym_id.0, val.clone());
            }
        } else if vm.scope_stack.depth() > 1 {
            // New variable in a local scope - define locally
            vm.scope_stack.define_local(sym_id.0, val.clone());
        } else {
            // New variable at global scope
            vm.globals.insert(sym_id.0, val.clone());
        }
        vm.stack.push(val);
    } else {
        return Err("StoreGlobal expects symbol constant".to_string());
    }
    Ok(())
}

pub fn handle_store_local(vm: &mut VM, bytecode: &[u8], ip: &mut usize) -> Result<(), String> {
    let idx = vm.read_u8(bytecode, ip) as usize;
    let val = vm.stack.pop().ok_or("Stack underflow")?;
    if idx >= vm.stack.len() {
        return Err("Local variable index out of bounds".to_string());
    }
    vm.stack[idx] = val;
    Ok(())
}

pub fn handle_load_upvalue(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    closure_env: Option<&std::rc::Rc<Vec<Value>>>,
) -> Result<(), String> {
    let _depth = vm.read_u8(bytecode, ip);
    let idx = vm.read_u8(bytecode, ip) as usize;

    // Load from closure environment
    if let Some(env) = closure_env {
        if idx < env.len() {
            vm.stack.push(env[idx].clone());
        } else {
            return Err(format!(
                "Upvalue index {} out of bounds (env size: {})",
                idx,
                env.len()
            ));
        }
    } else {
        return Err("LoadUpvalue used outside of closure".to_string());
    }
    Ok(())
}
