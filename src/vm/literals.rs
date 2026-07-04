use super::core::VM;
use crate::hir::region::RuntimeRegion;
use crate::value::Value;

pub(crate) fn handle_nil(vm: &mut VM) {
    vm.fiber.stack.push(Value::NIL);
}

/// Materialize a heap literal (a string, or quoted compound data) into its
/// solver-assigned per-activation region. Reads the inline `u32` template byte
/// length followed by the recursive `ConstTemplate` from the bytecode stream,
/// then builds a FRESH structure into `region_id` — an ordinary allocation,
/// freed at its `decref_point`. The whole structure
/// shares the one region (an immutable aggregate). The template lives in the
/// reclaimable bytecode; `materialize` copies its data into the region, so the
/// result is independent of the template. Mirrors `elle_jit_materialize_const`.
pub(crate) fn handle_materialize_const(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    region_id: RuntimeRegion,
) {
    // u32 byte-length prefix (consumed by the disassembler to skip the template);
    // `decode` is self-delimiting and advances `ip` past exactly the template.
    let _len = vm.read_u32(bytecode, ip);
    let template = crate::value::ConstTemplate::decode(bytecode, ip);
    // A quoted-symbol leaf re-interns into this instance's table via the driving
    // VM; `vm.symbols()` is the same table compilation used.
    let heap = unsafe { &mut *vm.heap_ptr };
    let val = template.materialize(heap, region_id, vm.symbols());
    vm.fiber.stack.push(val);
}

pub(crate) fn handle_empty_list(vm: &mut VM) {
    vm.fiber.stack.push(Value::EMPTY_LIST);
}

pub(crate) fn handle_true(vm: &mut VM) {
    vm.fiber.stack.push(Value::TRUE);
}

pub(crate) fn handle_false(vm: &mut VM) {
    vm.fiber.stack.push(Value::FALSE);
}
