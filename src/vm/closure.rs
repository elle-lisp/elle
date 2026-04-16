use super::core::VM;
use crate::value::arena::alloc_inline_slice;
use crate::value::fiber::SignalBits;
use crate::value::{Closure, Value};

pub(crate) fn handle_make_closure(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    constants: &[Value],
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
    // `env` is arena-allocated inline; its lifetime matches the Closure's.
    let closure = Closure {
        template: template_closure.template.clone(),
        env: alloc_inline_slice::<Value>(&captured),
        squelch_mask: SignalBits::EMPTY,
    };

    vm.fiber.stack.push(Value::closure(closure));
}
