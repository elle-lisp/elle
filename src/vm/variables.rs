use super::core::VM;
use crate::value::{error_val, Value, SIG_ERROR};

pub fn handle_load_global(vm: &mut VM, bytecode: &[u8], ip: &mut usize, constants: &[Value]) {
    let idx = vm.read_u16(bytecode, ip) as usize;
    if let Some(sym_id) = constants[idx].as_symbol() {
        // First, check if variable exists in current scope (scope-aware lookup)
        if let Some(val) = vm.scope_stack.get(sym_id) {
            // Don't automatically unwrap cells - closures need to capture the cell
            // for shared mutable captures. Unwrapping happens at use sites.
            vm.fiber.stack.push(val);
            return;
        }

        // Fall back to global scope
        if let Some(val) = vm
            .globals
            .get(sym_id as usize)
            .filter(|v| !v.is_undefined())
        {
            // Don't automatically unwrap cells in global scope
            // Cells created by the box primitive should remain as cells
            vm.fiber.stack.push(*val);
        } else {
            // Signal undefined-variable exception
            let msg = format!("undefined variable: symbol #{}", sym_id);
            vm.fiber.signal = Some((SIG_ERROR, error_val("undefined-variable", msg)));
            vm.fiber.stack.push(Value::NIL); // Push placeholder
        }
    } else {
        panic!("VM bug: LoadGlobal expects symbol constant");
    }
}

pub fn handle_store_global(vm: &mut VM, bytecode: &[u8], ip: &mut usize, constants: &[Value]) {
    let idx = vm.read_u16(bytecode, ip) as usize;
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on StoreGlobal");
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
        } else if vm
            .globals
            .get(sym_id as usize)
            .is_some_and(|v| !v.is_undefined())
        {
            // Exists in global scope — update there
            let idx = sym_id as usize;
            if idx >= vm.globals.len() {
                vm.globals.resize(idx + 1, Value::UNDEFINED);
            }
            vm.globals[idx] = val;
        } else if vm.scope_stack.depth() > 1 {
            // New variable in a local scope — define locally
            vm.scope_stack.define_local(sym_id, val);
        } else {
            // New variable at global scope
            let idx = sym_id as usize;
            if idx >= vm.globals.len() {
                vm.globals.resize(idx + 1, Value::UNDEFINED);
            }
            vm.globals[idx] = val;
        }
        vm.fiber.stack.push(val);
    } else {
        panic!("VM bug: StoreGlobal expects symbol constant");
    }
}

pub fn handle_store_local(vm: &mut VM, bytecode: &[u8], ip: &mut usize) {
    let _depth = vm.read_u8(bytecode, ip);
    let idx = vm.read_u8(bytecode, ip) as usize;
    let value = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on StoreLocal");
    let frame_base = vm.current_frame_base();
    let abs_idx = frame_base + idx;
    if abs_idx >= vm.fiber.stack.len() {
        // Need to extend stack if storing to a new local
        while vm.fiber.stack.len() <= abs_idx {
            vm.fiber.stack.push(Value::NIL);
        }
    }
    vm.fiber.stack[abs_idx] = value;
    // Push the value back so it can be used as the result of set!
    vm.fiber.stack.push(value);
}

pub fn handle_load_upvalue(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    closure_env: Option<&std::rc::Rc<Vec<Value>>>,
) {
    let _depth = vm.read_u8(bytecode, ip);
    let idx = vm.read_u8(bytecode, ip) as usize;

    // Load from closure environment
    let env = closure_env.expect("VM bug: LoadUpvalue used outside of closure");
    if idx >= env.len() {
        panic!(
            "VM bug: Upvalue index {} out of bounds (env size: {})",
            idx,
            env.len()
        );
    }
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
            vm.fiber.stack.push(inner);
        }
    } else {
        // Everything else (including symbols and user Cell) pushed as-is
        // Symbols in the environment are literal symbol values, not variable references
        vm.fiber.stack.push(val);
    }
}

pub fn handle_load_upvalue_raw(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    closure_env: Option<&std::rc::Rc<Vec<Value>>>,
) {
    let _depth = vm.read_u8(bytecode, ip);
    let idx = vm.read_u8(bytecode, ip) as usize;

    // Load from closure environment WITHOUT unwrapping cells
    // This is used when forwarding captures to nested closures
    let env = closure_env.expect("VM bug: LoadUpvalueRaw used outside of closure");
    if idx >= env.len() {
        panic!(
            "VM bug: Upvalue index {} out of bounds (env size: {})",
            idx,
            env.len()
        );
    }
    vm.fiber.stack.push(env[idx]);
}

pub fn handle_store_upvalue(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    closure_env: Option<&std::rc::Rc<Vec<Value>>>,
) {
    let _depth = vm.read_u8(bytecode, ip);
    let idx = vm.read_u8(bytecode, ip) as usize;
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on StoreUpvalue");

    // Store to closure environment
    let env = closure_env.expect("VM bug: StoreUpvalue used outside of closure");
    if idx >= env.len() {
        panic!(
            "VM bug: Upvalue index {} out of bounds (env size: {})",
            idx,
            env.len()
        );
    }
    // Handle cell-based storage for shared mutable captures
    // If the closure environment contains a cell at this index, update the cell
    let env_val = env[idx];
    if env_val.is_cell() {
        // Update the cell's contents
        if let Some(cell_ref) = env_val.as_cell() {
            let mut cell_mut = cell_ref.borrow_mut();
            *cell_mut = val;
        }
        vm.fiber.stack.push(val);
    } else if let Some(sym) = env_val.as_symbol() {
        // This is a global variable reference - update it in the global scope
        let idx = sym as usize;
        if idx >= vm.globals.len() {
            vm.globals.resize(idx + 1, Value::UNDEFINED);
        }
        vm.globals[idx] = val;
        vm.fiber.stack.push(val);
    } else {
        // If it's not a cell or symbol, we cannot mutate it
        panic!("VM bug: Cannot mutate non-cell closure environment variables");
    }
}
