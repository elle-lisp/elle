use crate::hir::region::RuntimeRegion;
use crate::value::Value;
use crate::vm::core::VM;

/// Handle MakeCapture instruction - wraps a value in a capture cell for shared mutable access
/// Pops value from stack, wraps it in a capture cell, pushes the cell
/// Idempotent: if the value is already a capture cell, it is not double-wrapped
///
/// Creates a CaptureCell (not LBox) because MakeCapture is emitted by the compiler for
/// mutable captured variables, which should auto-unwrap on LoadUpvalue.
/// User-created boxes via `box` use a different code path.
pub(crate) fn handle_make_capture(vm: &mut VM, region_id: RuntimeRegion) {
    use crate::value::heap::HeapObject;
    use std::cell::RefCell;
    use std::rc::Rc;
    let value = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on MakeCapture");
    if value.is_capture_cell() {
        // Already a capture cell (e.g., locally-defined variable from outer lambda) — don't double-wrap
        vm.fiber.stack.push(value);
    } else {
        // alloc_in_region → alloc_obj → incref_cross_region_refs handles
        // the cross-region incref for the initial value automatically.
        let obj = HeapObject::CaptureCell {
            cell: Rc::new(RefCell::new(value)),
            traits: Value::NIL,
        };
        let val = vm.heap().alloc_in_region(obj, region_id);
        vm.fiber.stack.push(val);
    }
}

/// Handle UnwrapCapture instruction - extracts value from a capture cell
/// Pops cell from stack, unwraps it, pushes the value
pub(crate) fn handle_unwrap_capture(vm: &mut VM) {
    let cell_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on UnwrapCapture");
    if let Some(value) = cell_val.capture_cell_get() {
        vm.fiber.stack.push(value);
    } else {
        panic!(
            "VM bug: Expected capture cell, got {}",
            cell_val.type_name()
        );
    }
}

/// Handle UpdateCapture instruction - updates a capture cell's contents
/// Pops new_value, then cell from stack, updates cell, pushes new_value
pub(crate) fn handle_update_capture(vm: &mut VM) {
    let new_value = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on UpdateCapture");
    let cell_val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on UpdateCapture");
    if cell_val.is_capture_cell() {
        // The funnel tracks cross-region refs relative to the cell's region.
        crate::value::arena::capture_store_with_rebind(
            unsafe { &mut *vm.heap_ptr },
            cell_val,
            new_value,
        );
        vm.fiber.stack.push(new_value);
    } else {
        panic!(
            "VM bug: Expected capture cell, got {}",
            cell_val.type_name()
        );
    }
}
