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
            // Phase 4: Transparently unwrap cells for shared mutable captures
            match val {
                Value::Cell(cell_rc) => {
                    let cell_ref = cell_rc.borrow();
                    vm.stack.push((**cell_ref).clone());
                }
                _ => {
                    vm.stack.push(val);
                }
            }
            return Ok(());
        }

        // Fall back to global scope
        if let Some(val) = vm.globals.get(&sym_id.0) {
            // Phase 4: Also handle cells in global scope
            match val {
                Value::Cell(cell_rc) => {
                    let cell_ref = cell_rc.borrow();
                    vm.stack.push((**cell_ref).clone());
                }
                _ => {
                    vm.stack.push(val.clone());
                }
            }
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
        // Check scope stack first (for proper shadowing)
        if let Some(existing) = vm.scope_stack.get(sym_id.0) {
            // Phase 4: Check if the existing value is a cell (for shared mutable captures)
            if let Value::Cell(cell_rc) = existing {
                // Update the cell's contents instead of replacing the cell itself
                let mut cell_ref = cell_rc.borrow_mut();
                **cell_ref = val.clone();
            } else {
                // Regular variable - update it directly
                if !vm.scope_stack.set(sym_id.0, val.clone()) {
                    // Shouldn't happen if get() succeeded
                    vm.scope_stack.define_local(sym_id.0, val.clone());
                }
            }
        } else if vm.globals.contains_key(&sym_id.0) {
            // Exists in global scope — update there
            vm.globals.insert(sym_id.0, val.clone());
        } else if vm.scope_stack.depth() > 1 {
            // New variable in a local scope — define locally
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
            let val = env[idx].clone();
            // Phase 4: Transparently unwrap cells for shared mutable captures
            // If the captured value is a cell, unwrap it to get the current value
            match val {
                Value::Cell(cell_rc) => {
                    let cell_ref = cell_rc.borrow();
                    vm.stack.push((**cell_ref).clone());
                }
                Value::Symbol(sym) => {
                    // This is a global variable reference - load it from the global scope
                    if let Some(global_val) = vm.globals.get(&sym.0) {
                        vm.stack.push(global_val.clone());
                    } else {
                        return Err(format!("Undefined global variable: {:?}", sym));
                    }
                }
                _ => {
                    vm.stack.push(val);
                }
            }
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

pub fn handle_load_upvalue_raw(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    closure_env: Option<&std::rc::Rc<Vec<Value>>>,
) -> Result<(), String> {
    let _depth = vm.read_u8(bytecode, ip);
    let idx = vm.read_u8(bytecode, ip) as usize;

    // Load from closure environment WITHOUT unwrapping cells
    // This is used when forwarding captures to nested closures
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
        return Err("LoadUpvalueRaw used outside of closure".to_string());
    }
    Ok(())
}

pub fn handle_store_upvalue(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    closure_env: Option<&std::rc::Rc<Vec<Value>>>,
) -> Result<(), String> {
    let _depth = vm.read_u8(bytecode, ip);
    let idx = vm.read_u8(bytecode, ip) as usize;
    let val = vm.stack.pop().ok_or("Stack underflow")?;

    // Store to closure environment
    if let Some(env) = closure_env {
        if idx < env.len() {
            // Phase 4: Handle cell-based storage for shared mutable captures
            // If the closure environment contains a cell at this index, update the cell
            match &env[idx] {
                Value::Cell(cell_rc) => {
                    // Update the cell's contents
                    let mut cell_ref = cell_rc.borrow_mut();
                    **cell_ref = val.clone();
                    vm.stack.push(val);
                    Ok(())
                }
                Value::Symbol(sym) => {
                    // This is a global variable reference - update it in the global scope
                    vm.globals.insert(sym.0, val.clone());
                    vm.stack.push(val);
                    Ok(())
                }
                _ => {
                    // If it's not a cell or symbol, we cannot mutate it
                    Err("Cannot mutate non-cell closure environment variables".to_string())
                }
            }
        } else {
            Err(format!(
                "Upvalue index {} out of bounds (env size: {})",
                idx,
                env.len()
            ))
        }
    } else {
        Err("StoreUpvalue used outside of closure".to_string())
    }
}
