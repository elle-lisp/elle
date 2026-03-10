use crate::value::Value;
use crate::vm::core::VM;

/// Handle MakeLBox instruction - wraps a value in a cell for shared mutable access
/// Pops value from stack, wraps it in a cell, pushes the cell
/// Idempotent: if the value is already a cell, it is not double-wrapped
///
/// Uses LocalCell (not Cell) because MakeLBox is emitted by the compiler for
/// mutable captured variables, which should auto-unwrap on LoadUpvalue.
/// User-created cells via `box` use a different code path.
pub(crate) fn handle_make_lbox(vm: &mut VM) {
    let value = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on MakeLBox");
    if value.is_lbox() {
        // Already a cell (e.g., locally-defined variable from outer lambda) — don't double-wrap
        vm.fiber.stack.push(value);
    } else {
        // Create a local cell for compiler-generated cells (mutable captures)
        // LocalCell is auto-unwrapped by LoadUpvalue
        let cell = Value::local_lbox(value);
        vm.fiber.stack.push(cell);
    }
}

/// Handle UnlBox instruction - extracts value from a cell
/// Pops cell from stack, unwraps it, pushes the value
pub(crate) fn handle_unlbox(vm: &mut VM) {
    let cell_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on UnlBox");
    if cell_val.is_lbox() {
        let cell_ref = cell_val
            .as_lbox()
            .expect("VM bug: Failed to extract cell reference");
        let value = *cell_ref.borrow();
        vm.fiber.stack.push(value);
    } else {
        panic!("VM bug: Expected cell, got {}", cell_val.type_name());
    }
}

/// Handle UpdateLBox instruction - updates a cell's contents
/// Pops new_value, then cell from stack, updates cell, pushes new_value
pub(crate) fn handle_update_lbox(vm: &mut VM) {
    let new_value = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on UpdateLBox");
    let cell_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on UpdateLBox");
    if cell_val.is_lbox() {
        let cell_ref = cell_val
            .as_lbox()
            .expect("VM bug: Failed to extract cell reference");
        let mut cell_mut = cell_ref.borrow_mut();
        *cell_mut = new_value;
        vm.fiber.stack.push(new_value);
    } else {
        panic!("VM bug: Expected cell, got {}", cell_val.type_name());
    }
}
