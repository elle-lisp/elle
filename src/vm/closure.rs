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
    if let Some(template_closure) = constants[idx].as_closure() {
        // Collect captured values from stack
        let mut captured = Vec::with_capacity(num_upvalues);
        for _ in 0..num_upvalues {
            captured.push(vm.stack.pop().ok_or("Stack underflow")?);
        }
        captured.reverse();

        // Create closure with captured values in environment
        let closure = Closure {
            bytecode: template_closure.bytecode.clone(),
            arity: template_closure.arity,
            env: Rc::new(captured),
            num_locals: template_closure.num_locals,
            num_captures: template_closure.num_captures,
            constants: template_closure.constants.clone(),
            source_ast: template_closure.source_ast.clone(),
            effect: template_closure.effect,
            cell_params_mask: template_closure.cell_params_mask,
            symbol_names: template_closure.symbol_names.clone(),
        };

        vm.stack.push(Value::closure(closure));
    } else {
        return Err("MakeClosure expects closure constant".to_string());
    }
    Ok(())
}
