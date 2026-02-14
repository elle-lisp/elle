mod utils;

use super::analysis::{analyze_lambda_mutations, analyze_mutated_vars};
use super::ast::{CaptureInfo, Expr};
use super::bytecode::{Bytecode, Instruction};
use super::effects::EffectContext;
use crate::binding::VarRef;
use crate::error::LocationMap;
use crate::symbol::SymbolTable;
use crate::value::{Closure, SymbolId, Value};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use utils::collect_defines;

struct Compiler {
    bytecode: Bytecode,
    #[allow(dead_code)]
    symbols: HashMap<SymbolId, usize>,
    scope_depth: usize,
    // Phase 4: Track lambda locals for proper cell-based storage
    lambda_locals: Vec<SymbolId>,
    lambda_captures_len: usize,
    lambda_params_len: usize,
    // Track current lambda's capture and param symbols for nested lambda compilation
    lambda_capture_syms: Vec<SymbolId>,
    lambda_param_syms: Vec<SymbolId>,
    // Phase 2: Symbol table and effect context for effect inference
    symbol_table: Option<Rc<SymbolTable>>,
    effect_context: EffectContext,
    // Track variables that are mutated across all closures in the current scope
    // This ensures all closures that capture these variables wrap them in cells
    scope_mutated_vars: HashSet<SymbolId>,
}

impl Compiler {
    fn new() -> Self {
        Compiler {
            bytecode: Bytecode::new(),
            symbols: HashMap::new(),
            scope_depth: 0,
            lambda_locals: Vec::new(),
            lambda_captures_len: 0,
            lambda_params_len: 0,
            lambda_capture_syms: Vec::new(),
            lambda_param_syms: Vec::new(),
            symbol_table: None,
            effect_context: EffectContext::new(),
            scope_mutated_vars: HashSet::new(),
        }
    }

    fn with_symbols(symbol_table: Rc<SymbolTable>) -> Self {
        Compiler {
            bytecode: Bytecode::new(),
            symbols: HashMap::new(),
            scope_depth: 0,
            lambda_locals: Vec::new(),
            lambda_captures_len: 0,
            lambda_params_len: 0,
            lambda_capture_syms: Vec::new(),
            lambda_param_syms: Vec::new(),
            symbol_table: Some(symbol_table.clone()),
            effect_context: EffectContext::with_symbols(&symbol_table),
            scope_mutated_vars: HashSet::new(),
        }
    }

    fn compile_expr(&mut self, expr: &Expr, tail: bool) {
        match expr {
            Expr::Literal(val) => {
                self.compile_literal(val);
            }

            Expr::Var(var_ref) => {
                match var_ref {
                    VarRef::Local { index } => {
                        // Local variable in current lambda frame (closure environment)
                        self.bytecode.emit(Instruction::LoadUpvalue);
                        self.bytecode.emit_byte(1); // depth = 1 (current closure)
                        self.bytecode.emit_byte(*index as u8);
                    }
                    VarRef::LetBound { sym } => {
                        // Let-bound variable - use LoadGlobal which checks scope stack first
                        let idx = self.bytecode.add_constant(Value::Symbol(*sym));
                        self.bytecode.emit(Instruction::LoadGlobal);
                        self.bytecode.emit_u16(idx);
                    }
                    VarRef::Upvalue { index, .. } => {
                        // Captured variable from enclosing closure
                        self.bytecode.emit(Instruction::LoadUpvalue);
                        self.bytecode.emit_byte(1); // depth = 1
                        self.bytecode.emit_byte(*index as u8);
                    }
                    VarRef::Global { sym } => {
                        let idx = self.bytecode.add_constant(Value::Symbol(*sym));
                        self.bytecode.emit(Instruction::LoadGlobal);
                        self.bytecode.emit_u16(idx);
                    }
                }
            }

            Expr::If { cond, then, else_ } => {
                self.compile_if(cond, then, else_, tail);
            }

            Expr::Begin(exprs) => {
                self.compile_begin(exprs, tail);
            }

            Expr::Block(exprs) => {
                self.compile_block(exprs, tail);
            }

            Expr::Call {
                func,
                args,
                tail: is_tail,
            } => {
                self.compile_call(func, args, *is_tail, tail);
            }

            Expr::Lambda {
                params,
                body,
                captures,
                num_locals,
                locals,
            } => {
                self.compile_lambda(params, body, captures, *num_locals, locals);
            }

            Expr::Let { bindings, body } => {
                self.compile_let(bindings, body, tail);
            }

            Expr::Letrec { bindings, body } => {
                self.compile_letrec(bindings, body, tail);
            }

            Expr::Set { target, value } => {
                self.compile_expr(value, false);
                match target {
                    VarRef::Local { index } => {
                        // Local variable in lambda frame
                        self.bytecode.emit(Instruction::StoreUpvalue);
                        self.bytecode.emit_byte(1);
                        self.bytecode.emit_byte(*index as u8);
                    }
                    VarRef::LetBound { sym } => {
                        // Let-bound variable - use StoreGlobal which checks scope stack first
                        let idx = self.bytecode.add_constant(Value::Symbol(*sym));
                        self.bytecode.emit(Instruction::StoreGlobal);
                        self.bytecode.emit_u16(idx);
                    }
                    VarRef::Upvalue { index, .. } => {
                        // Captured variable from enclosing closure
                        self.bytecode.emit(Instruction::StoreUpvalue);
                        self.bytecode.emit_byte(1);
                        self.bytecode.emit_byte(*index as u8);
                    }
                    VarRef::Global { sym } => {
                        let idx = self.bytecode.add_constant(Value::Symbol(*sym));
                        self.bytecode.emit(Instruction::StoreGlobal);
                        self.bytecode.emit_u16(idx);
                    }
                }
            }

            Expr::Define { name, value } => {
                self.compile_expr(value, false);
                if let Some(local_idx) = self.lambda_locals.iter().position(|s| s == name) {
                    // Inside a lambda: store to the pre-allocated cell in closure env
                    let env_idx = self.lambda_captures_len + self.lambda_params_len + local_idx;
                    self.bytecode.emit(Instruction::StoreUpvalue);
                    self.bytecode.emit_byte(1); // depth = 1 (current closure)
                    self.bytecode.emit_byte(env_idx as u8);
                } else if self.scope_depth > 0 {
                    // Inside a block/loop/let scope (not a lambda) — define locally
                    let idx = self.bytecode.add_constant(Value::Symbol(*name));
                    self.bytecode.emit(Instruction::DefineLocal);
                    self.bytecode.emit_u16(idx);
                } else {
                    // Top-level — define globally
                    let idx = self.bytecode.add_constant(Value::Symbol(*name));
                    self.bytecode.emit(Instruction::StoreGlobal);
                    self.bytecode.emit_u16(idx);
                }
            }

            Expr::While { cond, body } => {
                self.compile_while(cond, body);
            }

            Expr::For { var, iter, body } => {
                self.compile_for(var, iter, body);
            }

            Expr::Match {
                value,
                patterns,
                default,
            } => {
                self.compile_match(value, patterns, default, tail);
            }

            Expr::Try {
                body,
                catch,
                finally,
            } => {
                self.compile_try(body, catch, finally, tail);
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

            Expr::HandlerCase { body, handlers } => {
                self.compile_handler_case(body, handlers, tail);
            }

            Expr::HandlerBind { handlers: _, body } => {
                self.compile_handler_bind(body, tail);
            }

            Expr::And(exprs) => {
                self.compile_and(exprs);
            }

            Expr::Or(exprs) => {
                self.compile_or(exprs);
            }

            Expr::Cond { clauses, else_body } => {
                self.compile_cond(clauses, else_body, tail);
            }

            Expr::Xor(_) => {
                // XOR is transformed to a function call in the converter
                // This case should never be reached, but we handle it for exhaustiveness
                panic!("Xor expression should be transformed to a function call");
            }

            Expr::Yield(expr) => {
                // Compile the expression to yield
                self.compile_expr(expr, false);
                // Emit yield instruction
                self.bytecode.emit(Instruction::Yield);
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

    /// Compile literal values (nil, booleans, and constants)
    fn compile_literal(&mut self, val: &Value) {
        match val {
            Value::Nil => self.bytecode.emit(Instruction::Nil),
            Value::Bool(true) => self.bytecode.emit(Instruction::True),
            Value::Bool(false) => self.bytecode.emit(Instruction::False),
            _ => {
                let idx = self.bytecode.add_constant(val.clone());
                self.bytecode.emit(Instruction::LoadConst);
                self.bytecode.emit_u16(idx);
            }
        }
    }

    /// Compile an if expression
    fn compile_if(&mut self, cond: &Expr, then_expr: &Expr, else_expr: &Expr, tail: bool) {
        self.compile_expr(cond, false);
        self.bytecode.emit(Instruction::JumpIfFalse);
        let else_jump = self.bytecode.current_pos();
        self.bytecode.emit_u16(0); // Placeholder

        self.compile_expr(then_expr, tail);
        self.bytecode.emit(Instruction::Jump);
        let end_jump = self.bytecode.current_pos();
        self.bytecode.emit_u16(0); // Placeholder

        let else_pos = self.bytecode.current_pos();
        self.bytecode
            .patch_jump(else_jump, (else_pos - else_jump - 2) as i16);

        self.compile_expr(else_expr, tail);

        let end_pos = self.bytecode.current_pos();
        self.bytecode
            .patch_jump(end_jump, (end_pos - end_jump - 2) as i16);
    }

    /// Compile a while loop
    fn compile_while(&mut self, cond: &Expr, body: &Expr) {
        // Push loop scope to isolate loop variables
        self.bytecode.emit(Instruction::PushScope);
        self.bytecode.emit_byte(3); // ScopeType::Loop = 3
        self.scope_depth += 1;

        // Implement while loop using conditional jumps
        let loop_label = self.bytecode.current_pos() as i32;

        // Compile condition
        self.compile_expr(cond, false);

        // Jump to end if condition is false
        self.bytecode.emit(Instruction::JumpIfFalse);
        let exit_jump = self.bytecode.current_pos() as i32;
        self.bytecode.emit_u16(0);

        // Compile body
        self.compile_expr(body, false);
        self.bytecode.emit(Instruction::Pop);

        // Jump back to loop condition
        self.bytecode.emit(Instruction::Jump);
        let loop_jump = self.bytecode.current_pos() as i32;
        self.bytecode.emit_u16(0);

        // Patch the exit jump
        let exit_pos = self.bytecode.current_pos() as i32;
        self.bytecode
            .patch_jump(exit_jump as usize, (exit_pos - exit_jump - 2) as i16);

        // Patch the loop back jump
        self.bytecode
            .patch_jump(loop_jump as usize, (loop_label - loop_jump - 2) as i16);

        self.scope_depth -= 1;
        self.bytecode.emit(Instruction::PopScope);
        self.bytecode.emit(Instruction::Nil);
    }

    /// Compile a for loop
    fn compile_for(&mut self, var: &SymbolId, iter: &Expr, body: &Expr) {
        self.bytecode.emit(Instruction::PushScope);
        self.bytecode.emit_byte(3); // ScopeType::Loop = 3
        self.scope_depth += 1;

        // Compile the iterable
        self.compile_expr(iter, false);

        // Loop start
        let loop_label = self.bytecode.current_pos() as i32;

        // Check if list is nil
        self.bytecode.emit(Instruction::Dup);
        self.bytecode.emit(Instruction::IsNil);
        self.bytecode.emit(Instruction::JumpIfTrue);
        let exit_jump = self.bytecode.current_pos() as i32;
        self.bytecode.emit_u16(0);

        // Extract car
        self.bytecode.emit(Instruction::Dup);
        self.bytecode.emit(Instruction::Car);

        // Store in loop variable
        let var_idx = self.bytecode.add_constant(Value::Symbol(*var));
        self.bytecode.emit(Instruction::DefineLocal);
        self.bytecode.emit_u16(var_idx);
        self.bytecode.emit(Instruction::Pop);

        // Compile body
        self.compile_expr(body, false);
        self.bytecode.emit(Instruction::Pop);

        // Update list to rest
        self.bytecode.emit(Instruction::Cdr);

        // Loop back
        self.bytecode.emit(Instruction::Jump);
        let loop_jump = self.bytecode.current_pos() as i32;
        self.bytecode.emit_u16(0);

        // Patch exit jump
        let exit_pos = self.bytecode.current_pos() as i32;
        self.bytecode
            .patch_jump(exit_jump as usize, (exit_pos - exit_jump - 2) as i16);

        // Patch the loop back jump
        self.bytecode
            .patch_jump(loop_jump as usize, (loop_label - loop_jump - 2) as i16);

        self.scope_depth -= 1;
        self.bytecode.emit(Instruction::PopScope);
        self.bytecode.emit(Instruction::Pop);
        self.bytecode.emit(Instruction::Nil);
    }

    /// Compile a cond expression
    fn compile_cond(
        &mut self,
        clauses: &[(Expr, Expr)],
        else_body: &Option<Box<Expr>>,
        tail: bool,
    ) {
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

    /// Compile a begin expression with pre-declared defines
    fn compile_begin(&mut self, exprs: &[Expr], tail: bool) {
        // Pre-declare all defines to enable recursive functions and forward references
        // This allows a function to reference itself in its own body
        let temp_expr = Expr::Begin(exprs.to_vec());
        let defines = collect_defines(&temp_expr);
        for sym_id in defines {
            // Skip pre-declaration for lambda locals — their cells are pre-allocated by the Call handler
            if self.lambda_locals.contains(&sym_id) {
                continue;
            }
            // Load nil and store it
            self.bytecode.emit(Instruction::Nil);
            let idx = self.bytecode.add_constant(Value::Symbol(sym_id));
            if !self.lambda_locals.is_empty() {
                // Inside a lambda — store to closure environment
                if let Some(local_idx) = self.lambda_locals.iter().position(|s| s == &sym_id) {
                    let env_idx = self.lambda_captures_len + self.lambda_params_len + local_idx;
                    self.bytecode.emit(Instruction::StoreUpvalue);
                    self.bytecode.emit_byte(1); // depth = 1 (current closure)
                    self.bytecode.emit_byte(env_idx as u8);
                } else {
                    // Symbol is not in lambda_locals, so it's not a local variable
                    // This shouldn't happen in normal code, but we'll skip it
                    self.bytecode.emit(Instruction::Pop);
                }
            } else if self.scope_depth > 0 {
                // Inside a block/loop scope (not a lambda) — define locally
                self.bytecode.emit(Instruction::DefineLocal);
                self.bytecode.emit_u16(idx);
                // DefineLocal pushes the value back, but we don't need it for pre-declaration
                self.bytecode.emit(Instruction::Pop);
            } else {
                // Top-level — define globally
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

    /// Compile a block expression with scoped defines
    fn compile_block(&mut self, exprs: &[Expr], tail: bool) {
        // Push block scope
        self.bytecode.emit(Instruction::PushScope);
        self.bytecode.emit_byte(2); // ScopeType::Block = 2
        self.scope_depth += 1;

        // Pre-declare defines within the block for mutual visibility
        let temp_expr = Expr::Block(exprs.to_vec());
        let defines = collect_defines(&temp_expr);
        for sym_id in defines {
            self.bytecode.emit(Instruction::Nil);
            let idx = self.bytecode.add_constant(Value::Symbol(sym_id));
            self.bytecode.emit(Instruction::DefineLocal);
            self.bytecode.emit_u16(idx);
            // DefineLocal pushes the value back, but we don't need it for pre-declaration
            self.bytecode.emit(Instruction::Pop);
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

    /// Compile a short-circuit AND expression
    fn compile_and(&mut self, exprs: &[Expr]) {
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

    /// Compile a short-circuit OR expression
    fn compile_or(&mut self, exprs: &[Expr]) {
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

    /// Compile a try-catch-finally expression
    fn compile_try(
        &mut self,
        body: &Expr,
        catch: &Option<(SymbolId, Box<Expr>)>,
        finally: &Option<Box<Expr>>,
        tail: bool,
    ) {
        // Try-catch-finally implementation using handler-case mechanism
        // (try body (catch var handler) finally)
        //
        // Control flow:
        // 1. PushHandler (set up exception handler)
        // 2. Compile body
        // 3. PopHandler (clean up on success)
        // 4. Jump to finally (success path)
        // [Exception handler code - only reached if exception occurs]
        // 5. CheckException
        // 6. If catch clause: MatchException, BindException, compile handler
        // 7. ClearException (only if exception was caught)
        // [Finally code - executes for both paths]
        // 8. Compile finally if present
        // 9. ClearException (if not already cleared)

        // Emit PushHandler with placeholder
        self.bytecode.emit(Instruction::PushHandler);
        let handler_offset_pos = self.bytecode.current_pos();
        self.bytecode.emit_i16(0); // Placeholder for handler offset
        self.bytecode.emit_i16(-1); // No finally offset in handler instruction

        // Compile the protected body
        self.compile_expr(body, tail);

        // Pop handler on successful completion
        self.bytecode.emit(Instruction::PopHandler);

        // Jump past exception handler code on success
        self.bytecode.emit(Instruction::Jump);
        let success_jump_pos = self.bytecode.current_pos();
        self.bytecode.emit_i16(0); // Placeholder for jump offset

        // ============================================================
        // Exception handler code - only reached if exception occurs
        // ============================================================
        let handler_code_start = self.bytecode.current_pos() as i16;
        self.bytecode
            .patch_jump(handler_offset_pos, handler_code_start);

        // Verify exception exists
        self.bytecode.emit(Instruction::CheckException);

        let mut catch_handled_jumps = Vec::new();

        // Handle catch clause if present
        if let Some((var, handler_expr)) = catch {
            // Match exception ID 4 (general exceptions like division by zero)
            self.bytecode.emit(Instruction::MatchException);
            self.bytecode.emit_u16(4);

            // If exception doesn't match, jump to unhandled path
            self.bytecode.emit(Instruction::JumpIfFalse);
            let unhandled_jump_pos = self.bytecode.current_pos();
            self.bytecode.emit_i16(0); // Placeholder

            // Exception matched - bind to variable
            self.bytecode.emit(Instruction::BindException);
            let var_idx = self.bytecode.add_constant(Value::Symbol(*var));
            self.bytecode.emit_u16(var_idx);

            // Compile catch handler body
            self.compile_expr(handler_expr, tail);

            // Clear exception after successful catch
            self.bytecode.emit(Instruction::ClearException);

            // Jump to finally code
            self.bytecode.emit(Instruction::Jump);
            catch_handled_jumps.push(self.bytecode.current_pos());
            self.bytecode.emit_i16(0); // Placeholder

            // Patch unhandled jump - exception doesn't match
            let unhandled_path = self.bytecode.current_pos() as i16;
            self.bytecode.patch_jump(unhandled_jump_pos, unhandled_path);

            // For unhandled exceptions: just skip to end
            // (exception state is preserved, will propagate)
            self.bytecode.emit(Instruction::Jump);
            catch_handled_jumps.push(self.bytecode.current_pos());
            self.bytecode.emit_i16(0); // Placeholder
        }

        // ============================================================
        // Finally code and end
        // ============================================================
        let finally_start = self.bytecode.current_pos();

        // Patch success jump to finally
        let relative_offset = (finally_start - success_jump_pos - 2) as i16;
        self.bytecode.patch_jump(success_jump_pos, relative_offset);

        // Patch catch handler jumps to finally
        for jump_pos in catch_handled_jumps {
            let relative_offset = (finally_start - jump_pos - 2) as i16;
            self.bytecode.patch_jump(jump_pos, relative_offset);
        }

        // Compile finally block if present
        if let Some(finally_expr) = finally {
            // Save result from try or catch
            self.bytecode.emit(Instruction::Dup);
            self.compile_expr(finally_expr, false);
            self.bytecode.emit(Instruction::Pop);
            // Result stays on stack
        }

        // Clear exception state (clears any unhandled exceptions too)
        // Note: if exception was unhandled, this will still clear it
        // For unhandled exceptions to propagate, we'd need different logic
        self.bytecode.emit(Instruction::ClearException);
    }

    /// Compile a handler-case expression (immediate stack unwinding on exception)
    fn compile_handler_case(
        &mut self,
        body: &Expr,
        handlers: &[(u32, SymbolId, Box<Expr>)],
        tail: bool,
    ) {
        // handler-case: immediate stack unwinding on exception
        // (handler-case protected (type1 (var1) handler1) ...)

        // Emit PushHandler with placeholder offsets (will be patched later)
        self.bytecode.emit(Instruction::PushHandler);
        let pushhandler_pos = self.bytecode.current_pos(); // Position right after PushHandler instruction
        let handler_offset_pos = pushhandler_pos; // Where we'll patch the offset (right after instruction byte)
        self.bytecode.emit_i16(0); // Placeholder for handler_offset
        self.bytecode.emit_i16(-1); // No finally block for now

        // Compile the protected body
        self.compile_expr(body, tail);

        // Emit PopHandler to clean up on successful completion
        self.bytecode.emit(Instruction::PopHandler);

        // Jump past handler clauses after successful execution
        self.bytecode.emit(Instruction::Jump);
        let end_jump = self.bytecode.current_pos();
        self.bytecode.emit_i16(0); // Placeholder for end jump

        // Patch the handler_offset to point here
        // Using absolute position - the interrupt mechanism will handle it correctly
        let handler_code_offset = self.bytecode.current_pos() as i16;
        self.bytecode
            .patch_jump(handler_offset_pos, handler_code_offset);

        // Emit CheckException (only reached if an exception actually occurred)
        self.bytecode.emit(Instruction::CheckException);

        // Compile each handler clause
        let mut handler_end_jumps = Vec::new();
        for (exception_id, var, handler_expr) in handlers {
            // Emit match check instruction with exception ID as immediate
            self.bytecode.emit(Instruction::MatchException);
            self.bytecode.emit_u16(*exception_id as u16);

            // If doesn't match, jump to next handler
            self.bytecode.emit(Instruction::JumpIfFalse);
            let next_handler_jump = self.bytecode.current_pos();
            self.bytecode.emit_i16(0); // Placeholder for next handler

            // Handler matches - bind the exception to the handler variable
            self.bytecode.emit(Instruction::BindException);
            let var_idx = self.bytecode.add_constant(Value::Symbol(*var));
            self.bytecode.emit_u16(var_idx);

            // Execute handler code
            self.compile_expr(handler_expr, tail);

            // Jump past remaining handlers on success
            self.bytecode.emit(Instruction::Jump);
            handler_end_jumps.push(self.bytecode.current_pos());
            self.bytecode.emit_i16(0); // Placeholder for end

            // Patch the next handler jump
            let next_handler_offset = self.bytecode.current_pos() as i16;
            self.bytecode
                .patch_jump(next_handler_jump, next_handler_offset);
        }

        // Patch all handler end jumps to the final end (before ClearException)
        let final_end_pos = self.bytecode.current_pos();

        // Patch handler end jumps (Jump instructions use relative offsets)
        for jump_pos in handler_end_jumps {
            // Relative jump: from jump_pos+2 (after the 2-byte offset) to final_end_pos
            let relative_offset = (final_end_pos - jump_pos - 2) as i16;
            self.bytecode.patch_jump(jump_pos, relative_offset);
        }

        // Patch the end jump from after PopHandler (Jump instruction uses relative offset)
        // Relative jump: from end_jump+2 to final_end_pos
        let relative_offset = (final_end_pos - end_jump - 2) as i16;
        self.bytecode.patch_jump(end_jump, relative_offset);

        // Clear exception state
        self.bytecode.emit(Instruction::ClearException);
    }

    /// Compile a handler-bind expression (non-unwinding handler attachment)
    fn compile_handler_bind(&mut self, body: &Expr, tail: bool) {
        // handler-bind: non-unwinding handler attachment
        // (handler-bind ((type handler-fn) ...) body)
        // Handlers are called but don't unwind the stack

        // For now, just compile the body (no unwinding handlers supported yet)
        // TODO: Implement actual handler-bind execution with non-unwinding semantics
        self.compile_expr(body, tail);
    }

    /// Compile a let binding expression with proper scope isolation
    fn compile_let(&mut self, bindings: &[(SymbolId, Expr)], body: &Expr, tail: bool) {
        // Let-bindings create a local scope with proper parallel binding semantics.
        // All binding expressions are evaluated BEFORE any variables are defined,
        // so bindings cannot see each other (only outer scope).

        // Analyze which variables are mutated across all closures in the let bindings and body
        // This ensures all closures that capture these variables wrap them in cells
        let mut bindings_mutated = HashSet::new();
        for (_var, expr) in bindings {
            bindings_mutated.extend(analyze_lambda_mutations(expr));
        }
        bindings_mutated.extend(analyze_lambda_mutations(body));
        let old_scope_mutated = std::mem::replace(&mut self.scope_mutated_vars, bindings_mutated);

        // First, compile ALL binding expressions (values go on stack)
        // This happens BEFORE the let scope is pushed, so bindings see outer scope only
        for (_var, expr) in bindings {
            self.compile_expr(expr, false);
        }

        // Now push the Let scope
        self.bytecode.emit(Instruction::PushScope);
        self.bytecode.emit_byte(4); // ScopeType::Let = 4

        // Define all variables in reverse order (since values are on stack in LIFO order)
        // Stack has: [val1, val2, val3, ...] with val_n on top
        // We define in reverse so that var_n gets val_n, var_(n-1) gets val_(n-1), etc.
        // Create a set of binding variables for filtering
        let binding_vars: HashSet<SymbolId> = bindings.iter().map(|(var, _)| *var).collect();
        for (var, _expr) in bindings.iter().rev() {
            // If this variable is mutated by any closure in the let body, wrap it in a cell
            // Only wrap if it's a binding in this let (not a local variable in a nested lambda)
            if self.scope_mutated_vars.contains(var) && binding_vars.contains(var) {
                self.bytecode.emit(Instruction::MakeCell);
            }
            let idx = self.bytecode.add_constant(Value::Symbol(*var));
            self.bytecode.emit(Instruction::DefineLocal);
            self.bytecode.emit_u16(idx);
            // DefineLocal pushes the value back, but we don't need it
            self.bytecode.emit(Instruction::Pop);
        }

        // Compile the body in the let scope
        self.compile_expr(body, tail);

        // Pop the let scope
        self.bytecode.emit(Instruction::PopScope);

        // Restore the old scope_mutated_vars
        self.scope_mutated_vars = old_scope_mutated;
    }

    /// Compile a letrec binding expression where bindings are mutually visible
    fn compile_letrec(&mut self, bindings: &[(SymbolId, Expr)], body: &Expr, tail: bool) {
        // Analyze which variables are mutated across all closures in the letrec bindings and body
        let mut bindings_mutated = HashSet::new();
        for (_var, expr) in bindings {
            bindings_mutated.extend(analyze_lambda_mutations(expr));
        }
        bindings_mutated.extend(analyze_lambda_mutations(body));
        let old_scope_mutated = std::mem::replace(&mut self.scope_mutated_vars, bindings_mutated);

        // Letrec creates a scope where all bindings are mutually visible
        // Pre-declare all binding names as nil, then update them with their values
        self.bytecode.emit(Instruction::PushScope);
        self.bytecode.emit_byte(4); // ScopeType::Let = 4
        self.scope_depth += 1;

        // Pre-declare all binding names as nil (enables mutual references)
        for (var, _) in bindings {
            self.bytecode.emit(Instruction::Nil);
            let idx = self.bytecode.add_constant(Value::Symbol(*var));
            self.bytecode.emit(Instruction::DefineLocal);
            self.bytecode.emit_u16(idx);
        }

        // Compile each binding expression and update the scope
        for (var, expr) in bindings {
            self.compile_expr(expr, false);
            let idx = self.bytecode.add_constant(Value::Symbol(*var));
            self.bytecode.emit(Instruction::DefineLocal);
            self.bytecode.emit_u16(idx);
        }

        // Compile the body
        self.compile_expr(body, tail);

        self.scope_depth -= 1;
        self.bytecode.emit(Instruction::PopScope);

        // Restore the old scope_mutated_vars
        self.scope_mutated_vars = old_scope_mutated;
    }

    /// Compile a function call expression
    fn compile_call(&mut self, func: &Expr, args: &[Expr], is_tail: bool, tail: bool) {
        // Compile arguments
        for arg in args {
            self.compile_expr(arg, false);
        }

        // Compile function
        self.compile_expr(func, false);

        // Emit call
        if tail && is_tail {
            self.bytecode.emit(Instruction::TailCall);
        } else {
            self.bytecode.emit(Instruction::Call);
        }
        self.bytecode.emit_byte(args.len() as u8);
    }

    /// Compile a lambda (closure creation) expression
    fn compile_lambda(
        &mut self,
        params: &[SymbolId],
        body: &Expr,
        captures: &[CaptureInfo],
        num_locals: usize,
        locals: &[SymbolId],
    ) {
        let num_captures = captures.len();
        // Phase 4: Locally-defined variables are now part of the closure environment
        // The closure environment layout is: [captures..., parameters..., locals...]
        // Each local is pre-allocated as a cell in the environment
        // We NO LONGER use PushScope/PopScope for lambda bodies - all variables are in closure_env

        // Phase 2: Infer the effect of the lambda body
        let effect = self.effect_context.infer_lambda_effect(body);

        let mut lambda_compiler = if let Some(ref symbol_table) = self.symbol_table {
            Compiler::with_symbols(symbol_table.clone())
        } else {
            Compiler::new()
        };
        lambda_compiler.scope_depth = 0; // NOT inside a scope (Phase 4: no scope_stack for lambdas)
        lambda_compiler.lambda_locals = locals.to_vec(); // Locally-defined symbols
        lambda_compiler.lambda_captures_len = num_captures;
        lambda_compiler.lambda_params_len = params.len();
        // Track this lambda's capture and param symbols for nested lambda compilation
        lambda_compiler.lambda_capture_syms = captures.iter().map(|c| c.sym).collect();
        lambda_compiler.lambda_param_syms = params.to_vec();

        // Compile the body directly (no scope management)
        lambda_compiler.compile_expr(body, true);

        // Return from the lambda
        lambda_compiler.bytecode.emit(Instruction::Return);

        // Create closure value with environment
        // Note: env is empty here, actual capturing happens at runtime via MakeClosure instruction
        // num_locals includes: parameters + captures + locally-defined variables
        // The environment layout will be: [captures..., parameters..., locals...]

        // Store the original AST for JIT compilation
        let source_ast = Some(Rc::new(crate::value::JitLambda {
            params: params.to_vec(),
            body: Box::new(body.clone()),
            captures: captures.iter().map(|c| c.sym).collect(),
            effect,
        }));

        let closure = Closure {
            bytecode: Rc::new(lambda_compiler.bytecode.instructions),
            arity: crate::value::Arity::Exact(params.len()),
            env: Rc::new(Vec::new()), // Will be populated by VM when closure is created
            num_locals,
            num_captures,
            constants: Rc::new(lambda_compiler.bytecode.constants),
            source_ast,
            effect,
        };

        let idx = self.bytecode.add_constant(Value::Closure(Rc::new(closure)));

        if num_captures == 0 && num_locals == params.len() {
            // No captures AND no locals — just load the closure template directly as a constant
            // No need for MakeClosure instruction
            self.bytecode.emit(Instruction::LoadConst);
            self.bytecode.emit_u16(idx);
        } else if num_captures == 0 {
            // Has locals but no external captures — still need MakeClosure for closure env
            // so that nested lambdas can access locally-defined variables via LoadUpvalueRaw
            self.bytecode.emit(Instruction::MakeClosure);
            self.bytecode.emit_u16(idx);
            self.bytecode.emit_byte(0); // 0 captures
        } else {
            // Has captures — emit capture loads + MakeClosure as before
            // First, analyze which variables are mutated in the lambda body
            let mutated_vars = analyze_mutated_vars(body);
            // Also include variables that are mutated across all closures in the current scope
            let all_mutated_vars: HashSet<SymbolId> = mutated_vars
                .union(&self.scope_mutated_vars)
                .copied()
                .collect();
            let mutated_captures: HashSet<SymbolId> = captures
                .iter()
                .map(|c| c.sym)
                .filter(|sym| all_mutated_vars.contains(sym))
                .collect();

            // Emit captured values onto the stack (in order)
            // These will be stored in the closure's environment by the VM
            for capture_info in captures.iter() {
                // Check if this is a locally-defined variable that hasn't been initialized yet
                // (in the CURRENT lambda, not the enclosing lambda)
                if let Some(_idx) = locals.iter().position(|s| *s == capture_info.sym) {
                    // This is a forward reference - we need to create a cell that will be updated later
                    // Push nil as a placeholder
                    let nil_idx = self.bytecode.add_constant(Value::Nil);
                    self.bytecode.emit(Instruction::LoadConst);
                    self.bytecode.emit_u16(nil_idx);
                    // Wrap in a cell so it can be updated later
                    self.bytecode.emit(Instruction::MakeCell);
                    continue;
                }

                // Find where this symbol lives in the current (enclosing) closure's environment
                let (env_index, from_locals) = if let Some(idx) = self
                    .lambda_capture_syms
                    .iter()
                    .position(|s| *s == capture_info.sym)
                {
                    // Found in enclosing lambda's captures
                    (idx, false)
                } else if let Some(idx) = self
                    .lambda_param_syms
                    .iter()
                    .position(|s| *s == capture_info.sym)
                {
                    // Found in enclosing lambda's params
                    (self.lambda_captures_len + idx, false)
                } else if let Some(idx) = self
                    .lambda_locals
                    .iter()
                    .position(|s| *s == capture_info.sym)
                {
                    // Found in enclosing lambda's locally-defined variables
                    // Use LoadUpvalueRaw to capture the cell itself (for self-recursion)
                    (
                        self.lambda_captures_len + self.lambda_params_len + idx,
                        true,
                    )
                } else {
                    // Not found in enclosing closure - use LoadGlobal
                    match &capture_info.source {
                        VarRef::LetBound { sym } | VarRef::Global { sym } => {
                            let sym_idx = self.bytecode.add_constant(Value::Symbol(*sym));
                            self.bytecode.emit(Instruction::LoadGlobal);
                            self.bytecode.emit_u16(sym_idx);

                            // Only create a new cell if this capture is mutated AND wasn't already
                            // wrapped in a cell by the let binding (scope_mutated_vars contains it)
                            if mutated_captures.contains(&capture_info.sym)
                                && !self.scope_mutated_vars.contains(&capture_info.sym)
                            {
                                self.bytecode.emit(Instruction::MakeCell);
                            }
                            continue;
                        }
                        VarRef::Local { index } => {
                            // Local in enclosing scope - use the index directly
                            // (This handles locally-defined variables)
                            // Use LoadUpvalueRaw to capture the cell itself (for self-recursion)
                            (
                                self.lambda_captures_len + self.lambda_params_len + *index,
                                true,
                            )
                        }
                        _ => {
                            // Fallback - shouldn't happen
                            (0, false)
                        }
                    }
                };

                // Load from enclosing closure's environment
                if from_locals {
                    // Use LoadUpvalueRaw to capture the cell itself (for self-recursion)
                    self.bytecode.emit(Instruction::LoadUpvalueRaw);
                } else {
                    self.bytecode.emit(Instruction::LoadUpvalue);
                }
                self.bytecode.emit_byte(1);
                self.bytecode.emit_byte(env_index as u8);

                if mutated_captures.contains(&capture_info.sym) {
                    self.bytecode.emit(Instruction::MakeCell);
                }
            }

            // Create closure with captured values
            self.bytecode.emit(Instruction::MakeClosure);
            self.bytecode.emit_u16(idx);
            self.bytecode.emit_byte(num_captures as u8);
        }
    }

    /// Compile a match expression with pattern matching
    fn compile_match(
        &mut self,
        value: &Expr,
        patterns: &[(super::ast::Pattern, Expr)],
        default: &Option<Box<Expr>>,
        tail: bool,
    ) {
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

/// Compile an expression to bytecode with symbol table for effect inference
pub fn compile_with_symbols(expr: &Expr, symbols: Rc<SymbolTable>) -> Bytecode {
    let mut compiler = Compiler::with_symbols(symbols);
    compiler.compile_expr(expr, true);
    compiler.bytecode.emit(Instruction::Return);
    compiler.finish()
}

/// Compile an expression to bytecode with source location metadata
///
/// Returns a tuple of (bytecode, location_map) where the location_map
/// contains the mapping from bytecode instruction index to source location.
///
/// Note: Currently returns an empty location map. Full metadata tracking
/// will be implemented in a future phase.
pub fn compile_with_metadata(
    expr: &Expr,
    _location: Option<crate::reader::SourceLoc>,
) -> (Bytecode, LocationMap) {
    let bytecode = compile(expr);
    let location_map = LocationMap::new(); // Empty for now - phase 2 will populate this
    (bytecode, location_map)
}

/// Compile a lambda expression to a Closure at runtime
///
/// This is used by the CPS interpreter to compile pure lambdas that are
/// created inside coroutines.
pub fn compile_lambda_to_closure(
    params: &[SymbolId],
    body: &Expr,
    captures: &[CaptureInfo],
    num_locals: usize,
    capture_values: Vec<Value>,
    effect: crate::compiler::effects::Effect,
) -> Result<Closure, String> {
    let num_captures = captures.len();
    // Create a compiler for the lambda body
    let mut lambda_compiler = Compiler::new();
    lambda_compiler.scope_depth = 0;
    lambda_compiler.lambda_locals = Vec::new();
    lambda_compiler.lambda_captures_len = num_captures;
    lambda_compiler.lambda_params_len = params.len();

    // Compile the body
    lambda_compiler.compile_expr(body, true);

    // Return from the lambda
    lambda_compiler.bytecode.emit(Instruction::Return);

    // Store the original AST for JIT compilation
    let source_ast = Some(Rc::new(crate::value::JitLambda {
        params: params.to_vec(),
        body: Box::new(body.clone()),
        captures: captures.iter().map(|c| c.sym).collect(),
        effect,
    }));

    // Create the closure
    let closure = Closure {
        bytecode: Rc::new(lambda_compiler.bytecode.instructions),
        arity: crate::value::Arity::Exact(params.len()),
        env: Rc::new(capture_values),
        num_locals,
        num_captures,
        constants: Rc::new(lambda_compiler.bytecode.constants),
        source_ast,
        effect,
    };

    Ok(closure)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    #[test]
    fn test_compile_pure_lambda() {
        // (fn (x) (+ x 1))
        let params = vec![crate::value::SymbolId(1)];
        let body = Box::new(Expr::Call {
            func: Box::new(Expr::Var(VarRef::global(crate::value::SymbolId(2)))), // +
            args: vec![Expr::Var(VarRef::local(0)), Expr::Literal(Value::Int(1))],
            tail: false,
        });

        let expr = Expr::Lambda {
            params: params.clone(),
            body: body.clone(),
            captures: vec![],
            num_locals: 1,
            locals: vec![],
        };

        let bytecode = compile(&expr);
        // Should compile without errors
        assert!(!bytecode.instructions.is_empty());
    }

    #[test]
    fn test_compile_with_symbols_infers_effect() {
        let mut symbols = crate::symbol::SymbolTable::new();
        let x_sym = symbols.intern("x");
        let plus_sym = symbols.intern("+");

        // (fn (x) (+ x 1))
        let params = vec![x_sym];
        let body = Box::new(Expr::Call {
            func: Box::new(Expr::Var(VarRef::global(plus_sym))),
            args: vec![Expr::Var(VarRef::local(0)), Expr::Literal(Value::Int(1))],
            tail: false,
        });

        let expr = Expr::Lambda {
            params: params.clone(),
            body: body.clone(),
            captures: vec![],
            num_locals: 1,
            locals: vec![],
        };

        let bytecode = compile_with_symbols(&expr, Rc::new(symbols));
        // Should compile without errors
        assert!(!bytecode.instructions.is_empty());
    }

    #[test]
    fn test_closure_stores_effect() {
        // Create a simple lambda and verify the closure stores the effect
        let params = vec![crate::value::SymbolId(1)];
        let body = Box::new(Expr::Literal(Value::Int(42)));

        let expr = Expr::Lambda {
            params: params.clone(),
            body: body.clone(),
            captures: vec![],
            num_locals: 1,
            locals: vec![],
        };

        let bytecode = compile(&expr);
        // The closure should be stored as a constant
        assert!(!bytecode.constants.is_empty());

        // Find the closure constant
        let closure_found = bytecode
            .constants
            .iter()
            .any(|v| matches!(v, Value::Closure(_)));
        assert!(closure_found, "Closure should be stored as a constant");
    }

    #[test]
    fn test_nested_lambda_effect() {
        // ((fn (x) (fn (y) (+ x y))) 1)
        let x_sym = crate::value::SymbolId(1);
        let y_sym = crate::value::SymbolId(2);
        let plus_sym = crate::value::SymbolId(3);

        let inner_lambda = Expr::Lambda {
            params: vec![y_sym],
            body: Box::new(Expr::Call {
                func: Box::new(Expr::Var(VarRef::global(plus_sym))),
                args: vec![
                    Expr::Var(VarRef::upvalue(x_sym, 0, false)), // x from outer scope
                    Expr::Var(VarRef::local(0)),                 // y from inner scope
                ],
                tail: false,
            }),
            captures: vec![CaptureInfo {
                sym: x_sym,
                source: VarRef::local(0), // x is local in outer scope
            }],
            num_locals: 2,
            locals: vec![],
        };

        let outer_lambda = Expr::Lambda {
            params: vec![x_sym],
            body: Box::new(inner_lambda),
            captures: vec![],
            num_locals: 1,
            locals: vec![],
        };

        let bytecode = compile(&outer_lambda);
        assert!(!bytecode.instructions.is_empty());
    }

    #[test]
    fn test_lambda_with_locals() {
        // (fn (x) (define y 1) (+ x y))
        let x_sym = crate::value::SymbolId(1);
        let y_sym = crate::value::SymbolId(2);
        let plus_sym = crate::value::SymbolId(3);

        let body = Box::new(Expr::Begin(vec![
            Expr::Define {
                name: y_sym,
                value: Box::new(Expr::Literal(Value::Int(1))),
            },
            Expr::Call {
                func: Box::new(Expr::Var(VarRef::global(plus_sym))),
                args: vec![Expr::Var(VarRef::local(0)), Expr::Var(VarRef::local(1))],
                tail: false,
            },
        ]));

        let expr = Expr::Lambda {
            params: vec![x_sym],
            body,
            captures: vec![],
            num_locals: 2,
            locals: vec![],
        };

        let bytecode = compile(&expr);
        assert!(!bytecode.instructions.is_empty());
    }

    #[test]
    fn test_pure_function_effect_inference() {
        // Test that a simple arithmetic function is inferred as Pure
        let mut symbols = crate::symbol::SymbolTable::new();
        let x_sym = symbols.intern("x");
        let plus_sym = symbols.intern("+");

        // (fn (x) (+ x 1))
        let expr = Expr::Lambda {
            params: vec![x_sym],
            body: Box::new(Expr::Call {
                func: Box::new(Expr::Var(VarRef::global(plus_sym))),
                args: vec![Expr::Var(VarRef::local(0)), Expr::Literal(Value::Int(1))],
                tail: false,
            }),
            captures: vec![],
            num_locals: 1,
            locals: vec![],
        };

        let bytecode = compile_with_symbols(&expr, Rc::new(symbols));

        // Find the closure constant
        let closure = bytecode.constants.iter().find_map(|v| {
            if let Value::Closure(c) = v {
                Some(c.clone())
            } else {
                None
            }
        });

        assert!(closure.is_some(), "Closure should be stored as a constant");
        let closure = closure.unwrap();

        // Verify the effect is Pure
        assert_eq!(
            closure.effect,
            crate::compiler::effects::Effect::Pure,
            "Simple arithmetic function should be Pure"
        );
    }

    #[test]
    fn test_nested_function_effect_inference() {
        // Test that a function calling another pure function is inferred as Pure
        let mut symbols = crate::symbol::SymbolTable::new();
        let x_sym = symbols.intern("x");
        let abs_sym = symbols.intern("abs");
        let plus_sym = symbols.intern("+");

        // (fn (x) (+ (abs x) 1))
        let expr = Expr::Lambda {
            params: vec![x_sym],
            body: Box::new(Expr::Call {
                func: Box::new(Expr::Var(VarRef::global(plus_sym))),
                args: vec![
                    Expr::Call {
                        func: Box::new(Expr::Var(VarRef::global(abs_sym))),
                        args: vec![Expr::Var(VarRef::local(0))],
                        tail: false,
                    },
                    Expr::Literal(Value::Int(1)),
                ],
                tail: false,
            }),
            captures: vec![],
            num_locals: 1,
            locals: vec![],
        };

        let bytecode = compile_with_symbols(&expr, Rc::new(symbols));

        let closure = bytecode.constants.iter().find_map(|v| {
            if let Value::Closure(c) = v {
                Some(c.clone())
            } else {
                None
            }
        });

        assert!(closure.is_some());
        let closure = closure.unwrap();
        assert_eq!(
            closure.effect,
            crate::compiler::effects::Effect::Pure,
            "Nested pure function calls should be Pure"
        );
    }

    #[test]
    fn test_closure_effect_propagation() {
        // Test that effect is stored in both JitLambda and Closure
        let mut symbols = crate::symbol::SymbolTable::new();
        let x_sym = symbols.intern("x");
        let plus_sym = symbols.intern("+");

        let expr = Expr::Lambda {
            params: vec![x_sym],
            body: Box::new(Expr::Call {
                func: Box::new(Expr::Var(VarRef::global(plus_sym))),
                args: vec![Expr::Var(VarRef::local(0)), Expr::Literal(Value::Int(1))],
                tail: false,
            }),
            captures: vec![],
            num_locals: 1,
            locals: vec![],
        };

        let bytecode = compile_with_symbols(&expr, Rc::new(symbols));

        let closure = bytecode
            .constants
            .iter()
            .find_map(|v| {
                if let Value::Closure(c) = v {
                    Some(c.clone())
                } else {
                    None
                }
            })
            .expect("Closure should exist");

        // Check that effect is in the Closure
        assert_eq!(closure.effect, crate::compiler::effects::Effect::Pure);

        // Check that effect is also in the JitLambda (source_ast)
        if let Some(jit_lambda) = &closure.source_ast {
            assert_eq!(jit_lambda.effect, crate::compiler::effects::Effect::Pure);
        }
    }

    #[test]
    fn test_lambda_with_captures_effect() {
        // Test that a lambda with captures still infers effect correctly
        let mut symbols = crate::symbol::SymbolTable::new();
        let x_sym = symbols.intern("x");
        let y_sym = symbols.intern("y");
        let plus_sym = symbols.intern("+");

        // (fn (y) (+ x y)) where x is captured
        let expr = Expr::Lambda {
            params: vec![y_sym],
            body: Box::new(Expr::Call {
                func: Box::new(Expr::Var(VarRef::global(plus_sym))),
                args: vec![
                    Expr::Var(VarRef::upvalue(x_sym, 0, false)), // x from outer scope
                    Expr::Var(VarRef::local(0)),                 // y from this scope
                ],
                tail: false,
            }),
            captures: vec![CaptureInfo {
                sym: x_sym,
                source: VarRef::global(x_sym), // x from outer scope (simulated as global for test)
            }],
            num_locals: 2,
            locals: vec![],
        };

        let bytecode = compile_with_symbols(&expr, Rc::new(symbols));

        let closure = bytecode
            .constants
            .iter()
            .find_map(|v| {
                if let Value::Closure(c) = v {
                    Some(c.clone())
                } else {
                    None
                }
            })
            .expect("Closure should exist");

        assert_eq!(closure.effect, crate::compiler::effects::Effect::Pure);
        assert_eq!(closure.num_captures, 1);
    }

    #[test]
    fn test_lambda_with_locals_effect() {
        // Test that a lambda with locally-defined variables infers effect correctly
        let mut symbols = crate::symbol::SymbolTable::new();
        let x_sym = symbols.intern("x");
        let y_sym = symbols.intern("y");
        let plus_sym = symbols.intern("+");

        // (fn (x) (define y 1) (+ x y))
        let expr = Expr::Lambda {
            params: vec![x_sym],
            body: Box::new(Expr::Begin(vec![
                Expr::Define {
                    name: y_sym,
                    value: Box::new(Expr::Literal(Value::Int(1))),
                },
                Expr::Call {
                    func: Box::new(Expr::Var(VarRef::global(plus_sym))),
                    args: vec![Expr::Var(VarRef::local(0)), Expr::Var(VarRef::local(1))],
                    tail: false,
                },
            ])),
            captures: vec![],
            num_locals: 2,
            locals: vec![],
        };

        let bytecode = compile_with_symbols(&expr, Rc::new(symbols));

        let closure = bytecode
            .constants
            .iter()
            .find_map(|v| {
                if let Value::Closure(c) = v {
                    Some(c.clone())
                } else {
                    None
                }
            })
            .expect("Closure should exist");

        assert_eq!(closure.effect, crate::compiler::effects::Effect::Pure);
        assert_eq!(closure.num_locals, 2); // x (param) + y (local)
    }
}
