pub mod arithmetic;
pub mod closure;
pub mod comparison;
pub mod control;
pub mod core;
pub mod data;
pub mod literals;
pub mod stack;
pub mod types;
pub mod variables;

pub use core::{CallFrame, VM};

use crate::compiler::bytecode::{Bytecode, Instruction};
use crate::value::Value;
use std::rc::Rc;

impl VM {
    pub fn execute(&mut self, bytecode: &Bytecode) -> Result<Value, String> {
        self.execute_bytecode(&bytecode.instructions, &bytecode.constants, None)
    }

    fn execute_bytecode(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&Rc<Vec<Value>>>,
    ) -> Result<Value, String> {
        let mut ip = 0;

        loop {
            if ip >= bytecode.len() {
                return Err("Unexpected end of bytecode".to_string());
            }

            let instr_byte = bytecode[ip];
            ip += 1;

            let instr: Instruction = unsafe { std::mem::transmute(instr_byte) };

            match instr {
                // Stack operations
                Instruction::LoadConst => {
                    stack::handle_load_const(self, bytecode, &mut ip, constants);
                }

                Instruction::LoadLocal => {
                    stack::handle_load_local(self, bytecode, &mut ip)?;
                }

                Instruction::Pop => {
                    stack::handle_pop(self)?;
                }

                Instruction::Dup => {
                    stack::handle_dup(self)?;
                }

                // Variable access
                Instruction::LoadGlobal => {
                    variables::handle_load_global(self, bytecode, &mut ip, constants)?;
                }

                Instruction::StoreGlobal => {
                    variables::handle_store_global(self, bytecode, &mut ip, constants)?;
                }

                Instruction::StoreLocal => {
                    variables::handle_store_local(self, bytecode, &mut ip)?;
                }

                Instruction::LoadUpvalue => {
                    variables::handle_load_upvalue(self, bytecode, &mut ip, closure_env)?;
                }

                // Control flow
                Instruction::Jump => {
                    control::handle_jump(bytecode, &mut ip, self);
                }

                Instruction::JumpIfFalse => {
                    control::handle_jump_if_false(bytecode, &mut ip, self)?;
                }

                Instruction::Return => {
                    return control::handle_return(self);
                }

                // Call instructions (complex, handled inline)
                Instruction::Call => {
                    let arg_count = self.read_u8(bytecode, &mut ip) as usize;
                    let func = self.stack.pop().ok_or("Stack underflow")?;

                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        args.push(self.stack.pop().ok_or("Stack underflow")?);
                    }
                    args.reverse();

                    let result = match func {
                        Value::NativeFn(f) => f(&args)?,
                        Value::Closure(closure) => {
                            self.call_depth += 1;
                            if self.call_depth > 1000 {
                                return Err("Stack overflow".to_string());
                            }

                            // Validate argument count
                            match closure.arity {
                                crate::value::Arity::Exact(n) => {
                                    if args.len() != n {
                                        return Err(format!(
                                            "Function expects {} arguments, got {}",
                                            n,
                                            args.len()
                                        ));
                                    }
                                }
                                crate::value::Arity::AtLeast(n) => {
                                    if args.len() < n {
                                        return Err(format!(
                                            "Function expects at least {} arguments, got {}",
                                            n,
                                            args.len()
                                        ));
                                    }
                                }
                                crate::value::Arity::Range(min, max) => {
                                    if args.len() < min || args.len() > max {
                                        return Err(format!(
                                            "Function expects {}-{} arguments, got {}",
                                            min,
                                            max,
                                            args.len()
                                        ));
                                    }
                                }
                            }

                            // Create a new environment that includes both captured variables and parameters
                            // The closure's env contains captured variables, and we append the arguments as parameters
                            let mut new_env = Vec::new();
                            new_env.extend((*closure.env).iter().cloned());
                            new_env.extend(args.clone());
                            let new_env_rc = std::rc::Rc::new(new_env);

                            let result = self.execute_bytecode(
                                &closure.bytecode,
                                &closure.constants,
                                Some(&new_env_rc),
                            )?;

                            self.call_depth -= 1;
                            result
                        }
                        _ => return Err(format!("Cannot call {:?}", func)),
                    };

                    self.stack.push(result);
                }

                Instruction::TailCall => {
                    let arg_count = self.read_u8(bytecode, &mut ip) as usize;
                    let func = self.stack.pop().ok_or("Stack underflow")?;

                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        args.push(self.stack.pop().ok_or("Stack underflow")?);
                    }
                    args.reverse();

                    match func {
                        Value::NativeFn(f) => {
                            return f(&args);
                        }
                        Value::Closure(closure) => {
                            return self.execute_bytecode(
                                &closure.bytecode,
                                constants,
                                Some(&closure.env),
                            );
                        }
                        _ => return Err(format!("Cannot call {:?}", func)),
                    };
                }

                // Closures
                Instruction::MakeClosure => {
                    closure::handle_make_closure(self, bytecode, &mut ip, constants)?;
                }

                // Data structures
                Instruction::Cons => {
                    data::handle_cons(self)?;
                }

                Instruction::Car => {
                    data::handle_car(self)?;
                }

                Instruction::Cdr => {
                    data::handle_cdr(self)?;
                }

                Instruction::MakeVector => {
                    data::handle_make_vector(self, bytecode, &mut ip)?;
                }

                Instruction::VectorRef => {
                    data::handle_vector_ref(self)?;
                }

                Instruction::VectorSet => {
                    data::handle_vector_set(self)?;
                }

                // Arithmetic (integer)
                Instruction::AddInt => {
                    arithmetic::handle_add_int(self)?;
                }

                Instruction::SubInt => {
                    arithmetic::handle_sub_int(self)?;
                }

                Instruction::MulInt => {
                    arithmetic::handle_mul_int(self)?;
                }

                Instruction::DivInt => {
                    arithmetic::handle_div_int(self)?;
                }

                // Arithmetic (polymorphic)
                Instruction::Add => {
                    arithmetic::handle_add(self)?;
                }

                Instruction::Sub => {
                    arithmetic::handle_sub(self)?;
                }

                Instruction::Mul => {
                    arithmetic::handle_mul(self)?;
                }

                Instruction::Div => {
                    arithmetic::handle_div(self)?;
                }

                // Comparisons
                Instruction::Eq => {
                    comparison::handle_eq(self)?;
                }

                Instruction::Lt => {
                    comparison::handle_lt(self)?;
                }

                Instruction::Gt => {
                    comparison::handle_gt(self)?;
                }

                Instruction::Le => {
                    comparison::handle_le(self)?;
                }

                Instruction::Ge => {
                    comparison::handle_ge(self)?;
                }

                // Type checks
                Instruction::IsNil => {
                    types::handle_is_nil(self)?;
                }

                Instruction::IsPair => {
                    types::handle_is_pair(self)?;
                }

                Instruction::IsNumber => {
                    types::handle_is_number(self)?;
                }

                Instruction::IsSymbol => {
                    types::handle_is_symbol(self)?;
                }

                Instruction::Not => {
                    types::handle_not(self)?;
                }

                // Literals
                Instruction::Nil => {
                    literals::handle_nil(self);
                }

                Instruction::True => {
                    literals::handle_true(self);
                }

                Instruction::False => {
                    literals::handle_false(self);
                }
            }
        }
    }
}
