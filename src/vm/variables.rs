use super::core::VM;
use crate::value::Value;

pub(crate) fn handle_store_local(vm: &mut VM, bytecode: &[u8], ip: &mut usize) {
    let idx = vm.read_u16(bytecode, ip) as usize;
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

pub(crate) fn handle_load_upvalue(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    closure_env: Option<&std::rc::Rc<Vec<Value>>>,
) {
    let _depth = vm.read_u8(bytecode, ip);
    let idx = vm.read_u16(bytecode, ip) as usize;

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

    if val.is_capture_cell() {
        // Auto-unwrap compiler-created capture cells
        if let Some(cell_ref) = val.as_capture_cell() {
            let inner = *cell_ref.borrow();
            vm.fiber.stack.push(inner);
        }
    } else {
        // Everything else (including symbols and user Cell) pushed as-is
        // Symbols in the environment are literal symbol values, not variable references
        vm.fiber.stack.push(val);
    }
}

pub(crate) fn handle_load_upvalue_raw(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    closure_env: Option<&std::rc::Rc<Vec<Value>>>,
) {
    let _depth = vm.read_u8(bytecode, ip);
    let idx = vm.read_u16(bytecode, ip) as usize;

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

pub(crate) fn handle_store_upvalue(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    closure_env: Option<&std::rc::Rc<Vec<Value>>>,
) {
    let _depth = vm.read_u8(bytecode, ip);
    let idx = vm.read_u16(bytecode, ip) as usize;
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
    // Handle cell-based storage for shared mutable captures.
    // Upvalues are always cells (LocalCell for mutable captures).
    let env_val = env[idx];
    if let Some(cell_ref) = env_val.as_capture_cell() {
        let mut cell_mut = cell_ref.borrow_mut();
        *cell_mut = val;
        vm.fiber.stack.push(val);
    } else {
        panic!(
            "VM bug: Cannot mutate non-capture closure environment variables (idx={}, env_len={}, val_type={}, env_val_type={})",
            idx, env.len(), val.type_name(), env_val.type_name(),
        );
    }
}
