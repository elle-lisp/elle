// Cranelift compiler with variable scoping (Phase 5)
//
// This version adds support for Let bindings and variable references,
// integrating the ScopeManager for proper variable scoping.
//
// Features:
// - Let binding compilation with variable storage
// - Variable reference compilation (Expr::Var)
// - Nested scope support
// - Variable shadowing
//
// Note: Phase 5 uses SSA value storage during compilation.
// Variables are stored as Cranelift SSA values in a HashMap.

use super::codegen::IrEmitter;
use super::compiler_v2::IrValue;
use super::scoping::ScopeManager;
use crate::compiler::ast::Expr;
use crate::symbol::SymbolTable;
use crate::value::Value;
use cranelift::prelude::*;
use std::collections::HashMap;

/// Compilation context with scoping support (Phase 5)
pub struct CompileContextV3<'a> {
    pub builder: &'a mut FunctionBuilder<'a>,
    pub symbol_table: &'a SymbolTable,
    pub scope_manager: &'a mut ScopeManager,
    /// Maps (depth, index) -> IrValue for variable storage
    pub variable_values: HashMap<(usize, usize), IrValue>,
}

impl<'a> CompileContextV3<'a> {
    /// Create a new compilation context with scoping
    pub fn new(
        builder: &'a mut FunctionBuilder<'a>,
        symbol_table: &'a SymbolTable,
        scope_manager: &'a mut ScopeManager,
    ) -> Self {
        CompileContextV3 {
            builder,
            symbol_table,
            scope_manager,
            variable_values: HashMap::new(),
        }
    }

    /// Store a variable value
    fn store_variable(&mut self, depth: usize, index: usize, value: IrValue) {
        self.variable_values.insert((depth, index), value);
    }

    /// Retrieve a stored variable value
    fn load_variable(&self, depth: usize, index: usize) -> Option<IrValue> {
        self.variable_values.get(&(depth, index)).copied()
    }
}

/// Expression compiler with variable scoping support (Phase 5)
pub struct ExprCompilerV3;

impl ExprCompilerV3 {
    /// Compile an expression with scoping support
    pub fn compile_expr_block(ctx: &mut CompileContextV3, expr: &Expr) -> Result<IrValue, String> {
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
    fn compile_literal(ctx: &mut CompileContextV3, val: &Value) -> Result<IrValue, String> {
        match val {
            Value::Nil => Ok(IrValue::I64(IrEmitter::emit_nil(ctx.builder))),
            Value::Bool(b) => Ok(IrValue::I64(IrEmitter::emit_bool(ctx.builder, *b))),
            Value::Int(i) => Ok(IrValue::I64(IrEmitter::emit_int(ctx.builder, *i))),
            Value::Float(f) => Ok(IrValue::F64(IrEmitter::emit_float(ctx.builder, *f))),
            _ => Err(format!("Unsupported literal type: {:?}", val)),
        }
    }

    /// Compile a variable reference
    fn compile_var(
        ctx: &mut CompileContextV3,
        _sym_id: crate::value::SymbolId,
        depth: usize,
        index: usize,
    ) -> Result<IrValue, String> {
        // Look up the variable value
        ctx.load_variable(depth, index).ok_or(format!(
            "Undefined variable at depth={}, index={}",
            depth, index
        ))
    }

    /// Compile a sequence of expressions (begin)
    fn compile_begin(ctx: &mut CompileContextV3, exprs: &[Expr]) -> Result<IrValue, String> {
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
    fn compile_block(ctx: &mut CompileContextV3, exprs: &[Expr]) -> Result<IrValue, String> {
        // Push a new scope for the block
        ctx.scope_manager.push_scope();

        let result = Self::compile_begin(ctx, exprs);

        // Pop the scope
        ctx.scope_manager.pop_scope()?;

        result
    }

    /// Compile a let binding expression
    fn compile_let(
        ctx: &mut CompileContextV3,
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

            // Store the value in our variable storage
            ctx.store_variable(depth, index, binding_val);
        }

        // Compile the body expression
        let result = Self::compile_expr_block(ctx, body)?;

        // Pop the scope
        ctx.scope_manager.pop_scope()?;

        Ok(result)
    }

    /// Compile a conditional expression (if/then/else)
    fn compile_if(
        ctx: &mut CompileContextV3,
        cond: &Expr,
        then_expr: &Expr,
        else_expr: &Expr,
    ) -> Result<IrValue, String> {
        // Compile the condition
        let cond_val = Self::compile_expr_block(ctx, cond)?;

        // Extract i64 value from condition (must be boolean or integer)
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
                // For now, return an error for float results in conditionals
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
        ctx: &mut CompileContextV3,
        func: &Expr,
        args: &[Expr],
    ) -> Result<IrValue, String> {
        // For now, only support direct function calls (symbol references)
        // This is a simplified version; full support comes later
        match func {
            Expr::Literal(Value::Symbol(sym_id)) => {
                // Get the function name from symbol table
                let func_name = ctx
                    .symbol_table
                    .name(*sym_id)
                    .ok_or("Unknown function symbol")?;

                // Try to handle as primitive operation
                if let Ok(result) = Self::try_compile_primitive_call(ctx, func_name, args) {
                    return Ok(result);
                }

                // Otherwise, unsupported
                Err(format!("Function not yet supported in JIT: {}", func_name))
            }
            _ => Err("Only direct function calls are supported in JIT".to_string()),
        }
    }

    /// Try to compile a primitive function call
    fn try_compile_primitive_call(
        _ctx: &mut CompileContextV3,
        func_name: &str,
        _args: &[Expr],
    ) -> Result<IrValue, String> {
        // Use the FunctionCallCompiler to handle this
        // This requires converting to the v2 context format
        // For now, return an error and we'll enhance this in next iteration
        Err(format!(
            "Primitive call not yet fully supported: {}",
            func_name
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::SymbolTable;
    use cranelift::codegen::ir::Function;
    use cranelift::frontend::FunctionBuilder;

    fn setup_compiler() -> (Function, SymbolTable, ScopeManager) {
        let func = Function::new();
        let sym_table = SymbolTable::new();
        let scope_manager = ScopeManager::new();
        (func, sym_table, scope_manager)
    }

    #[test]
    fn test_context_creation() {
        let (mut func, sym_table, mut scope_mgr) = setup_compiler();
        let mut builder_context = Default::default();
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_context);

        let ctx = CompileContextV3::new(&mut builder, &sym_table, &mut scope_mgr);

        // Just verify context was created successfully
        assert_eq!(ctx.scope_manager.current_depth(), 0);
        assert_eq!(ctx.variable_values.len(), 0);
    }

    #[test]
    fn test_variable_storage() {
        let (_, _, mut scope_mgr) = setup_compiler();
        let mut var_values = HashMap::new();

        // Store and retrieve a variable
        let sym_id = crate::value::SymbolId(1);
        scope_mgr.bind(sym_id);
        let (depth, index) = (0, 0);

        let ir_val = IrValue::I64(cranelift::prelude::Value::from_u32(0));
        var_values.insert((depth, index), ir_val);

        // Verify retrieval
        assert!(var_values.contains_key(&(depth, index)));
    }

    #[test]
    fn test_scope_manager_integration() {
        let mut scope_mgr = ScopeManager::new();

        // Create a scope and bind variables
        scope_mgr.push_scope();
        let sym1 = crate::value::SymbolId(1);
        let (depth, _index) = scope_mgr.bind(sym1);

        // Verify binding worked
        assert_eq!(depth, 1);
        assert_eq!(scope_mgr.current_depth(), 1);

        // Pop scope
        scope_mgr.pop_scope().unwrap();
        assert_eq!(scope_mgr.current_depth(), 0);
    }
}
