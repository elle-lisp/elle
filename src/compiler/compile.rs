use super::ast::Expr;
use super::bytecode::{Bytecode, Instruction};
use crate::value::{Closure, SymbolId, Value};
use std::collections::HashMap;
use std::rc::Rc;

struct Compiler {
    bytecode: Bytecode,
    #[allow(dead_code)]
    symbols: HashMap<SymbolId, usize>,
}

impl Compiler {
    fn new() -> Self {
        Compiler {
            bytecode: Bytecode::new(),
            symbols: HashMap::new(),
        }
    }

    /// Collect all top-level define statements from an expression
    /// Returns a vector of symbol IDs that are defined at this level
    fn collect_defines(expr: &Expr) -> Vec<SymbolId> {
        match expr {
            Expr::Begin(exprs) => {
                let mut defines = Vec::new();
                for e in exprs {
                    if let Expr::Define { name, .. } = e {
                        defines.push(*name);
                    }
                }
                defines
            }
            Expr::Define { name, .. } => vec![*name],
            _ => Vec::new(),
        }
    }

    fn compile_expr(&mut self, expr: &Expr, tail: bool) {
        match expr {
            Expr::Literal(val) => match val {
                Value::Nil => self.bytecode.emit(Instruction::Nil),
                Value::Bool(true) => self.bytecode.emit(Instruction::True),
                Value::Bool(false) => self.bytecode.emit(Instruction::False),
                _ => {
                    let idx = self.bytecode.add_constant(val.clone());
                    self.bytecode.emit(Instruction::LoadConst);
                    self.bytecode.emit_u16(idx);
                }
            },

            Expr::Var(_sym, depth, index) => {
                // Variables in closure environment - access via LoadUpvalue
                // depth indicates nesting level:
                // 0 = current lambda's scope (parameters + captures)
                // 1 = enclosing lambda's scope
                // We add 1 to depth when using LoadUpvalue since it counts from current closure
                self.bytecode.emit(Instruction::LoadUpvalue);
                self.bytecode.emit_byte((*depth + 1) as u8);
                self.bytecode.emit_byte(*index as u8);
            }

            Expr::GlobalVar(sym) => {
                let idx = self.bytecode.add_constant(Value::Symbol(*sym));
                self.bytecode.emit(Instruction::LoadGlobal);
                self.bytecode.emit_u16(idx);
            }

            Expr::If { cond, then, else_ } => {
                self.compile_expr(cond, false);
                self.bytecode.emit(Instruction::JumpIfFalse);
                let else_jump = self.bytecode.current_pos();
                self.bytecode.emit_u16(0); // Placeholder

                self.compile_expr(then, tail);
                self.bytecode.emit(Instruction::Jump);
                let end_jump = self.bytecode.current_pos();
                self.bytecode.emit_u16(0); // Placeholder

                let else_pos = self.bytecode.current_pos();
                self.bytecode
                    .patch_jump(else_jump, (else_pos - else_jump - 2) as i16);

                self.compile_expr(else_, tail);

                let end_pos = self.bytecode.current_pos();
                self.bytecode
                    .patch_jump(end_jump, (end_pos - end_jump - 2) as i16);
            }

            Expr::Begin(exprs) => {
                // Pre-declare all top-level defines to enable recursive functions
                // This allows a function to reference itself in its own body
                let defines = Self::collect_defines(expr);
                for sym_id in defines {
                    // Load nil and store it in the global
                    self.bytecode.emit(Instruction::Nil);
                    let idx = self.bytecode.add_constant(Value::Symbol(sym_id));
                    self.bytecode.emit(Instruction::StoreGlobal);
                    self.bytecode.emit_u16(idx);
                }

                // Now compile the expressions normally
                for (i, expr) in exprs.iter().enumerate() {
                    let is_last = i == exprs.len() - 1;
                    self.compile_expr(expr, tail && is_last);
                    if !is_last {
                        self.bytecode.emit(Instruction::Pop);
                    }
                }
            }

            Expr::Call {
                func,
                args,
                tail: is_tail,
            } => {
                // Compile arguments
                for arg in args {
                    self.compile_expr(arg, false);
                }

                // Compile function
                self.compile_expr(func, false);

                // Emit call
                if tail && *is_tail {
                    self.bytecode.emit(Instruction::TailCall);
                } else {
                    self.bytecode.emit(Instruction::Call);
                }
                self.bytecode.emit_byte(args.len() as u8);
            }

            Expr::Lambda {
                params,
                body,
                captures,
            } => {
                // Create a new compiler for the lambda body
                let mut lambda_compiler = Compiler::new();

                // Compile the body
                lambda_compiler.compile_expr(body, true);
                lambda_compiler.bytecode.emit(Instruction::Return);

                // Create closure value with environment
                // Note: env is empty here, actual capturing happens at runtime via MakeClosure instruction
                let closure = Closure {
                    bytecode: Rc::new(lambda_compiler.bytecode.instructions),
                    arity: crate::value::Arity::Exact(params.len()),
                    env: Rc::new(Vec::new()), // Will be populated by VM when closure is created
                    num_locals: params.len() + captures.len(),
                    constants: Rc::new(lambda_compiler.bytecode.constants),
                };

                let idx = self.bytecode.add_constant(Value::Closure(Rc::new(closure)));

                // Emit captured values onto the stack (in order)
                // These will be stored in the closure's environment by the VM
                for (sym, _depth, _index) in captures {
                    // Load the captured variable
                    let sym_idx = self.bytecode.add_constant(Value::Symbol(*sym));
                    self.bytecode.emit(Instruction::LoadGlobal);
                    self.bytecode.emit_u16(sym_idx);
                }

                // Create closure with captured values
                self.bytecode.emit(Instruction::MakeClosure);
                self.bytecode.emit_u16(idx);
                self.bytecode.emit_byte(captures.len() as u8);
            }

            Expr::Let {
                bindings: _,
                body: _,
            } => {
                // Let-bindings should have been transformed to lambda calls at the converter stage
                // This should not be reached in normal compilation
                panic!("Unexpected Let expression in compile phase - should have been transformed to lambda call");
            }

            Expr::Set {
                var,
                depth,
                index,
                value,
            } => {
                self.compile_expr(value, false);
                if *index == usize::MAX {
                    // Global variable set
                    let idx = self.bytecode.add_constant(Value::Symbol(*var));
                    self.bytecode.emit(Instruction::StoreGlobal);
                    self.bytecode.emit_u16(idx);
                } else if *depth == 0 {
                    // Local variable set
                    self.bytecode.emit(Instruction::StoreLocal);
                    self.bytecode.emit_byte(*index as u8);
                } else {
                    // Upvalue variable set (not supported yet - treat as error or global)
                    // For now, treat as global to avoid corruption
                    let idx = self.bytecode.add_constant(Value::Symbol(*var));
                    self.bytecode.emit(Instruction::StoreGlobal);
                    self.bytecode.emit_u16(idx);
                }
            }

            Expr::Define { name, value } => {
                self.compile_expr(value, false);
                let idx = self.bytecode.add_constant(Value::Symbol(*name));
                self.bytecode.emit(Instruction::StoreGlobal);
                self.bytecode.emit_u16(idx);
            }

            Expr::While { cond, body } => {
                // Implement while loop using conditional jumps
                // Loop label - start of condition check
                let loop_label = self.bytecode.current_pos() as i32;

                // Compile condition
                self.compile_expr(cond, false);

                // Jump to end if condition is false
                self.bytecode.emit(Instruction::JumpIfFalse);
                let exit_jump = self.bytecode.current_pos() as i32;
                self.bytecode.emit_u16(0); // Placeholder for exit offset

                // Compile body
                self.compile_expr(body, false);

                // Pop the body result (we don't care about it)
                self.bytecode.emit(Instruction::Pop);

                // Jump back to loop condition
                self.bytecode.emit(Instruction::Jump);
                let loop_jump = self.bytecode.current_pos() as i32;
                self.bytecode.emit_u16(0); // Placeholder

                // Patch the exit jump
                let exit_pos = self.bytecode.current_pos() as i32;
                self.bytecode
                    .patch_jump(exit_jump as usize, (exit_pos - exit_jump - 2) as i16);

                // Patch the loop back jump
                self.bytecode
                    .patch_jump(loop_jump as usize, (loop_label - loop_jump - 2) as i16);

                // Return nil after loop
                self.bytecode.emit(Instruction::Nil);
            }

            Expr::For { var, iter, body } => {
                // Implement for loop: (for x in lst (do-something-with x))
                // Compile the iterable (list)
                self.compile_expr(iter, false);

                // Store the list in a temporary location and iterate through it
                let loop_label = self.bytecode.current_pos() as i32;

                // Check if list is nil (end of iteration)
                self.bytecode.emit(Instruction::Dup); // Duplicate the list
                self.bytecode.emit(Instruction::IsNil);
                self.bytecode.emit(Instruction::JumpIfFalse);
                let body_jump = self.bytecode.current_pos() as i32;
                self.bytecode.emit_u16(0); // Placeholder for jump to body

                // If nil, exit loop
                self.bytecode.emit(Instruction::Pop);
                self.bytecode.emit(Instruction::Nil);
                self.bytecode.emit(Instruction::Jump);
                let exit_jump = self.bytecode.current_pos() as i32;
                self.bytecode.emit_u16(0); // Placeholder for exit

                // Patch body jump
                let body_pos = self.bytecode.current_pos() as i32;
                self.bytecode
                    .patch_jump(body_jump as usize, (body_pos - body_jump - 2) as i16);

                // Extract car (current element) and cdr (rest)
                self.bytecode.emit(Instruction::Dup); // Duplicate list
                self.bytecode.emit(Instruction::Car); // Get current element
                                                      // Store in variable for body
                let var_idx = self.bytecode.add_constant(Value::Symbol(*var));
                self.bytecode.emit(Instruction::StoreGlobal);
                self.bytecode.emit_u16(var_idx);

                // Get rest for next iteration
                self.bytecode.emit(Instruction::Cdr);

                // Compile body
                self.compile_expr(body, false);
                self.bytecode.emit(Instruction::Pop); // Pop body result

                // Loop back
                self.bytecode.emit(Instruction::Jump);
                let loop_jump = self.bytecode.current_pos() as i32;
                self.bytecode.emit_u16(0); // Placeholder

                // Patch exit jump
                let exit_pos = self.bytecode.current_pos() as i32;
                self.bytecode
                    .patch_jump(exit_jump as usize, (exit_pos - exit_jump - 2) as i16);

                // Patch the loop back jump
                self.bytecode
                    .patch_jump(loop_jump as usize, (loop_label - loop_jump - 2) as i16);
            }

            Expr::Match {
                value,
                patterns,
                default,
            } => {
                // Compile the value to match against
                self.compile_expr(value, false);
                let mut exit_jumps = Vec::new();
                let mut pending_jumps: Vec<Vec<usize>> = Vec::new();

                // Compile all patterns
                for (pattern, body_expr) in patterns {
                    // If we have pending jumps from the previous pattern, patch them now
                    // They should jump to this position (start of this pattern check)
                    if !pending_jumps.is_empty() {
                        let target = self.bytecode.instructions.len();
                        for jump_positions in pending_jumps.drain(..) {
                            for jump_idx in jump_positions {
                                let offset = (target as i32) - (jump_idx as i32 + 2);
                                self.bytecode.patch_jump(jump_idx, offset as i16);
                            }
                        }
                    }

                    // Compile pattern check and collect jumps that should be patched when we know
                    // where the next pattern (or default) starts
                    let pattern_jumps = self.compile_pattern_check(pattern);
                    pending_jumps.push(pattern_jumps);

                    // Pattern matched - compile the body
                    // If the body is a lambda (pattern variables), keep the matched value on stack
                    // to apply to the lambda. Otherwise, pop it.
                    let is_lambda = matches!(body_expr, Expr::Lambda { .. });
                    if is_lambda {
                        // Keep matched value on stack to apply to lambda
                        self.compile_expr(body_expr, false);
                        // Apply lambda to matched value: (lambda-expr matched-value)
                        self.bytecode.emit(Instruction::Call);
                        self.bytecode.emit_byte(1); // 1 argument
                    } else {
                        // No pattern variables, pop the value and execute body
                        self.bytecode.emit(Instruction::Pop);
                        self.compile_expr(body_expr, tail);
                    }

                    // Jump to end of match
                    self.bytecode.emit(Instruction::Jump);
                    exit_jumps.push(self.bytecode.instructions.len());
                    self.bytecode.emit_i16(0);
                }

                // Patch any remaining jumps from the last pattern to point to default
                let default_start = self.bytecode.instructions.len();
                for jump_positions in pending_jumps.drain(..) {
                    for jump_idx in jump_positions {
                        let offset = (default_start as i32) - (jump_idx as i32 + 2);
                        self.bytecode.patch_jump(jump_idx, offset as i16);
                    }
                }

                // Default/fallback case
                if let Some(default_expr) = default {
                    self.compile_expr(default_expr, tail);
                } else {
                    self.bytecode.emit(Instruction::Nil);
                }

                // Patch all exit jumps to the end
                let end_pos = self.bytecode.instructions.len();
                for jump_idx in exit_jumps {
                    let offset = (end_pos as i32) - (jump_idx as i32 + 2);
                    self.bytecode.patch_jump(jump_idx, offset as i16);
                }
            }

            Expr::Try {
                body,
                catch: _,
                finally,
            } => {
                // Try-catch implementation
                // For now: compile body, then optionally execute finally
                // Full exception handling requires VM-level support for stack unwinding

                self.compile_expr(body, false);

                // Finally block: always executed after try/catch
                if let Some(finally_expr) = finally {
                    // Save the result
                    self.bytecode.emit(Instruction::Dup);
                    self.compile_expr(finally_expr, false);
                    self.bytecode.emit(Instruction::Pop);
                    // The original result stays on stack
                }

                // NOTE: Catch handlers will need VM support to:
                // 1. Check if body produced an exception
                // 2. Unwind stack to try frame
                // 3. Bind exception to catch variable
                // 4. Execute handler
                // For now, parsing works but catch is not yet functional
            }

            Expr::Quote(expr) => {
                // Quote: return the expression itself without evaluation
                // For Phase 2, we treat quoted expressions as literal data
                // This would require converting AST to Value representation
                self.compile_expr(expr, tail);
            }

            Expr::Quasiquote(expr) => {
                // Quasiquote: quote with unquote support
                // For Phase 2, similar to quote but tracks unquote positions
                self.compile_expr(expr, tail);
            }

            Expr::Unquote(expr) => {
                // Unquote: evaluate inside quasiquote
                self.compile_expr(expr, tail);
            }

            Expr::DefMacro {
                name: _,
                params: _,
                body: _,
            } => {
                // DefMacro: Just return nil
                // The actual macro registration happens during parsing (value_to_expr)
                // where the macro definition is stored in the symbol table
                self.bytecode.emit(Instruction::Nil);
            }

            Expr::Module {
                name: _,
                exports: _,
                body,
            } => {
                // Module definition: compile body in module context
                self.compile_expr(body, tail);
            }

            Expr::Import { module: _ } => {
                // Import: no runtime effect in Phase 2
                // Would load module definitions at compile time
                self.bytecode.emit(Instruction::Nil);
            }

            Expr::ModuleRef { module: _, name: _ } => {
                // Module-qualified reference: resolved during compilation
                // For Phase 2, treat as regular global variable lookup
                self.bytecode.emit(Instruction::Nil);
            }

            Expr::Throw { value: _ } => {
                // Throw is compiled as a function call during value_to_expr
                // This case should never be reached, but we handle it for exhaustiveness
                self.bytecode.emit(Instruction::Nil);
            }

            Expr::And(exprs) => {
                // Short-circuit AND: returns first falsy value or last value
                // (and) => true, (and a) => a, (and a b c) => c if all truthy, else first falsy
                if exprs.is_empty() {
                    self.bytecode.emit(Instruction::True);
                    return;
                }

                let mut end_jumps = Vec::new();

                for (i, expr) in exprs.iter().enumerate() {
                    self.compile_expr(expr, false);

                    // For all but the last expression, check if it's false
                    if i < exprs.len() - 1 {
                        // Dup the value to check it without consuming it
                        self.bytecode.emit(Instruction::Dup);
                        self.bytecode.emit(Instruction::Not);
                        self.bytecode.emit(Instruction::JumpIfTrue);
                        let exit_jump = self.bytecode.instructions.len();
                        self.bytecode.emit_u16(0); // Placeholder

                        // Pop the duplicate for the next evaluation
                        self.bytecode.emit(Instruction::Pop);

                        end_jumps.push(exit_jump);
                    }
                }

                // Patch all exit jumps (for falsy values) to the end
                let end_pos = self.bytecode.instructions.len();
                for jump_pos in end_jumps {
                    let offset = (end_pos as i32) - (jump_pos as i32 + 2);
                    self.bytecode.patch_jump(jump_pos, offset as i16);
                }
            }

            Expr::Or(exprs) => {
                // Short-circuit OR: returns first truthy value or last value
                // (or) => false, (or a) => a, (or a b c) => a if truthy, else next...
                if exprs.is_empty() {
                    self.bytecode.emit(Instruction::False);
                    return;
                }

                let mut end_jumps = Vec::new();

                for (i, expr) in exprs.iter().enumerate() {
                    self.compile_expr(expr, false);

                    // For all but the last expression, check if it's true
                    if i < exprs.len() - 1 {
                        // Dup the value to check it without consuming it
                        self.bytecode.emit(Instruction::Dup);
                        self.bytecode.emit(Instruction::JumpIfTrue);
                        let exit_jump = self.bytecode.instructions.len();
                        self.bytecode.emit_u16(0); // Placeholder

                        // Pop the duplicate for the next evaluation
                        self.bytecode.emit(Instruction::Pop);

                        end_jumps.push(exit_jump);
                    }
                }

                // Patch all exit jumps (for truthy values) to the end
                let end_pos = self.bytecode.instructions.len();
                for jump_pos in end_jumps {
                    let offset = (end_pos as i32) - (jump_pos as i32 + 2);
                    self.bytecode.patch_jump(jump_pos, offset as i16);
                }
            }

            Expr::Xor(_) => {
                // XOR is transformed to a function call in the converter
                // This case should never be reached, but we handle it for exhaustiveness
                panic!("Xor expression should be transformed to a function call");
            }
        }
    }

    /// Compile pattern matching check. Returns list of jump positions to patch if pattern fails.
    fn compile_pattern_check(&mut self, pattern: &super::ast::Pattern) -> Vec<usize> {
        use super::ast::Pattern;

        match pattern {
            Pattern::Wildcard => {
                // Wildcard matches anything, no check needed
                Vec::new()
            }
            Pattern::Nil => {
                // Check if value is nil
                self.bytecode.emit(Instruction::Dup);
                self.bytecode.emit(Instruction::Nil);
                self.bytecode.emit(Instruction::Eq);
                self.bytecode.emit(Instruction::JumpIfFalse);
                let fail_jump = self.bytecode.instructions.len();
                self.bytecode.emit_i16(0);
                vec![fail_jump]
            }
            Pattern::Literal(val) => {
                // Check if value equals literal
                self.bytecode.emit(Instruction::Dup);
                let const_idx = self.bytecode.add_constant(val.clone());
                self.bytecode.emit(Instruction::LoadConst);
                self.bytecode.emit_u16(const_idx);
                self.bytecode.emit(Instruction::Eq);
                self.bytecode.emit(Instruction::JumpIfFalse);
                let fail_jump = self.bytecode.instructions.len();
                self.bytecode.emit_i16(0);
                vec![fail_jump]
            }
            Pattern::Var(_var_id) => {
                // Variable pattern always matches - no type check needed
                Vec::new()
            }
            Pattern::Cons { head: _, tail: _ } => {
                // Cons pattern: check if it's a pair/cons cell
                self.bytecode.emit(Instruction::Dup);
                self.bytecode.emit(Instruction::IsPair);
                self.bytecode.emit(Instruction::JumpIfFalse);
                let fail_jump = self.bytecode.instructions.len();
                self.bytecode.emit_i16(0);
                // Full cons pattern matching would recursively compile head/tail patterns
                // For Phase 2, just check if it's a pair
                vec![fail_jump]
            }
            Pattern::List(_patterns) => {
                // List pattern: for Phase 2, just check if it's a list
                // Full implementation would check length and match elements
                // For now, accept any value
                Vec::new()
            }
            Pattern::Guard {
                pattern: inner,
                condition: _,
            } => {
                // Guard pattern: check inner pattern first, then condition
                // Full guard implementation would evaluate the condition
                // For Phase 2, just check the pattern
                self.compile_pattern_check(inner)
            }
        }
    }

    fn finish(self) -> Bytecode {
        self.bytecode
    }
}

/// Compile an expression to bytecode
pub fn compile(expr: &Expr) -> Bytecode {
    let mut compiler = Compiler::new();
    compiler.compile_expr(expr, true);
    compiler.bytecode.emit(Instruction::Return);
    compiler.finish()
}
