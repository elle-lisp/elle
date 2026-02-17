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
    if let Some(sym_id) = constants[idx].as_symbol() {
        // First, check if variable exists in current scope (scope-aware lookup)
        if let Some(val) = vm.scope_stack.get(sym_id) {
            // Don't automatically unwrap cells - closures need to capture the cell
            // for shared mutable captures. Unwrapping happens at use sites.
            vm.stack.push(val);
            return Ok(());
        }

        // Fall back to global scope
        if let Some(val) = vm.globals.get(&sym_id) {
            // Don't automatically unwrap cells in global scope
            // Cells created by the box primitive should remain as cells
            vm.stack.push(*val);
        } else {
            // Signal undefined-variable exception (ID 5)
            let msg = format!("undefined variable: symbol #{}", sym_id);
            let mut cond = Condition::undefined_variable(msg).with_field(0, Value::symbol(sym_id)); // Store the symbol
            if let Some(loc) = vm.current_source_loc.clone() {
                cond.location = Some(loc);
            }
            vm.current_exception = Some(Rc::new(cond));
            vm.stack.push(Value::NIL); // Push placeholder
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
    if let Some(sym_id) = constants[idx].as_symbol() {
        // Check scope stack first (for proper shadowing)
        if let Some(existing) = vm.scope_stack.get(sym_id) {
            // Check if the existing value is a cell (for shared mutable captures)
            if existing.is_cell() {
                // Update the cell's contents instead of replacing the cell itself
                if let Some(cell_ref) = existing.as_cell() {
                    let mut cell_mut = cell_ref.borrow_mut();
                    *cell_mut = val;
                }
            } else {
                // Regular variable - update it directly
                if !vm.scope_stack.set(sym_id, val) {
                    // Shouldn't happen if get() succeeded
                    vm.scope_stack.define_local(sym_id, val);
                }
            }
        } else if vm.globals.contains_key(&sym_id) {
            // Exists in global scope — update there
            vm.globals.insert(sym_id, val);
        } else if vm.scope_stack.depth() > 1 {
            // New variable in a local scope — define locally
            vm.scope_stack.define_local(sym_id, val);
        } else {
            // New variable at global scope
            vm.globals.insert(sym_id, val);
        }
        vm.stack.push(val);
    } else {
        return Err("StoreGlobal expects symbol constant".to_string());
    }
    Ok(())
}

pub fn handle_store_local(vm: &mut VM, bytecode: &[u8], ip: &mut usize) -> Result<(), String> {
    let _depth = vm.read_u8(bytecode, ip);
    let idx = vm.read_u8(bytecode, ip) as usize;
    let value = vm.stack.pop().ok_or("Stack underflow on StoreLocal")?;
    let frame_base = vm.current_frame_base();
    let abs_idx = frame_base + idx;
    if abs_idx >= vm.stack.len() {
        // Need to extend stack if storing to a new local
        while vm.stack.len() <= abs_idx {
            vm.stack.push(Value::NIL);
        }
    }
    vm.stack[abs_idx] = value;
    // Push the value back so it can be used as the result of set!
    vm.stack.push(value);
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
            let val = env[idx];
            // Handle different value types:
            // - LocalCell: auto-unwrap (compiler-created cells for mutable captures)
            // - Cell (user box): push as-is (NOT auto-unwrapped)
            // - Symbol: push as-is (literal symbol values)
            // - Other: push as-is

            if val.is_local_cell() {
                // Auto-unwrap compiler-created local cells
                if let Some(cell_ref) = val.as_cell() {
                    let inner = *cell_ref.borrow();
                    vm.stack.push(inner);
                }
            } else {
                // Everything else (including symbols and user Cell) pushed as-is
                // Symbols in the environment are literal symbol values, not variable references
                vm.stack.push(val);
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
            vm.stack.push(env[idx]);
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
            // Handle cell-based storage for shared mutable captures
            // If the closure environment contains a cell at this index, update the cell
            let env_val = env[idx];
            if env_val.is_cell() {
                // Update the cell's contents
                if let Some(cell_ref) = env_val.as_cell() {
                    let mut cell_mut = cell_ref.borrow_mut();
                    *cell_mut = val;
                }
                vm.stack.push(val);
                Ok(())
            } else if let Some(sym) = env_val.as_symbol() {
                // This is a global variable reference - update it in the global scope
                vm.globals.insert(sym, val);
                vm.stack.push(val);
                Ok(())
            } else {
                // If it's not a cell or symbol, we cannot mutate it
                Err("Cannot mutate non-cell closure environment variables".to_string())
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
