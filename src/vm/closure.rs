use super::core::VM;
use crate::hir::region::RegionId;
use crate::value::fiber::SignalBits;
use crate::value::heap::{Closure, HeapObject};
use crate::value::Value;

pub(crate) fn handle_make_closure(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    constants: &[Value],
    region_id: RegionId,
) {
    let idx = vm.read_u16(bytecode, ip) as usize;
    let num_upvalues = vm.read_u16(bytecode, ip) as usize;

    // Get the closure template from constants
    let template_closure = constants[idx]
        .as_closure()
        .expect("VM bug: MakeClosure expects closure constant");

    // Collect captured values from stack
    let mut captured = Vec::with_capacity(num_upvalues);
    for _ in 0..num_upvalues {
        captured.push(
            vm.fiber
                .stack
                .pop()
                .expect("VM bug: Stack underflow on MakeClosure"),
        );
    }
    captured.reverse();

    // Create closure with shared template and captured environment.
    // `env` is allocated inline in the same region as the closure.
    let env = vm
        .heap()
        .alloc_inline_slice_in_region::<Value>(&captured, region_id);
    let closure = Closure {
        template: template_closure.template.clone(),
        env,
        squelch_mask: SignalBits::EMPTY,
    };

    let obj = HeapObject::Closure {
        closure,
        traits: Value::NIL,
    };
    // alloc_in_region → alloc_obj → incref_cross_region_refs handles
    // cross-region incref for closure env contents automatically.
    let val = vm.heap().alloc_in_region(obj, region_id);
    vm.fiber.stack.push(val);
}
