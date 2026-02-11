// Cranelift code generation for Elle Lisp expressions
//
// This module handles the core logic of translating Elle AST expressions
// into Cranelift IR (CLIF) and compiling to native x86_64 code.

use super::branching::BranchManager;
use super::codegen::IrEmitter;
use super::context::JITContext;
use super::scoping::ScopeManager;
use super::stack_allocator::{SlotType, StackAllocator};
use crate::compiler::ast::Expr;
use crate::symbol::SymbolTable;
use crate::value::Value;
use cranelift::prelude::*;
use cranelift_jit::JITModule;
use cranelift_module::Module;

/// Represents a compiled expression value in CLIF IR
/// Maps Elle values to Cranelift SSA values
#[derive(Debug, Clone, Copy)]
pub enum IrValue {
    /// An i64 SSA value (nil, bool, int, or encoded float)
    I64(cranelift::prelude::Value),
    /// An f64 SSA value (unboxed float)
    F64(cranelift::prelude::Value),
}

/// Compilation context for JIT code generation with variable support
pub struct CompileContext<'a, 'b, 'c> {
    pub builder: &'a mut FunctionBuilder<'b>,
    pub symbols: &'a SymbolTable,
    pub scope_manager: ScopeManager,
    pub stack_allocator: StackAllocator,
    pub module: &'c mut JITModule,
}

impl<'a, 'b, 'c> CompileContext<'a, 'b, 'c> {
    pub fn new(
        builder: &'a mut FunctionBuilder<'b>,
        symbols: &'a SymbolTable,
        module: &'c mut JITModule,
    ) -> Self {
        CompileContext {
            builder,
            symbols,
            scope_manager: ScopeManager::new(),
            stack_allocator: StackAllocator::new(),
            module,
        }
    }
}

/// Expression compiler
pub struct ExprCompiler;

impl ExprCompiler {
    /// Compile a single expression to a function
    pub fn compile_expr(
        ctx: &mut JITContext,
        name: &str,
        expr: &Expr,
        symbols: &SymbolTable,
    ) -> Result<*const u8, String> {
        // Create function signature: fn(args_ptr: i64, args_len: i64) -> i64
        let mut sig = ctx.make_signature();
        sig.params.push(AbiParam::new(types::I64)); // args pointer
        sig.params.push(AbiParam::new(types::I64)); // args length
        sig.returns.push(AbiParam::new(types::I64)); // return value

        let func_id = ctx.declare_function(name, sig)?;

        // Set the signature before building
        ctx.ctx.func.signature = ctx.module.make_signature();
        ctx.ctx
            .func
            .signature
            .params
            .push(AbiParam::new(types::I64));
        ctx.ctx
            .func
            .signature
            .params
            .push(AbiParam::new(types::I64));
        ctx.ctx
            .func
            .signature
            .returns
            .push(AbiParam::new(types::I64));

        let mut builder = FunctionBuilder::new(&mut ctx.ctx.func, &mut ctx.builder_ctx);
        let entry_block = builder.create_block();
        builder.append_block_param(entry_block, types::I64); // args pointer
        builder.append_block_param(entry_block, types::I64); // args length
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        // Create compilation context with scope/stack support
        let mut compile_ctx = CompileContext::new(&mut builder, symbols, &mut ctx.module);

        // Compile the expression
        let result = Self::compile_expr_block(&mut compile_ctx, expr)?;

        // Convert the compiled value to i64 for return
        let return_val = match result {
            IrValue::I64(v) => v,
            IrValue::F64(_v) => {
                // TODO: Encode float as its bit representation (i64)
                // For now, return 0
                compile_ctx.builder.ins().iconst(types::I64, 0)
            }
        };
        compile_ctx.builder.ins().return_(&[return_val]);

        // Drop the context to release the mutable borrow on builder
        drop(compile_ctx);
        builder.finalize();

        ctx.define_function(func_id)?;
        ctx.clear();

        Ok(ctx.get_function(func_id))
    }

    /// Compile an expression within a builder block
    /// Returns an IrValue (Cranelift SSA value)
    pub fn compile_expr_block(ctx: &mut CompileContext, expr: &Expr) -> Result<IrValue, String> {
        match expr {
            Expr::Literal(val) => Self::compile_literal(ctx, val),
            Expr::Var(sym_id, depth, index) => Self::compile_var(ctx, *sym_id, *depth, *index),
            Expr::Set {
                var,
                depth,
                index,
                value,
            } => Self::compile_set(ctx, *var, *depth, *index, value),
            Expr::Begin(exprs) => Self::compile_begin(ctx, exprs),
            Expr::If { cond, then, else_ } => Self::compile_if(ctx, cond, then, else_),
            Expr::Cond { clauses, else_body } => Self::try_compile_cond(ctx, clauses, else_body),
            // Try to compile And/Or with shortcircuiting
            Expr::And(exprs) => Self::try_compile_and(ctx, exprs),
            Expr::Or(exprs) => Self::try_compile_or(ctx, exprs),
            // Try to compile Let bindings
            Expr::Let { bindings, body } => Self::try_compile_let(ctx, bindings, body),
            // Try to compile While loops
            Expr::While { cond, body } => Self::try_compile_while(ctx, cond, body),
            // Try to compile For loops
            Expr::For { var, iter, body } => Self::try_compile_for(ctx, *var, iter, body),
            // Try to compile binary operations with integer operands
            Expr::Call { func, args, .. } if args.len() == 2 => {
                Self::try_compile_binop(ctx, func, args)
            }
            // Try to compile unary operations like empty?
            Expr::Call { func, args, .. } if args.len() == 1 => {
                Self::try_compile_unary_op(ctx, func, args)
            }
            _ => Err(format!(
                "Expression type not yet supported in JIT: {:?}",
                expr
            )),
        }
    }

    /// Compile a variable reference (load from stack)
    fn compile_var(
        ctx: &mut CompileContext,
        _sym_id: crate::value::SymbolId,
        depth: usize,
        index: usize,
    ) -> Result<IrValue, String> {
        let (slot, slot_type) = ctx
            .stack_allocator
            .get_with_type(depth, index)
            .ok_or_else(|| format!("Variable not allocated at depth={}, index={}", depth, index))?;

        match slot_type {
            SlotType::I64 => {
                let value = ctx.builder.ins().stack_load(types::I64, slot, 0);
                Ok(IrValue::I64(value))
            }
            SlotType::F64 => {
                let value = ctx.builder.ins().stack_load(types::F64, slot, 0);
                Ok(IrValue::F64(value))
            }
        }
    }

    /// Compile a set! expression (store to stack)
    fn compile_set(
        ctx: &mut CompileContext,
        _var: crate::value::SymbolId,
        depth: usize,
        index: usize,
        value: &Expr,
    ) -> Result<IrValue, String> {
        // Compile the value expression
        let compiled_value = Self::compile_expr_block(ctx, value)?;

        // Get the stack slot (must already be allocated)
        let (slot, slot_type) =
            ctx.stack_allocator
                .get_with_type(depth, index)
                .ok_or_else(|| {
                    format!(
                        "Cannot set! unbound variable at depth={}, index={}",
                        depth, index
                    )
                })?;

        match (compiled_value, slot_type) {
            (IrValue::I64(v), SlotType::I64) => {
                ctx.builder.ins().stack_store(v, slot, 0);
                Ok(IrValue::I64(v))
            }
            (IrValue::F64(v), SlotType::F64) => {
                ctx.builder.ins().stack_store(v, slot, 0);
                Ok(IrValue::F64(v))
            }
            (IrValue::I64(v), SlotType::F64) => {
                // Convert i64 to f64
                let converted = ctx.builder.ins().fcvt_from_sint(types::F64, v);
                ctx.builder.ins().stack_store(converted, slot, 0);
                Ok(IrValue::F64(converted))
            }
            (IrValue::F64(v), SlotType::I64) => {
                // Convert f64 to i64 (truncate)
                let converted = ctx.builder.ins().fcvt_to_sint(types::I64, v);
                ctx.builder.ins().stack_store(converted, slot, 0);
                Ok(IrValue::I64(converted))
            }
        }
    }

    /// Try to compile a Cond expression (multi-way conditional)
    fn try_compile_cond(
        ctx: &mut CompileContext,
        clauses: &[(Expr, Expr)],
        else_body: &Option<Box<Expr>>,
    ) -> Result<IrValue, String> {
        if clauses.is_empty() {
            // No clauses - return else body or nil
            if let Some(else_expr) = else_body {
                return Self::compile_expr_block(ctx, else_expr);
            } else {
                return Ok(IrValue::I64(ctx.builder.ins().iconst(types::I64, 0)));
                // nil
            }
        }

        // Create blocks for each clause + one for end
        let mut clause_blocks = Vec::new();
        for _ in 0..clauses.len() {
            clause_blocks.push(ctx.builder.create_block());
        }
        let end_block = ctx.builder.create_block();

        let zero = ctx.builder.ins().iconst(types::I64, 0);

        // Evaluate first condition
        let (first_cond, first_body) = &clauses[0];
        let cond_val = Self::compile_expr_block(ctx, first_cond)?;
        let cond_i64 = match cond_val {
            IrValue::I64(v) => v,
            _ => return Err("Cond condition must be I64".to_string()),
        };

        let is_true = ctx.builder.ins().icmp(IntCC::NotEqual, cond_i64, zero);

        // Jump to first body if true, else to second clause or else block
        let next_block = if clauses.len() > 1 {
            clause_blocks[1]
        } else if else_body.is_some() {
            let else_eval_block = ctx.builder.create_block();
            ctx.builder.switch_to_block(else_eval_block);
            ctx.builder.seal_block(else_eval_block);
            if let Some(else_expr) = else_body {
                let else_val = Self::compile_expr_block(ctx, else_expr)?;
                let else_i64 = Self::ir_value_to_i64(ctx.builder, else_val)?;
                ctx.builder.ins().jump(end_block, &[else_i64]);
            } else {
                ctx.builder.ins().jump(end_block, &[zero]);
            }
            else_eval_block
        } else {
            end_block
        };

        ctx.builder
            .ins()
            .brif(is_true, clause_blocks[0], &[], next_block, &[]);

        // Compile first clause body
        ctx.builder.switch_to_block(clause_blocks[0]);
        ctx.builder.seal_block(clause_blocks[0]);
        let first_body_val = Self::compile_expr_block(ctx, first_body)?;
        let first_body_i64 = Self::ir_value_to_i64(ctx.builder, first_body_val)?;
        ctx.builder.ins().jump(end_block, &[first_body_i64]);

        // Compile remaining clauses
        for i in 1..clauses.len() {
            ctx.builder.switch_to_block(clause_blocks[i]);
            ctx.builder.seal_block(clause_blocks[i]);

            let (cond, body) = &clauses[i];
            let cond_val = Self::compile_expr_block(ctx, cond)?;
            let cond_i64 = match cond_val {
                IrValue::I64(v) => v,
                _ => return Err("Cond condition must be I64".to_string()),
            };

            let is_true = ctx.builder.ins().icmp(IntCC::NotEqual, cond_i64, zero);

            let next_block = if i + 1 < clauses.len() {
                clause_blocks[i + 1]
            } else if else_body.is_some() {
                let else_eval_block = ctx.builder.create_block();
                ctx.builder.switch_to_block(else_eval_block);
                ctx.builder.seal_block(else_eval_block);
                if let Some(else_expr) = else_body {
                    let else_val = Self::compile_expr_block(ctx, else_expr)?;
                    let else_i64 = Self::ir_value_to_i64(ctx.builder, else_val)?;
                    ctx.builder.ins().jump(end_block, &[else_i64]);
                } else {
                    ctx.builder.ins().jump(end_block, &[zero]);
                }
                else_eval_block
            } else {
                end_block
            };

            ctx.builder
                .ins()
                .brif(is_true, clause_blocks[i], &[], next_block, &[]);

            ctx.builder.switch_to_block(clause_blocks[i]);
            ctx.builder.seal_block(clause_blocks[i]);
            let body_val = Self::compile_expr_block(ctx, body)?;
            let body_i64 = Self::ir_value_to_i64(ctx.builder, body_val)?;
            ctx.builder.ins().jump(end_block, &[body_i64]);
        }

        ctx.builder.switch_to_block(end_block);
        ctx.builder.seal_block(end_block);
        let param = ctx.builder.block_params(end_block)[0];

        Ok(IrValue::I64(param))
    }

    /// Try to compile a Let expression
    fn try_compile_let(
        ctx: &mut CompileContext,
        bindings: &[(crate::value::SymbolId, Expr)],
        body: &Expr,
    ) -> Result<IrValue, String> {
        // Push a new scope
        ctx.scope_manager.push_scope();

        // Compile each binding
        for (sym_id, binding_expr) in bindings {
            let binding_val = Self::compile_expr_block(ctx, binding_expr)?;

            // Bind in scope and allocate stack slot
            let (depth, index) = ctx.scope_manager.bind(*sym_id);

            // Determine slot type from the compiled value
            let slot_type = match binding_val {
                IrValue::I64(_) => SlotType::I64,
                IrValue::F64(_) => SlotType::F64,
            };

            let (slot, _) = ctx
                .stack_allocator
                .allocate(ctx.builder, depth, index, slot_type)?;

            // Store value with correct type
            match binding_val {
                IrValue::I64(v) => {
                    ctx.builder.ins().stack_store(v, slot, 0);
                }
                IrValue::F64(v) => {
                    ctx.builder.ins().stack_store(v, slot, 0);
                }
            }
        }

        // Compile body
        let result = Self::compile_expr_block(ctx, body)?;

        // Pop scope
        ctx.scope_manager.pop_scope().map_err(|e| e.to_string())?;

        Ok(result)
    }

    /// Determine the slot type for a list of values (for for-loop iteration)
    fn determine_list_slot_type(elements: &[Value]) -> Result<SlotType, String> {
        if elements.is_empty() {
            return Ok(SlotType::I64); // Default for empty lists
        }

        let first_is_float = matches!(&elements[0], Value::Float(_));

        for elem in elements.iter().skip(1) {
            let is_float = matches!(elem, Value::Float(_));
            if is_float != first_is_float {
                return Err("Mixed int/float elements in for loop not yet supported".to_string());
            }
        }

        Ok(if first_is_float {
            SlotType::F64
        } else {
            SlotType::I64
        })
    }

    /// Try to compile a For loop expression
    /// (for var iterable body) - iterates over elements, binding to var
    ///
    /// Supports: For loops over literal lists (unrolled at compile time)
    /// Not supported: For loops over runtime-computed iterables (requires variable binding)
    fn try_compile_for(
        ctx: &mut CompileContext,
        var: crate::value::SymbolId,
        iter: &Expr,
        body: &Expr,
    ) -> Result<IrValue, String> {
        // Check if the iterable is a literal list that we can unroll
        match iter {
            Expr::Literal(Value::Nil) => {
                // Empty list - for loop body never executes, return nil
                Ok(IrValue::I64(ctx.builder.ins().iconst(types::I64, 0)))
            }
            Expr::Literal(Value::Cons(cons_rc)) => {
                // Collect elements from literal cons list
                let mut elements = Vec::new();
                let mut current = (**cons_rc).clone();
                loop {
                    elements.push(current.first.clone());
                    match &current.rest {
                        Value::Nil => break,
                        Value::Cons(next_cons) => current = (**next_cons).clone(),
                        _ => return Err("For loop over improper list not supported".to_string()),
                    }
                }

                // Determine the slot type for the loop variable
                let slot_type = Self::determine_list_slot_type(&elements)?;

                // Push loop scope
                ctx.scope_manager.push_scope();

                // Bind loop variable and allocate stack slot with correct type
                let (depth, index) = ctx.scope_manager.bind(var);
                let (slot, _) =
                    ctx.stack_allocator
                        .allocate(ctx.builder, depth, index, slot_type)?;

                let mut result = IrValue::I64(ctx.builder.ins().iconst(types::I64, 0));

                for elem in elements {
                    // Compile and store the element value
                    match (&elem, slot_type) {
                        (Value::Nil, SlotType::I64) => {
                            let v = ctx.builder.ins().iconst(types::I64, 0);
                            ctx.builder.ins().stack_store(v, slot, 0);
                        }
                        (Value::Bool(b), SlotType::I64) => {
                            let v = ctx.builder.ins().iconst(types::I64, if *b { 1 } else { 0 });
                            ctx.builder.ins().stack_store(v, slot, 0);
                        }
                        (Value::Int(i), SlotType::I64) => {
                            let v = ctx.builder.ins().iconst(types::I64, *i);
                            ctx.builder.ins().stack_store(v, slot, 0);
                        }
                        (Value::Float(f), SlotType::F64) => {
                            let v = ctx.builder.ins().f64const(*f);
                            ctx.builder.ins().stack_store(v, slot, 0);
                        }
                        _ => {
                            ctx.scope_manager.pop_scope().ok();
                            return Err(format!(
                                "Unsupported element type in for loop: {:?}",
                                elem
                            ));
                        }
                    }

                    // Compile body (can now reference the loop variable)
                    result = Self::compile_expr_block(ctx, body)?;
                }

                // Pop loop scope
                ctx.scope_manager.pop_scope().map_err(|e| e.to_string())?;

                Ok(result)
            }
            _ => {
                // Runtime-computed iterables - compile the iterable expression and use runtime helpers
                Self::compile_for_runtime(ctx, var, iter, body)
            }
        }
    }

    /// Compile a for loop over a runtime-computed iterable
    /// Uses runtime helper functions (jit_car, jit_cdr, jit_is_nil) to iterate
    fn compile_for_runtime(
        ctx: &mut CompileContext,
        var: crate::value::SymbolId,
        iter: &Expr,
        body: &Expr,
    ) -> Result<IrValue, String> {
        // 1. Compile the iterable expression
        let list_val = Self::compile_expr_block(ctx, iter)?;
        let list_i64 = match list_val {
            IrValue::I64(v) => v,
            _ => return Err("For loop iterable must evaluate to I64".to_string()),
        };

        // 2. Create loop blocks
        let header_block = ctx.builder.create_block();
        let body_block = ctx.builder.create_block();
        let exit_block = ctx.builder.create_block();

        // 3. Push scope and allocate loop variable
        ctx.scope_manager.push_scope();
        let (depth, index) = ctx.scope_manager.bind(var);
        let (var_slot, _) =
            ctx.stack_allocator
                .allocate(ctx.builder, depth, index, SlotType::I64)?;

        // 4. Allocate slot for list iterator pointer
        use cranelift::codegen::ir::StackSlotData;
        use cranelift::codegen::ir::StackSlotKind;
        let iter_slot = ctx
            .builder
            .create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8));

        // Store initial list value
        ctx.builder.ins().stack_store(list_i64, iter_slot, 0);

        // Jump to header
        ctx.builder.ins().jump(header_block, &[]);

        // 5. Header block: check if list is nil
        ctx.builder.switch_to_block(header_block);
        // Don't seal yet - back edge from body

        let current_list = ctx.builder.ins().stack_load(types::I64, iter_slot, 0);

        // Call jit_is_nil
        let is_nil_result = Self::call_helper(ctx, "jit_is_nil", current_list)?;

        let zero = ctx.builder.ins().iconst(types::I64, 0);
        let is_nil = ctx.builder.ins().icmp(IntCC::NotEqual, is_nil_result, zero);
        ctx.builder
            .ins()
            .brif(is_nil, exit_block, &[], body_block, &[]);

        // 6. Body block
        ctx.builder.switch_to_block(body_block);
        ctx.builder.seal_block(body_block);

        // Get car (current element)
        let current = ctx.builder.ins().stack_load(types::I64, iter_slot, 0);
        let car_val = Self::call_helper(ctx, "jit_car", current)?;

        // Store in loop variable
        ctx.builder.ins().stack_store(car_val, var_slot, 0);

        // Compile body
        let _body_result = Self::compile_expr_block(ctx, body)?;

        // Get cdr (advance iterator)
        let current_for_cdr = ctx.builder.ins().stack_load(types::I64, iter_slot, 0);
        let cdr_val = Self::call_helper(ctx, "jit_cdr", current_for_cdr)?;
        ctx.builder.ins().stack_store(cdr_val, iter_slot, 0);

        // Jump back to header
        ctx.builder.ins().jump(header_block, &[]);

        // Seal header after back-edge
        ctx.builder.seal_block(header_block);

        // 7. Exit block
        ctx.builder.switch_to_block(exit_block);
        ctx.builder.seal_block(exit_block);

        ctx.scope_manager.pop_scope().map_err(|e| e.to_string())?;

        Ok(IrValue::I64(ctx.builder.ins().iconst(types::I64, 0)))
    }

    /// Call a runtime helper function (jit_is_nil, jit_car, jit_cdr)
    fn call_helper(
        ctx: &mut CompileContext,
        name: &str,
        arg: cranelift::prelude::Value,
    ) -> Result<cranelift::prelude::Value, String> {
        use cranelift_module::Linkage;

        // Create signature: fn(i64) -> i64
        let mut sig = ctx.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));

        // Declare the imported function
        let func_id = ctx
            .module
            .declare_function(name, Linkage::Import, &sig)
            .map_err(|e| format!("Failed to declare helper '{}': {:?}", name, e))?;

        // Get function reference for this function
        let func_ref = ctx.module.declare_func_in_func(func_id, ctx.builder.func);

        // Emit the call
        let call = ctx.builder.ins().call(func_ref, &[arg]);

        // Return the result
        Ok(ctx.builder.inst_results(call)[0])
    }

    /// Try to compile a While loop expression
    /// (while cond body) - executes body repeatedly while cond is truthy, returns nil
    fn try_compile_while(
        ctx: &mut CompileContext,
        cond: &Expr,
        body: &Expr,
    ) -> Result<IrValue, String> {
        // Create blocks: header (check condition), body_block (execute), exit
        let header_block = ctx.builder.create_block();
        let body_block = ctx.builder.create_block();
        let exit_block = ctx.builder.create_block();

        // Jump to header to start loop
        ctx.builder.ins().jump(header_block, &[]);

        // Header block: evaluate condition and branch
        ctx.builder.switch_to_block(header_block);
        // Don't seal yet - we'll add predecessors from body_block

        let cond_val = Self::compile_expr_block(ctx, cond)?;
        let cond_i64 = match cond_val {
            IrValue::I64(v) => v,
            _ => return Err("While condition must evaluate to I64".to_string()),
        };

        let zero = ctx.builder.ins().iconst(types::I64, 0);
        let is_true = ctx.builder.ins().icmp(IntCC::NotEqual, cond_i64, zero);
        ctx.builder
            .ins()
            .brif(is_true, body_block, &[], exit_block, &[]);

        // Body block: execute body and jump back to header
        ctx.builder.switch_to_block(body_block);
        ctx.builder.seal_block(body_block);

        let _body_val = Self::compile_expr_block(ctx, body)?;
        // Note: we discard the body value (while loops return nil)
        ctx.builder.ins().jump(header_block, &[]);

        // Now seal header after we've added the back-edge from body
        ctx.builder.seal_block(header_block);

        // Exit block: return nil
        ctx.builder.switch_to_block(exit_block);
        ctx.builder.seal_block(exit_block);

        let nil_val = ctx.builder.ins().iconst(types::I64, 0);
        Ok(IrValue::I64(nil_val))
    }

    /// Try to compile an And expression with shortcircuiting
    /// (and expr1 expr2 expr3 ...) => returns first falsy value or last value
    fn try_compile_and(ctx: &mut CompileContext, exprs: &[Expr]) -> Result<IrValue, String> {
        if exprs.is_empty() {
            return Ok(IrValue::I64(ctx.builder.ins().iconst(types::I64, 1))); // (and) => true
        }

        if exprs.len() == 1 {
            return Self::compile_expr_block(ctx, &exprs[0]);
        }

        // For multi-argument and, we need control flow for proper short-circuiting
        // Create blocks: one for each expression evaluation + one for the final result
        let mut eval_blocks = Vec::new();
        let mut phi_values = Vec::new();

        for _ in 0..exprs.len() {
            eval_blocks.push(ctx.builder.create_block());
        }
        let end_block = ctx.builder.create_block();

        let zero = ctx.builder.ins().iconst(types::I64, 0);

        // Start with first expression
        let first_val = Self::compile_expr_block(ctx, &exprs[0])?;
        let first_i64 = match first_val {
            IrValue::I64(v) => v,
            _ => return Err("And on non-I64 values not supported".to_string()),
        };
        phi_values.push(first_i64);

        // Check if first is false (equal to 0), if so jump to end with that value
        // Otherwise continue to next expression
        let is_false = ctx.builder.ins().icmp(IntCC::Equal, first_i64, zero);
        if exprs.len() > 1 {
            ctx.builder
                .ins()
                .brif(is_false, end_block, &[first_i64], eval_blocks[1], &[]);
        } else {
            ctx.builder.ins().jump(end_block, &[first_i64]);
        }

        // Evaluate remaining expressions
        for i in 1..exprs.len() {
            ctx.builder.switch_to_block(eval_blocks[i]);
            ctx.builder.seal_block(eval_blocks[i]);

            let val = Self::compile_expr_block(ctx, &exprs[i])?;
            let val_i64 = match val {
                IrValue::I64(v) => v,
                _ => return Err("And on non-I64 values not supported".to_string()),
            };
            phi_values.push(val_i64);

            // If this is not the last expression, check if value is false
            if i < exprs.len() - 1 {
                let is_false = ctx.builder.ins().icmp(IntCC::Equal, val_i64, zero);
                if i + 1 < eval_blocks.len() {
                    ctx.builder.ins().brif(
                        is_false,
                        end_block,
                        &[val_i64],
                        eval_blocks[i + 1],
                        &[],
                    );
                } else {
                    ctx.builder.ins().jump(end_block, &[val_i64]);
                }
            } else {
                // Last expression - jump to end with its value
                ctx.builder.ins().jump(end_block, &[val_i64]);
            }
        }

        ctx.builder.switch_to_block(end_block);
        ctx.builder.seal_block(end_block);
        let param = ctx.builder.block_params(end_block)[0];

        Ok(IrValue::I64(param))
    }

    /// Try to compile an Or expression with shortcircuiting
    /// (or expr1 expr2 expr3 ...) => returns first truthy value or last value
    fn try_compile_or(ctx: &mut CompileContext, exprs: &[Expr]) -> Result<IrValue, String> {
        if exprs.is_empty() {
            return Ok(IrValue::I64(ctx.builder.ins().iconst(types::I64, 0))); // (or) => false
        }

        if exprs.len() == 1 {
            return Self::compile_expr_block(ctx, &exprs[0]);
        }

        // For multi-argument or, we need control flow for proper short-circuiting
        let mut eval_blocks = Vec::new();
        let mut phi_values = Vec::new();

        for _ in 0..exprs.len() {
            eval_blocks.push(ctx.builder.create_block());
        }
        let end_block = ctx.builder.create_block();

        let zero = ctx.builder.ins().iconst(types::I64, 0);

        // Start with first expression
        let first_val = Self::compile_expr_block(ctx, &exprs[0])?;
        let first_i64 = match first_val {
            IrValue::I64(v) => v,
            _ => return Err("Or on non-I64 values not supported".to_string()),
        };
        phi_values.push(first_i64);

        // Check if first is true (not equal to 0), if so jump to end with that value
        // Otherwise continue to next expression
        let is_true = ctx.builder.ins().icmp(IntCC::NotEqual, first_i64, zero);
        if exprs.len() > 1 {
            ctx.builder
                .ins()
                .brif(is_true, end_block, &[first_i64], eval_blocks[1], &[]);
        } else {
            ctx.builder.ins().jump(end_block, &[first_i64]);
        }

        // Evaluate remaining expressions
        for i in 1..exprs.len() {
            ctx.builder.switch_to_block(eval_blocks[i]);
            ctx.builder.seal_block(eval_blocks[i]);

            let val = Self::compile_expr_block(ctx, &exprs[i])?;
            let val_i64 = match val {
                IrValue::I64(v) => v,
                _ => return Err("Or on non-I64 values not supported".to_string()),
            };
            phi_values.push(val_i64);

            // If this is not the last expression, check if value is true
            if i < exprs.len() - 1 {
                let is_true = ctx.builder.ins().icmp(IntCC::NotEqual, val_i64, zero);
                if i + 1 < eval_blocks.len() {
                    ctx.builder
                        .ins()
                        .brif(is_true, end_block, &[val_i64], eval_blocks[i + 1], &[]);
                } else {
                    ctx.builder.ins().jump(end_block, &[val_i64]);
                }
            } else {
                // Last expression - jump to end with its value
                ctx.builder.ins().jump(end_block, &[val_i64]);
            }
        }

        ctx.builder.switch_to_block(end_block);
        ctx.builder.seal_block(end_block);
        let param = ctx.builder.block_params(end_block)[0];

        Ok(IrValue::I64(param))
    }

    /// Try to compile a unary operation (like empty?, abs)
    fn try_compile_unary_op(
        ctx: &mut CompileContext,
        func: &Expr,
        args: &[Expr],
    ) -> Result<IrValue, String> {
        // Extract operator name from function
        let op_name = match func {
            Expr::Literal(Value::Symbol(sym_id)) => ctx.symbols.name(*sym_id),
            _ => None,
        };

        let op_name = match op_name {
            Some(name) => name,
            None => return Err("Could not resolve operator name".to_string()),
        };

        // Compile the argument
        let arg = Self::compile_expr_block(ctx, &args[0])?;

        // Perform the unary operation
        match op_name {
            "empty?" => {
                // empty? on nil returns true, on cons returns false
                // This is essentially the same as (nil? x)
                match arg {
                    IrValue::I64(val) => {
                        // Check if value is nil (0)
                        let zero = ctx.builder.ins().iconst(types::I64, 0);
                        let result =
                            ctx.builder
                                .ins()
                                .icmp(cranelift::prelude::IntCC::Equal, val, zero);
                        Ok(IrValue::I64(result))
                    }
                    _ => Err("empty? on non-I64 not supported".to_string()),
                }
            }
            "abs" => {
                // Absolute value: if x < 0 then -x else x
                match arg {
                    IrValue::I64(val) => {
                        let zero = ctx.builder.ins().iconst(types::I64, 0);
                        let is_negative = ctx.builder.ins().icmp(IntCC::SignedLessThan, val, zero);
                        let negated = ctx.builder.ins().ineg(val);
                        let result = ctx.builder.ins().select(is_negative, negated, val);
                        Ok(IrValue::I64(result))
                    }
                    _ => Err("abs on non-I64 not supported".to_string()),
                }
            }
            "nil?" => {
                // nil? is same as empty?
                match arg {
                    IrValue::I64(val) => {
                        let zero = ctx.builder.ins().iconst(types::I64, 0);
                        let result =
                            ctx.builder
                                .ins()
                                .icmp(cranelift::prelude::IntCC::Equal, val, zero);
                        Ok(IrValue::I64(result))
                    }
                    _ => Err("nil? on non-I64 not supported".to_string()),
                }
            }
            "not" => {
                // Logical NOT
                match arg {
                    IrValue::I64(val) => {
                        let zero = ctx.builder.ins().iconst(types::I64, 0);
                        let result =
                            ctx.builder
                                .ins()
                                .icmp(cranelift::prelude::IntCC::Equal, val, zero);
                        Ok(IrValue::I64(result))
                    }
                    _ => Err("not on non-I64 not supported".to_string()),
                }
            }
            _ => Err(format!("Unknown unary operator: {}", op_name)),
        }
    }

    /// Try to compile a binary operation
    /// Only works for operations on literal integers
    fn try_compile_binop(
        ctx: &mut CompileContext,
        func: &Expr,
        args: &[Expr],
    ) -> Result<IrValue, String> {
        // Extract operator name from function
        let op_name = match func {
            Expr::Literal(Value::Symbol(sym_id)) => ctx.symbols.name(*sym_id),
            _ => None,
        };

        let op_name = match op_name {
            Some(name) => name,
            None => return Err("Could not resolve operator name".to_string()),
        };

        // Compile the arguments
        let left = Self::compile_expr_block(ctx, &args[0])?;
        let right = Self::compile_expr_block(ctx, &args[1])?;

        // Perform the binary operation
        match (left, right) {
            (IrValue::I64(l), IrValue::I64(r)) => {
                let result = match op_name {
                    "+" => IrEmitter::emit_add_int(ctx.builder, l, r),
                    "-" => IrEmitter::emit_sub_int(ctx.builder, l, r),
                    "*" => IrEmitter::emit_mul_int(ctx.builder, l, r),
                    "/" => IrEmitter::emit_sdiv_int(ctx.builder, l, r),
                    "=" => IrEmitter::emit_eq_int(ctx.builder, l, r),
                    "<" => IrEmitter::emit_lt_int(ctx.builder, l, r),
                    ">" => IrEmitter::emit_gt_int(ctx.builder, l, r),
                    "<=" => {
                        // <= is (not (>))
                        let gt_result = IrEmitter::emit_gt_int(ctx.builder, l, r);
                        let zero = ctx.builder.ins().iconst(types::I64, 0);
                        ctx.builder.ins().icmp(IntCC::Equal, gt_result, zero)
                    }
                    ">=" => {
                        // >= is (not (<))
                        let lt_result = IrEmitter::emit_lt_int(ctx.builder, l, r);
                        let zero = ctx.builder.ins().iconst(types::I64, 0);
                        ctx.builder.ins().icmp(IntCC::Equal, lt_result, zero)
                    }
                    "!=" | "neq" => {
                        // != is (not (=))
                        let eq_result = IrEmitter::emit_eq_int(ctx.builder, l, r);
                        let zero = ctx.builder.ins().iconst(types::I64, 0);
                        ctx.builder.ins().icmp(IntCC::Equal, eq_result, zero)
                    }
                    _ => return Err(format!("Unknown binary operator: {}", op_name)),
                };
                Ok(IrValue::I64(result))
            }
            _ => Err("Type mismatch in binary operation".to_string()),
        }
    }

    /// Compile a literal value to CLIF IR
    fn compile_literal(ctx: &mut CompileContext, val: &Value) -> Result<IrValue, String> {
        match val {
            Value::Nil => {
                // Nil is encoded as 0i64
                let ir_val = IrEmitter::emit_nil(ctx.builder);
                Ok(IrValue::I64(ir_val))
            }
            Value::Bool(b) => {
                // Bool is encoded as 0 (false) or 1 (true)
                let ir_val = IrEmitter::emit_bool(ctx.builder, *b);
                Ok(IrValue::I64(ir_val))
            }
            Value::Int(i) => {
                // Int is emitted directly
                let ir_val = IrEmitter::emit_int(ctx.builder, *i);
                Ok(IrValue::I64(ir_val))
            }
            Value::Float(f) => {
                // Float is emitted as f64
                let ir_val = IrEmitter::emit_float(ctx.builder, *f);
                Ok(IrValue::F64(ir_val))
            }
            _ => Err(format!(
                "Cannot compile non-primitive literal in JIT: {:?}",
                val
            )),
        }
    }

    /// Compile a begin (sequence) expression
    fn compile_begin(ctx: &mut CompileContext, exprs: &[Expr]) -> Result<IrValue, String> {
        let mut result = IrValue::I64(IrEmitter::emit_nil(ctx.builder));
        for expr in exprs {
            result = Self::compile_expr_block(ctx, expr)?;
        }
        Ok(result)
    }

    /// Compile an if expression with proper conditional branching
    fn compile_if(
        ctx: &mut CompileContext,
        cond: &Expr,
        then_expr: &Expr,
        else_expr: &Expr,
    ) -> Result<IrValue, String> {
        // Compile the condition expression
        let cond_val = Self::compile_expr_block(ctx, cond)?;

        // Extract i64 value from condition (floats would need conversion)
        let cond_i64 = match cond_val {
            IrValue::I64(v) => v,
            IrValue::F64(_v) => {
                // For now, treat any f64 as truthy (non-zero)
                // TODO: Proper float-to-int conversion
                return Err("Float conditions not yet supported".to_string());
            }
        };

        // Create branch blocks
        let (then_block, else_block, join_block) = BranchManager::create_if_blocks(ctx.builder);

        // Emit the conditional branch
        BranchManager::emit_if_cond(ctx.builder, cond_i64, then_block, else_block);

        // Compile then branch
        ctx.builder.switch_to_block(then_block);
        ctx.builder.seal_block(then_block);
        let then_val = Self::compile_expr_block(ctx, then_expr)?;
        let then_i64 = Self::ir_value_to_i64(ctx.builder, then_val)?;
        BranchManager::jump_to_join(ctx.builder, join_block, then_i64);

        // Compile else branch
        ctx.builder.switch_to_block(else_block);
        ctx.builder.seal_block(else_block);
        let else_val = Self::compile_expr_block(ctx, else_expr)?;
        let else_i64 = Self::ir_value_to_i64(ctx.builder, else_val)?;
        BranchManager::jump_to_join(ctx.builder, join_block, else_i64);

        // Set up join block and get the result value
        BranchManager::setup_join_block_for_value(join_block, ctx.builder);
        ctx.builder.switch_to_block(join_block);
        ctx.builder.seal_block(join_block);
        let result_i64 = BranchManager::get_join_value(ctx.builder, join_block);

        Ok(IrValue::I64(result_i64))
    }

    /// Convert IrValue to i64 for control flow operations
    fn ir_value_to_i64(
        builder: &mut FunctionBuilder,
        val: IrValue,
    ) -> Result<cranelift::prelude::Value, String> {
        match val {
            IrValue::I64(v) => Ok(v),
            IrValue::F64(_v) => {
                // For now, return a placeholder
                // TODO: Proper float-to-i64 encoding
                Ok(builder.ins().iconst(types::I64, 0))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::context::JITContext;
    use super::*;
    use cranelift::codegen::ir;

    #[test]
    fn test_compile_expr_block_literal() {
        use crate::symbol::SymbolTable;

        let mut jit_ctx = JITContext::new().expect("Failed to create JIT context");
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = ir::Function::new();
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.returns.push(AbiParam::new(types::I64));
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let symbols = SymbolTable::new();
        let mut ctx = CompileContext::new(&mut builder, &symbols, &mut jit_ctx.module);
        let result = ExprCompiler::compile_expr_block(&mut ctx, &Expr::Literal(Value::Int(42)));
        assert!(
            result.is_ok(),
            "Failed to compile integer literal: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_compile_var_reference() {
        use crate::symbol::SymbolTable;

        let mut jit_ctx = JITContext::new().expect("Failed to create JIT context");
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = ir::Function::new();
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.returns.push(AbiParam::new(types::I64));
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let symbols = SymbolTable::new();
        let mut ctx = CompileContext::new(&mut builder, &symbols, &mut jit_ctx.module);

        // Allocate a variable at depth 0, index 0
        let (slot, _) = ctx
            .stack_allocator
            .allocate(ctx.builder, 0, 0, SlotType::I64)
            .unwrap();
        let const_val = ctx.builder.ins().iconst(types::I64, 42);
        ctx.builder.ins().stack_store(const_val, slot, 0);

        // Now try to reference it
        let result = ExprCompiler::compile_var(&mut ctx, crate::value::SymbolId(0), 0, 0);
        assert!(
            result.is_ok(),
            "Failed to compile variable reference: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_compile_set() {
        use crate::symbol::SymbolTable;

        let mut jit_ctx = JITContext::new().expect("Failed to create JIT context");
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = ir::Function::new();
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.returns.push(AbiParam::new(types::I64));
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let symbols = SymbolTable::new();
        let mut ctx = CompileContext::new(&mut builder, &symbols, &mut jit_ctx.module);

        // Allocate a variable at depth 0, index 0
        let (slot, _) = ctx
            .stack_allocator
            .allocate(ctx.builder, 0, 0, SlotType::I64)
            .unwrap();
        let const_val = ctx.builder.ins().iconst(types::I64, 10);
        ctx.builder.ins().stack_store(const_val, slot, 0);

        // Now try to set it
        let result = ExprCompiler::compile_set(
            &mut ctx,
            crate::value::SymbolId(0),
            0,
            0,
            &Expr::Literal(Value::Int(99)),
        );
        assert!(result.is_ok(), "Failed to compile set!: {:?}", result.err());
    }

    #[test]
    fn test_compile_expr_block_begin() {
        use crate::symbol::SymbolTable;

        let mut jit_ctx = JITContext::new().expect("Failed to create JIT context");
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = ir::Function::new();
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.returns.push(AbiParam::new(types::I64));
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let symbols = SymbolTable::new();
        let mut ctx = CompileContext::new(&mut builder, &symbols, &mut jit_ctx.module);
        let result = ExprCompiler::compile_expr_block(
            &mut ctx,
            &Expr::Begin(vec![
                Expr::Literal(Value::Int(1)),
                Expr::Literal(Value::Int(2)),
            ]),
        );
        assert!(
            result.is_ok(),
            "Failed to compile begin expression: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_compile_while_loop() {
        use crate::symbol::SymbolTable;

        let mut jit_ctx = JITContext::new().expect("Failed to create JIT context");
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = ir::Function::new();
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.returns.push(AbiParam::new(types::I64));
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let mut symbols = SymbolTable::new();
        let lt_sym = symbols.intern("<");

        // Test: (while (< 0 10) 42)
        // A while loop with a condition and a body
        let mut ctx = CompileContext::new(&mut builder, &symbols, &mut jit_ctx.module);
        let result = ExprCompiler::compile_expr_block(
            &mut ctx,
            &Expr::While {
                cond: Box::new(Expr::Call {
                    func: Box::new(Expr::Literal(Value::Symbol(lt_sym))),
                    args: vec![Expr::Literal(Value::Int(0)), Expr::Literal(Value::Int(10))],
                    tail: false,
                }),
                body: Box::new(Expr::Literal(Value::Int(42))),
            },
        );
        assert!(
            result.is_ok(),
            "Failed to compile while loop: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_compile_for_loop_empty_list() {
        use crate::symbol::SymbolTable;

        let mut jit_ctx = JITContext::new().expect("Failed to create JIT context");
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = ir::Function::new();
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.returns.push(AbiParam::new(types::I64));
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let mut symbols = SymbolTable::new();
        let item_sym = symbols.intern("item");

        // Test: (for item nil item)
        // For loop over empty list should compile and return nil
        let mut ctx = CompileContext::new(&mut builder, &symbols, &mut jit_ctx.module);
        let result = ExprCompiler::compile_expr_block(
            &mut ctx,
            &Expr::For {
                var: item_sym,
                iter: Box::new(Expr::Literal(Value::Nil)),
                body: Box::new(Expr::Literal(Value::Int(0))),
            },
        );
        // Should succeed for empty list
        assert!(
            result.is_ok(),
            "Failed to compile for loop with empty list: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_compile_for_loop_literal_cons() {
        use crate::symbol::SymbolTable;
        use crate::value::cons;

        let mut jit_ctx = JITContext::new().expect("Failed to create JIT context");
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = ir::Function::new();
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.returns.push(AbiParam::new(types::I64));
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let mut symbols = SymbolTable::new();
        let item_sym = symbols.intern("item");

        // Test: (for item (list 1 2 3) 42)
        // For loop over literal cons list should compile
        let literal_list = cons(
            Value::Int(1),
            cons(Value::Int(2), cons(Value::Int(3), Value::Nil)),
        );

        let mut ctx = CompileContext::new(&mut builder, &symbols, &mut jit_ctx.module);
        let result = ExprCompiler::compile_expr_block(
            &mut ctx,
            &Expr::For {
                var: item_sym,
                iter: Box::new(Expr::Literal(literal_list)),
                body: Box::new(Expr::Literal(Value::Int(42))),
            },
        );
        // Should succeed for literal list
        assert!(
            result.is_ok(),
            "Failed to compile for loop with literal cons: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_compile_for_loop_computed_iterable() {
        use crate::symbol::SymbolTable;

        let mut jit_ctx = JITContext::new().expect("Failed to create JIT context");
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = ir::Function::new();
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.returns.push(AbiParam::new(types::I64));
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let mut symbols = SymbolTable::new();
        let item_sym = symbols.intern("item");

        // Test: (for item (begin list) item)
        // For loops over computed iterables (like function calls) should now work
        // We use a begin expression to simulate a computed iterable
        let mut ctx = CompileContext::new(&mut builder, &symbols, &mut jit_ctx.module);
        let result = ExprCompiler::compile_expr_block(
            &mut ctx,
            &Expr::For {
                var: item_sym,
                iter: Box::new(Expr::Begin(vec![Expr::Literal(Value::Nil)])), // Computed iterable
                body: Box::new(Expr::Literal(Value::Int(0))),
            },
        );
        // Should now succeed for computed iterables
        assert!(
            result.is_ok(),
            "For loop should compile for computed iterables: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_compile_let_with_var_access() {
        use crate::symbol::SymbolTable;

        let mut jit_ctx = JITContext::new().expect("Failed to create JIT context");
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = ir::Function::new();
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.returns.push(AbiParam::new(types::I64));
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let mut symbols = SymbolTable::new();
        let x_sym = symbols.intern("x");

        // Test: (let ((x 42)) x)
        let mut ctx = CompileContext::new(&mut builder, &symbols, &mut jit_ctx.module);
        let result = ExprCompiler::compile_expr_block(
            &mut ctx,
            &Expr::Let {
                bindings: vec![(x_sym, Expr::Literal(Value::Int(42)))],
                body: Box::new(Expr::Var(x_sym, 1, 0)),
            },
        );
        assert!(
            result.is_ok(),
            "Failed to compile let with var access: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_compile_for_loop_with_variable() {
        use crate::symbol::SymbolTable;
        use crate::value::cons;

        let mut jit_ctx = JITContext::new().expect("Failed to create JIT context");
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = ir::Function::new();
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.returns.push(AbiParam::new(types::I64));
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let mut symbols = SymbolTable::new();
        let x_sym = symbols.intern("x");

        // Test: (for x '(1 2 3) x)
        let literal_list = cons(
            Value::Int(1),
            cons(Value::Int(2), cons(Value::Int(3), Value::Nil)),
        );

        let mut ctx = CompileContext::new(&mut builder, &symbols, &mut jit_ctx.module);
        let result = ExprCompiler::compile_expr_block(
            &mut ctx,
            &Expr::For {
                var: x_sym,
                iter: Box::new(Expr::Literal(literal_list)),
                body: Box::new(Expr::Var(x_sym, 1, 0)),
            },
        );
        assert!(
            result.is_ok(),
            "Failed to compile for loop with variable: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_compile_let_with_float() {
        use crate::symbol::SymbolTable;

        let mut jit_ctx = JITContext::new().expect("Failed to create JIT context");
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = ir::Function::new();
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.returns.push(AbiParam::new(types::I64));
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let mut symbols = SymbolTable::new();
        let x_sym = symbols.intern("x");

        // Test: (let ((x 3.14159265358979)) x)
        let mut ctx = CompileContext::new(&mut builder, &symbols, &mut jit_ctx.module);
        let result = ExprCompiler::compile_expr_block(
            &mut ctx,
            &Expr::Let {
                bindings: vec![(x_sym, Expr::Literal(Value::Float(std::f64::consts::PI)))],
                body: Box::new(Expr::Var(x_sym, 1, 0)),
            },
        );
        assert!(
            result.is_ok(),
            "Failed to compile let with float: {:?}",
            result.err()
        );
        assert!(matches!(result.unwrap(), IrValue::F64(_)));
    }

    #[test]
    fn test_compile_for_loop_float_elements() {
        use crate::symbol::SymbolTable;
        use crate::value::cons;

        let mut jit_ctx = JITContext::new().expect("Failed to create JIT context");
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = ir::Function::new();
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.returns.push(AbiParam::new(types::I64));
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let mut symbols = SymbolTable::new();
        let x_sym = symbols.intern("x");

        // Test: (for x '(1.0 2.0 3.0) x)
        let literal_list = cons(
            Value::Float(1.0),
            cons(Value::Float(2.0), cons(Value::Float(3.0), Value::Nil)),
        );

        let mut ctx = CompileContext::new(&mut builder, &symbols, &mut jit_ctx.module);
        let result = ExprCompiler::compile_expr_block(
            &mut ctx,
            &Expr::For {
                var: x_sym,
                iter: Box::new(Expr::Literal(literal_list)),
                body: Box::new(Expr::Var(x_sym, 1, 0)),
            },
        );
        assert!(
            result.is_ok(),
            "Failed to compile for loop with floats: {:?}",
            result.err()
        );
    }
}
