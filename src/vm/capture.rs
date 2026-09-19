// audited: 2026-09-19
//! The capture-cell opcodes: MakeCapture, UnwrapCapture, and UpdateCapture.

use crate::hir::region::RuntimeRegion;
use crate::value::heap::CellOrigin;
use crate::vm::core::VM;

/// Handle `MakeCapture`: pop a value, wrap it in a capture cell, and push
/// the cell. Idempotent — a value that is already a capture cell is pushed
/// back as it is, never wrapped a second time.
///
/// The compiler emits `MakeCapture` for mutable captured variables, so the
/// wrapper is a `CaptureCell` (auto-unwrapped by `LoadUpvalue`), never the
/// user's `LBox` — `box` takes a different path.
///
/// `origin` carries the instruction's binding name and assigned bit — the
/// compiled provenance the image dumper's snap decision reads
/// (docs/impl/image/sealing.md).
pub(crate) fn handle_make_capture(vm: &mut VM, region_id: RuntimeRegion, origin: CellOrigin) {
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
        let val = crate::value::build::capture_cell(vm.heap(), value, origin, region_id);
        vm.fiber.stack.push(val);
    }
}

/// Handle `UnwrapCapture`: pop a capture cell and push the value it holds.
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

/// Handle `UpdateCapture`: pop the new value and then the cell, store the
/// value through the tracked funnel, and push the new value back.
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
