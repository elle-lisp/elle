use super::core::VM;
use crate::value::{Condition, Value};
use std::rc::Rc;

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
            // Don't automatically unwrap cells in local scope
            // Cells created by the box primitive should remain as cells
            vm.stack.push(val);
            return Ok(());
        }

        // Fall back to global scope
        if let Some(val) = vm.globals.get(&sym_id.0) {
            // Don't automatically unwrap cells in global scope
            // Cells created by the box primitive should remain as cells
            vm.stack.push(val.clone());
        } else {
            // Signal undefined-variable exception (ID 5)
            let mut cond = Condition::new(5);
            cond.set_field(0, Value::Symbol(sym_id)); // Store the symbol
            if let Some(loc) = vm.current_source_loc.clone() {
                cond.location = Some(loc);
            }
            vm.current_exception = Some(Rc::new(cond));
            vm.stack.push(Value::Nil); // Push placeholder
            return Ok(());
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
            if let Value::Cell(cell_rc) | Value::LocalCell(cell_rc) = existing {
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
            // Handle different value types:
            // - LocalCell: auto-unwrap (internal cells for locally-defined variables)
            // - Cell: do NOT unwrap (user-created via `box` primitive)
            // - Symbol: load from global scope
            // - Other: push as-is
            match val {
                Value::LocalCell(cell_rc) => {
                    // Auto-unwrap internal cells for locally-defined variables
                    let inner = cell_rc.borrow().clone();
                    vm.stack.push(*inner);
                }
                _ => {
                    // Everything else (including symbols and user Cell) pushed as-is
                    // Symbols in the environment are literal symbol values, not variable references
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
                Value::LocalCell(cell_rc) | Value::Cell(cell_rc) => {
                    // Update the cell's contents (both LocalCell and Cell)
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
