// Cranelift compiler with stack-based variable storage (Phase 5 Enhanced)
//
// This is an enhanced version of compiler_v3 that uses proper stack slot
// allocation instead of SSA value HashMap storage. This provides a more
// realistic compilation model where variables are stored on the stack.
//
// Features:
// - StackAllocator for proper stack slot management
// - Stack store/load operations for variable access
// - Let binding compilation with stack allocation
// - Variable reference compilation via stack loads

use super::codegen::IrEmitter;
use super::compiler_v2::IrValue;
use super::scoping::ScopeManager;
use super::stack_allocator::{SlotType, StackAllocator};
use crate::compiler::ast::Expr;
use crate::symbol::SymbolTable;
use crate::value::Value;
use cranelift::prelude::*;

/// Compilation context with stack-based variable storage (Phase 5 Enhanced)
pub struct CompileContextV3Stack<'a> {
    pub builder: &'a mut FunctionBuilder<'a>,
    pub symbol_table: &'a SymbolTable,
    pub scope_manager: &'a mut ScopeManager,
    pub stack_allocator: &'a mut StackAllocator,
}

impl<'a> CompileContextV3Stack<'a> {
    /// Create a new stack-based compilation context
    pub fn new(
        builder: &'a mut FunctionBuilder<'a>,
        symbol_table: &'a SymbolTable,
        scope_manager: &'a mut ScopeManager,
        stack_allocator: &'a mut StackAllocator,
    ) -> Self {
        CompileContextV3Stack {
            builder,
            symbol_table,
            scope_manager,
            stack_allocator,
        }
    }
}

/// Expression compiler with stack-based variable storage (Phase 5 Enhanced)
pub struct ExprCompilerV3Stack;

impl ExprCompilerV3Stack {
    /// Compile an expression with stack-based scoping support
    pub fn compile_expr_block(
        ctx: &mut CompileContextV3Stack,
        expr: &Expr,
    ) -> Result<IrValue, String> {
        match expr {
            Expr::Literal(val) => Self::compile_literal(ctx, val),
            Expr::Var(sym_id, depth, index) => Self::compile_var(ctx, *sym_id, *depth, *index),
            Expr::Begin(exprs) => Self::compile_begin(ctx, exprs),
            Expr::Block(exprs) => Self::compile_block(ctx, exprs),
            Expr::If { cond, then, else_ } => Self::compile_if(ctx, cond, then, else_),
            Expr::Let { bindings, body } => Self::compile_let(ctx, bindings, body),
            Expr::Call { func, args, .. } => Self::compile_call(ctx, func, args),
            _ => Err(format!(
                "Expression type not yet supported in JIT: {:?}",
                expr
            )),
        }
    }

    /// Compile a literal value
    fn compile_literal(ctx: &mut CompileContextV3Stack, val: &Value) -> Result<IrValue, String> {
        match val {
            Value::Nil => Ok(IrValue::I64(IrEmitter::emit_nil(ctx.builder))),
            Value::Bool(b) => Ok(IrValue::I64(IrEmitter::emit_bool(ctx.builder, *b))),
            Value::Int(i) => Ok(IrValue::I64(IrEmitter::emit_int(ctx.builder, *i))),
            Value::Float(f) => Ok(IrValue::F64(IrEmitter::emit_float(ctx.builder, *f))),
            _ => Err(format!("Unsupported literal type: {:?}", val)),
        }
    }

    /// Compile a variable reference - load from stack slot
    fn compile_var(
        ctx: &mut CompileContextV3Stack,
        _sym_id: crate::value::SymbolId,
        depth: usize,
        index: usize,
    ) -> Result<IrValue, String> {
        // Get the stack slot for this variable
        let slot = ctx.stack_allocator.get(depth, index).ok_or(format!(
            "Variable not allocated at depth={}, index={}",
            depth, index
        ))?;

        // Load the value from the stack slot (as i64)
        // For now, we store everything as i64 and bitcast as needed
        let value = ctx.builder.ins().stack_load(types::I64, slot, 0);
        Ok(IrValue::I64(value))
    }

    /// Compile a sequence of expressions (begin)
    fn compile_begin(ctx: &mut CompileContextV3Stack, exprs: &[Expr]) -> Result<IrValue, String> {
        if exprs.is_empty() {
            return Ok(IrValue::I64(IrEmitter::emit_nil(ctx.builder)));
        }

        let mut result = IrValue::I64(IrEmitter::emit_nil(ctx.builder));
        for expr in exprs {
            result = Self::compile_expr_block(ctx, expr)?;
        }
        Ok(result)
    }

    /// Compile a block expression (with its own scope)
    fn compile_block(ctx: &mut CompileContextV3Stack, exprs: &[Expr]) -> Result<IrValue, String> {
        // Push a new scope for the block
        ctx.scope_manager.push_scope();

        let result = Self::compile_begin(ctx, exprs);

        // Pop the scope
        ctx.scope_manager.pop_scope()?;

        result
    }

    /// Compile a let binding expression with stack allocation
    fn compile_let(
        ctx: &mut CompileContextV3Stack,
        bindings: &[(crate::value::SymbolId, Expr)],
        body: &Expr,
    ) -> Result<IrValue, String> {
        // Push a new scope for the let bindings
        ctx.scope_manager.push_scope();

        // Compile each binding
        for (sym_id, binding_expr) in bindings {
            // Compile the binding expression value
            let binding_val = Self::compile_expr_block(ctx, binding_expr)?;

            // Bind the symbol in the current scope
            let (depth, index) = ctx.scope_manager.bind(*sym_id);

            // Determine slot type from the compiled value
            let slot_type = match binding_val {
                IrValue::I64(_) => SlotType::I64,
                IrValue::F64(_) => SlotType::F64,
            };

            // Allocate a stack slot for this binding
            let (slot, _offset) =
                ctx.stack_allocator
                    .allocate(ctx.builder, depth, index, slot_type)?;

            // Store the value in the stack slot
            match binding_val {
                IrValue::I64(v) => {
                    ctx.builder.ins().stack_store(v, slot, 0);
                }
                IrValue::F64(v) => {
                    ctx.builder.ins().stack_store(v, slot, 0);
                }
            }
        }

        // Compile the body expression
        let result = Self::compile_expr_block(ctx, body)?;

        // Pop the scope
        ctx.scope_manager.pop_scope()?;

        Ok(result)
    }

    /// Compile a conditional expression (if/then/else)
    fn compile_if(
        ctx: &mut CompileContextV3Stack,
        cond: &Expr,
        then_expr: &Expr,
        else_expr: &Expr,
    ) -> Result<IrValue, String> {
        // Compile the condition
        let cond_val = Self::compile_expr_block(ctx, cond)?;

        // Extract i64 value from condition
        let cond_i64 = match cond_val {
            IrValue::I64(v) => v,
            IrValue::F64(_) => {
                return Err("Condition must be boolean or integer, not float".to_string())
            }
        };

        // Create blocks for then, else, and continuation
        let then_block = ctx.builder.create_block();
        let else_block = ctx.builder.create_block();
        let cont_block = ctx.builder.create_block();

        // Add parameters to continuation block for the result value
        ctx.builder.append_block_param(cont_block, types::I64);

        // Branch on condition (0 = false, non-zero = true)
        let zero = ctx.builder.ins().iconst(types::I64, 0);
        let cond_is_true = ctx.builder.ins().icmp(IntCC::NotEqual, cond_i64, zero);
        ctx.builder
            .ins()
            .brif(cond_is_true, then_block, &[], else_block, &[]);

        // Compile then branch
        ctx.builder.switch_to_block(then_block);
        let then_val = Self::compile_expr_block(ctx, then_expr)?;
        let then_i64 = match then_val {
            IrValue::I64(v) => v,
            IrValue::F64(_) => {
                return Err("Float values not yet supported in conditional results".to_string());
            }
        };
        ctx.builder.ins().jump(cont_block, &[then_i64]);

        // Compile else branch
        ctx.builder.switch_to_block(else_block);
        let else_val = Self::compile_expr_block(ctx, else_expr)?;
        let else_i64 = match else_val {
            IrValue::I64(v) => v,
            IrValue::F64(_) => {
                return Err("Float values not yet supported in conditional results".to_string());
            }
        };
        ctx.builder.ins().jump(cont_block, &[else_i64]);

        // Continue with the result
        ctx.builder.switch_to_block(cont_block);
        let result_val = ctx.builder.block_params(cont_block)[0];

        Ok(IrValue::I64(result_val))
    }

    /// Compile a function call
    fn compile_call(
        _ctx: &mut CompileContextV3Stack,
        func: &Expr,
        _args: &[Expr],
    ) -> Result<IrValue, String> {
        // For now, only support direct function calls
        match func {
            Expr::Literal(Value::Symbol(_sym_id)) => {
                Err("Function calls not yet fully supported in Phase 5 stack compiler".to_string())
            }
            _ => Err("Only direct function calls are supported in JIT".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::SymbolTable;
    use cranelift::codegen::ir::Function;
    use cranelift::frontend::FunctionBuilder;

    fn setup_compiler() -> (Function, SymbolTable, ScopeManager, StackAllocator) {
        let func = Function::new();
        let sym_table = SymbolTable::new();
        let scope_manager = ScopeManager::new();
        let stack_allocator = StackAllocator::new();
        (func, sym_table, scope_manager, stack_allocator)
    }

    #[test]
    fn test_stack_context_creation() {
        let (mut func, sym_table, mut scope_mgr, mut stack_alloc) = setup_compiler();
        let mut builder_context = Default::default();
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_context);

        let ctx =
            CompileContextV3Stack::new(&mut builder, &sym_table, &mut scope_mgr, &mut stack_alloc);

        // Just verify context was created successfully
        assert_eq!(ctx.scope_manager.current_depth(), 0);
        assert_eq!(ctx.stack_allocator.slot_count(), 0);
    }

    #[test]
    fn test_stack_allocator_integration() {
        let (_func, _sym_table, _scope_mgr, stack_alloc) = setup_compiler();

        // Verify allocator is empty at start
        assert_eq!(stack_alloc.slot_count(), 0);
        assert_eq!(stack_alloc.total_size(), 0);
        assert!(!stack_alloc.has(0, 0));
    }

    #[test]
    fn test_scope_and_stack_together() {
        let (_func, _sym_table, mut scope_mgr, stack_alloc) = setup_compiler();

        // Bind a variable
        let sym = crate::value::SymbolId(1);
        scope_mgr.bind(sym);

        // Verify variable is bound but not allocated yet
        assert!(scope_mgr.is_bound(sym));
        assert!(!stack_alloc.has(0, 0));
    }

    #[test]
    fn test_nested_scopes_with_stack() {
        let (_func, _sym_table, mut scope_mgr, stack_alloc) = setup_compiler();

        // Global scope
        let sym1 = crate::value::SymbolId(1);
        scope_mgr.bind(sym1);
        assert_eq!(stack_alloc.slot_count(), 0); // Not allocated yet

        // Nested scope
        scope_mgr.push_scope();
        let sym2 = crate::value::SymbolId(2);
        scope_mgr.bind(sym2);

        // Both should be accessible
        assert!(scope_mgr.lookup(sym1).is_some());
        assert!(scope_mgr.lookup(sym2).is_some());

        // Pop back
        scope_mgr.pop_scope().unwrap();
        assert!(scope_mgr.lookup(sym1).is_some());
        assert!(!scope_mgr.is_bound(sym2));
    }
}
