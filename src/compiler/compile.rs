use super::ast::Expr;
use super::bytecode::{Bytecode, Instruction};
use crate::value::{Closure, SymbolId, Value};
use std::collections::HashMap;
use std::rc::Rc;

struct Compiler {
    bytecode: Bytecode,
    #[allow(dead_code)]
    symbols: HashMap<SymbolId, usize>,
    scope_depth: usize,
}

impl Compiler {
    fn new() -> Self {
        Compiler {
            bytecode: Bytecode::new(),
            symbols: HashMap::new(),
            scope_depth: 0,
        }
    }

    /// Collect all define statements from an expression
    /// Returns a vector of symbol IDs that are defined at this level
    /// Recursively collects from nested structures like while/for loop bodies
    fn collect_defines(expr: &Expr) -> Vec<SymbolId> {
        let mut defines = Vec::new();
        let mut seen = std::collections::HashSet::new();

        fn collect_recursive(
            expr: &Expr,
            defines: &mut Vec<SymbolId>,
            seen: &mut std::collections::HashSet<u32>,
        ) {
            match expr {
                Expr::Begin(exprs) => {
                    for e in exprs {
                        if let Expr::Define { name, .. } = e {
                            if seen.insert(name.0) {
                                defines.push(*name);
                            }
                        }
                        // Also recursively collect from nested structures
                        collect_recursive(e, defines, seen);
                    }
                }
                Expr::Define { name, .. } => {
                    if seen.insert(name.0) {
                        defines.push(*name);
                    }
                }
                Expr::While { body, .. } | Expr::For { body, .. } => {
                    collect_recursive(body, defines, seen);
                }
                _ => {}
            }
        }

        collect_recursive(expr, &mut defines, &mut seen);
        defines
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
                // Only do this at global scope, not inside loops/blocks
                if self.scope_depth == 0 {
                    let defines = Self::collect_defines(expr);
                    for sym_id in defines {
                        // Load nil and store it in the global
                        self.bytecode.emit(Instruction::Nil);
                        let idx = self.bytecode.add_constant(Value::Symbol(sym_id));
                        self.bytecode.emit(Instruction::StoreGlobal);
                        self.bytecode.emit_u16(idx);
                    }
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

            Expr::Block(exprs) => {
                // Push block scope
                self.bytecode.emit(Instruction::PushScope);
                self.bytecode.emit_byte(2); // ScopeType::Block = 2
                self.scope_depth += 1;

                // Pre-declare defines within the block for mutual visibility
                let defines = Self::collect_defines(expr);
                for sym_id in defines {
                    self.bytecode.emit(Instruction::Nil);
                    let idx = self.bytecode.add_constant(Value::Symbol(sym_id));
                    self.bytecode.emit(Instruction::DefineLocal);
                    self.bytecode.emit_u16(idx);
                }

                // Compile expressions
                for (i, expr) in exprs.iter().enumerate() {
                    let is_last = i == exprs.len() - 1;
                    self.compile_expr(expr, tail && is_last);
                    if !is_last {
                        self.bytecode.emit(Instruction::Pop);
                    }
                }

                self.scope_depth -= 1;
                self.bytecode.emit(Instruction::PopScope);
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
                    num_captures: captures.len(),
                    constants: Rc::new(lambda_compiler.bytecode.constants),
                };

                let idx = self.bytecode.add_constant(Value::Closure(Rc::new(closure)));

                // Emit captured values onto the stack (in order)
                // These will be stored in the closure's environment by the VM
                for (sym, depth, index) in captures {
                    if *index == usize::MAX {
                        // This is a global variable - load it as a global
                        let sym_idx = self.bytecode.add_constant(Value::Symbol(*sym));
                        self.bytecode.emit(Instruction::LoadGlobal);
                        self.bytecode.emit_u16(sym_idx);
                    } else {
                        // This is a local variable from an outer scope
                        // Load it using LoadUpvalue with the resolved depth and index
                        self.bytecode.emit(Instruction::LoadUpvalue);
                        self.bytecode.emit_byte((*depth + 1) as u8);
                        self.bytecode.emit_byte(*index as u8);
                    }
                }

                // Create closure with captured values
                self.bytecode.emit(Instruction::MakeClosure);
                self.bytecode.emit_u16(idx);
                self.bytecode.emit_byte(captures.len() as u8);
            }

            Expr::Let { bindings, body } => {
                // Let-bindings create a local scope with proper isolation
                // NOTE: Currently, let-bindings are transformed to lambda calls at the converter stage
                // (see src/compiler/converters.rs), so this code is never reached in normal execution.
                // This implementation is preserved for future direct let-binding compilation.

                // Push a Let scope
                self.bytecode.emit(Instruction::PushScope);
                self.bytecode.emit_byte(4); // ScopeType::Let = 4

                // Compile and store each binding in the local scope
                for (var, expr) in bindings {
                    // Compile the binding expression
                    self.compile_expr(expr, false);
                    // Define the variable in the let scope
                    let idx = self.bytecode.add_constant(Value::Symbol(*var));
                    self.bytecode.emit(Instruction::DefineLocal);
                    self.bytecode.emit_u16(idx);
                }

                // Compile the body in the let scope
                self.compile_expr(body, tail);

                // Pop the let scope
                self.bytecode.emit(Instruction::PopScope);
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
                if self.scope_depth > 0 {
                    // Inside a scope (loop, block) — define locally
                    self.bytecode.emit(Instruction::Dup);
                    self.bytecode.emit(Instruction::DefineLocal);
                } else {
                    // Top-level — define globally
                    self.bytecode.emit(Instruction::StoreGlobal);
                }
                self.bytecode.emit_u16(idx);
            }

            Expr::While { cond, body } => {
                // Push loop scope to isolate loop variables
                self.bytecode.emit(Instruction::PushScope);
                self.bytecode.emit_byte(3); // ScopeType::Loop = 3
                self.scope_depth += 1;

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

                self.scope_depth -= 1;
                // Pop loop scope
                self.bytecode.emit(Instruction::PopScope);

                // Return nil after loop
                self.bytecode.emit(Instruction::Nil);
            }

            Expr::For { var, iter, body } => {
                // Push loop scope to isolate loop variables
                self.bytecode.emit(Instruction::PushScope);
                self.bytecode.emit_byte(3); // ScopeType::Loop = 3
                self.scope_depth += 1;

                // Implement for loop: (for x lst (do-something-with x))
                // Compile the iterable (list)
                self.compile_expr(iter, false);

                // Loop start: check if list is nil
                let loop_label = self.bytecode.current_pos() as i32;

                // Check if list is nil (end of iteration)
                // Stack: [list]
                self.bytecode.emit(Instruction::Dup); // Stack: [list, list]
                self.bytecode.emit(Instruction::IsNil);
                self.bytecode.emit(Instruction::JumpIfTrue);
                let exit_jump = self.bytecode.current_pos() as i32;
                self.bytecode.emit_u16(0); // Placeholder for exit jump
                                           // Stack: [list]

                // List is not nil: Extract car (current element)
                self.bytecode.emit(Instruction::Dup); // Stack: [list, list]
                self.bytecode.emit(Instruction::Car); // Stack: [list, first_element]

                // Store element in loop variable (locally, not globally)
                let var_idx = self.bytecode.add_constant(Value::Symbol(*var));
                self.bytecode.emit(Instruction::DefineLocal);
                self.bytecode.emit_u16(var_idx);
                // DefineLocal pops the value without pushing back
                // Stack: [list, first_element] -> DefineLocal pops first_element -> [list]

                // Compile body (body may reference the loop variable, but won't consume the list)
                self.compile_expr(body, false);
                self.bytecode.emit(Instruction::Pop); // Pop body result
                                                      // Stack: [list]

                // Update list to rest for next iteration
                self.bytecode.emit(Instruction::Cdr); // Stack: [rest_of_list]

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

                self.scope_depth -= 1;
                // Pop loop scope
                self.bytecode.emit(Instruction::PopScope);

                // Pop the nil list from stack and push nil (loop return value)
                // Stack: [nil]
                self.bytecode.emit(Instruction::Pop);
                self.bytecode.emit(Instruction::Nil);
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

            Expr::Cond { clauses, else_body } => {
                // Cond expression: evaluates test expressions until one is truthy
                // Syntax: (cond (test1 body1) (test2 body2) ... [(else body)])
                //
                // Compilation strategy:
                // For each clause:
                //   1. Compile test expression
                //   2. JumpIfFalse to next clause
                //   3. Compile body (in tail position if tail is true)
                //   4. Jump to end
                // For else clause (if present):
                //   1. Compile body (in tail position if tail is true)
                // If no else clause:
                //   1. Load nil

                if clauses.is_empty() {
                    // (cond) with no clauses => nil, or else_body if present
                    if let Some(else_expr) = else_body {
                        self.compile_expr(else_expr, tail);
                    } else {
                        self.bytecode.emit(Instruction::Nil);
                    }
                    return;
                }

                let mut end_jumps = Vec::new();

                // Compile each clause
                for (test, body) in clauses {
                    self.compile_expr(test, false);

                    self.bytecode.emit(Instruction::JumpIfFalse);
                    let next_clause_jump = self.bytecode.instructions.len();
                    self.bytecode.emit_u16(0); // Placeholder for next clause

                    // Compile the body
                    self.compile_expr(body, tail);

                    // Jump to end after executing this body
                    self.bytecode.emit(Instruction::Jump);
                    let end_jump = self.bytecode.instructions.len();
                    self.bytecode.emit_u16(0); // Placeholder for end
                    end_jumps.push(end_jump);

                    // Patch the jump to next clause
                    let next_clause_pos = self.bytecode.instructions.len();
                    let offset = (next_clause_pos as i32) - (next_clause_jump as i32 + 2);
                    self.bytecode.patch_jump(next_clause_jump, offset as i16);
                }

                // Handle else clause or nil
                if let Some(else_expr) = else_body {
                    self.compile_expr(else_expr, tail);
                } else {
                    self.bytecode.emit(Instruction::Nil);
                }

                // Patch all end jumps
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

            Expr::ScopeVar(depth, index) => {
                // Scoped variable reference (from outer scopes at runtime)
                // This will be handled by Phase 2 VM runtime scope stack
                // For now, emit LoadUpvalue as a placeholder
                self.bytecode.emit(Instruction::LoadUpvalue);
                self.bytecode.emit_byte((*depth + 1) as u8);
                self.bytecode.emit_byte(*index as u8);
            }

            Expr::ScopeEntry(scope_type) => {
                // Push a new scope onto the runtime scope stack
                // This will be implemented in Phase 2 with PushScope instruction
                // For now, this is a no-op (will be handled by Phase 2)
                let _ = scope_type; // Suppress unused warning
            }

            Expr::ScopeExit => {
                // Pop the current scope from the runtime scope stack
                // This will be implemented in Phase 2 with PopScope instruction
                // For now, this is a no-op (will be handled by Phase 2)
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
