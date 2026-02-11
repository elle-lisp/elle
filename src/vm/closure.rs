use super::core::VM;
use crate::value::{Closure, Value};
use std::rc::Rc;

pub fn handle_make_closure(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    constants: &[Value],
) -> Result<(), String> {
    let idx = vm.read_u16(bytecode, ip) as usize;
    let num_upvalues = vm.read_u8(bytecode, ip) as usize;

    // Get the closure template from constants
    if let Value::Closure(template) = &constants[idx] {
        // Collect captured values from stack
        let mut captured = Vec::with_capacity(num_upvalues);
        for _ in 0..num_upvalues {
            captured.push(vm.stack.pop().ok_or("Stack underflow")?);
        }
        captured.reverse();

        // Create closure with captured values in environment
        let closure = Closure {
            bytecode: template.bytecode.clone(),
            arity: template.arity,
            env: Rc::new(captured),
            num_locals: template.num_locals,
            num_captures: template.num_captures,
            constants: template.constants.clone(),
            source_ast: template.source_ast.clone(),
        };

        vm.stack.push(Value::Closure(Rc::new(closure)));
    } else {
        return Err("MakeClosure expects closure constant".to_string());
    }
    Ok(())
}
