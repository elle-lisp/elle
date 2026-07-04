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
    vm.fiber.stack.push(value);
}

/// Push the currently-executing closure — the value path for a self-reference.
/// `current_closure` is a per-activation register naming the closure whose body
/// is running (restored across every call/tail-call/suspend boundary), so a
/// value-position `loop`/`go` resolves to the closure itself with no capture
/// slot. It is a borrow (no incref): the executing closure is kept alive by the
/// activation running it, and any escape of the pushed value is counted by the
/// ordinary return/store RC path — RC-identical to naming the closure through a
/// binding slot, without any per-call cell.
pub(crate) fn handle_load_self(vm: &mut VM) {
    // LoadSelf is emitted only inside a closure body, and every entrant that
    // runs a closure body hands the callee through the one-shot entry register
    // (docs/impl/vm.md § The executing-closure register) — so an untracked
    // (NIL) register here means an entry path dropped the handoff, and the
    // self-reference would silently resolve to nil. Fail loudly at the read.
    debug_assert!(
        !vm.fiber.current_closure.is_nil(),
        "LoadSelf on an untracked activation: an entry path ran a closure body \
         without handing the callee through `pending_entry_closure` \
         (docs/impl/vm.md § The executing-closure register)"
    );
    vm.fiber.stack.push(vm.fiber.current_closure);
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
    if env_val.is_capture_cell() {
        // The funnel tracks cross-region refs relative to the cell's region.
        crate::value::arena::capture_store_with_rebind(unsafe { &mut *vm.heap_ptr }, env_val, val);
        vm.fiber.stack.push(val);
    } else {
        panic!(
            "VM bug: Cannot mutate non-capture closure environment variables (idx={}, env_len={}, val_type={}, env_val_type={})",
            idx, env.len(), val.type_name(), env_val.type_name(),
        );
    }
}
