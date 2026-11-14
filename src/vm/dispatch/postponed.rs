// audited: 2026-11-13
//! The interpreter's `DecrefPostponed`: the release an operand arm postponed to its tail call.
//!
//! docs/impl/region/operand.md
//! docs/impl/region/unwind.md

use crate::value::Value;
use crate::vm::core::VM;

/// Read the postponed slot and the route slot it was copied from, both frame
/// locals. A value in the postponed slot is a release the arm owed: stamp the
/// route `nil`, so the release behind the call and the abandoned-frame walk
/// read the receipt, then release the value as `DecrefValueRegion` does. The
/// postponed slot always ends `nil`.
pub(crate) fn handle_decref_postponed(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    locations: crate::value::closure::LocationTable<'_>,
    instr_ip: usize,
) {
    let slot = vm.read_u16(bytecode, ip) as usize;
    let route = vm.read_u16(bytecode, ip) as usize;
    let base = vm.current_frame_base();
    let held = std::mem::replace(&mut vm.fiber.stack[base + slot], Value::NIL);
    if held.is_nil() {
        return;
    }
    vm.fiber.stack[base + route] = Value::NIL;
    vm.fiber.stack.push(held);
    super::region::handle_decref_value_region(vm, locations, instr_ip);
}
