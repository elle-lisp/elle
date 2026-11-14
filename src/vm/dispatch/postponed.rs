// audited: 2026-11-14
//! The interpreter's `DecrefPostponed`: the release an operand arm postponed to its tail call.
//!
//! docs/impl/region/operand.md
//! docs/impl/region/unwind.md

use crate::value::Value;
use crate::vm::core::VM;

/// Run `InstrRef::DecrefPostponed`, whose doc says what it does. The release
/// is `handle_decref_value_region`, which pops the value pushed here.
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
