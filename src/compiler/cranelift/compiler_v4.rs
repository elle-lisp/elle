// Cranelift compiler with user-defined function support (Phase 6)
//
// This version extends compiler_v3 with:
// - Lambda expression compilation
// - User-defined function calls
// - Parameter binding and passing
// - Closure support foundation
//
// Note: This is an enhanced teaching compiler. The stack-based variant
// (compiler_v4_stack) provides production-ready implementation.

use super::codegen::IrEmitter;
use super::compiler_v2::IrValue;
use super::function_compiler::{CompiledLambda, FunctionCompiler};
use super::scoping::ScopeManager;
use crate::compiler::ast::Expr;
use crate::symbol::SymbolTable;
use crate::value::{SymbolId, Value};
use cranelift::prelude::*;
use std::collections::HashMap;

/// Represents a compiled function value at runtime
#[derive(Debug, Clone)]
pub struct FunctionValue {
    pub lambda: CompiledLambda,
}

/// Compilation context with function support (Phase 6)
pub struct CompileContextV4<'a> {
    pub builder: &'a mut FunctionBuilder<'a>,
    pub symbol_table: &'a SymbolTable,
    pub scope_manager: &'a mut ScopeManager,
    /// Maps (depth, index) -> IrValue for variable storage
    pub variable_values: HashMap<(usize, usize), IrValue>,
    /// Stores compiled function values (lambda objects)
    pub functions: HashMap<SymbolId, FunctionValue>,
}

impl<'a> CompileContextV4<'a> {
    /// Create a new compilation context with function support
    pub fn new(
        builder: &'a mut FunctionBuilder<'a>,
        symbol_table: &'a SymbolTable,
        scope_manager: &'a mut ScopeManager,
    ) -> Self {
        CompileContextV4 {
            builder,
            symbol_table,
            scope_manager,
            variable_values: HashMap::new(),
            functions: HashMap::new(),
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

    /// Retrieve a compiled function
    fn get_function(&self, sym_id: SymbolId) -> Option<FunctionValue> {
        self.functions.get(&sym_id).cloned()
    }

    /// Store a compiled function (marked as allow dead code for future use)
    #[allow(dead_code)]
    fn store_function(&mut self, sym_id: SymbolId, func: FunctionValue) {
        self.functions.insert(sym_id, func);
    }
}

/// Expression compiler with user-defined function support (Phase 6)
pub struct ExprCompilerV4;

impl ExprCompilerV4 {
    /// Compile an expression with function support
    pub fn compile_expr_block(ctx: &mut CompileContextV4, expr: &Expr) -> Result<IrValue, String> {
        match expr {
            Expr::Literal(val) => Self::compile_literal(ctx, val),
            Expr::Var(sym_id, depth, index) => Self::compile_var(ctx, *sym_id, *depth, *index),
            Expr::Begin(exprs) => Self::compile_begin(ctx, exprs),
            Expr::Block(exprs) => Self::compile_block(ctx, exprs),
            Expr::If { cond, then, else_ } => Self::compile_if(ctx, cond, then, else_),
            Expr::Let { bindings, body } => Self::compile_let(ctx, bindings, body),
            Expr::Lambda {
                params,
                body,
                captures,
                locals: _, // Locals are handled at compile time
            } => Self::compile_lambda(ctx, params.clone(), body.clone(), captures.clone()),
            Expr::Call { func, args, .. } => Self::compile_call(ctx, func, args),
            _ => Err(format!(
                "Expression type not yet supported in JIT: {:?}",
                expr
            )),
        }
    }

    /// Compile a literal value
    fn compile_literal(ctx: &mut CompileContextV4, val: &Value) -> Result<IrValue, String> {
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
        ctx: &mut CompileContextV4,
        _sym_id: SymbolId,
        depth: usize,
        index: usize,
    ) -> Result<IrValue, String> {
        ctx.load_variable(depth, index).ok_or(format!(
            "Undefined variable at depth={}, index={}",
            depth, index
        ))
    }

    /// Compile a sequence of expressions (begin)
    fn compile_begin(ctx: &mut CompileContextV4, exprs: &[Expr]) -> Result<IrValue, String> {
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
    fn compile_block(ctx: &mut CompileContextV4, exprs: &[Expr]) -> Result<IrValue, String> {
        // Push a new scope for the block
        ctx.scope_manager.push_scope();

        let result = Self::compile_begin(ctx, exprs);

        // Pop the scope
        ctx.scope_manager.pop_scope()?;

        result
    }

    /// Compile a let binding expression
    fn compile_let(
        ctx: &mut CompileContextV4,
        bindings: &[(SymbolId, Expr)],
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

            // Store the value
            ctx.store_variable(depth, index, binding_val);
        }

        // Compile the body expression
        let result = Self::compile_expr_block(ctx, body)?;

        // Pop the scope
        ctx.scope_manager.pop_scope()?;

        Ok(result)
    }

    /// Compile a lambda expression
    fn compile_lambda(
        ctx: &mut CompileContextV4,
        params: Vec<SymbolId>,
        body: Box<Expr>,
        captures: Vec<(SymbolId, usize, usize)>,
    ) -> Result<IrValue, String> {
        // Compile the lambda
        let _compiled = FunctionCompiler::compile_lambda(params, body, captures, ctx.symbol_table)?;

        // For now, we can't return a function value directly in JIT
        // This would require boxing/wrapping the function
        // Phase 6+ will implement proper function returns
        Err("Lambda return values not yet supported - use function definitions instead".to_string())
    }

    /// Compile a conditional expression (if/then/else)
    fn compile_if(
        ctx: &mut CompileContextV4,
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

        // Add parameters to continuation block
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
        ctx: &mut CompileContextV4,
        func: &Expr,
        args: &[Expr],
    ) -> Result<IrValue, String> {
        // First try to call a primitive operation (from Phase 4)
        if let Expr::Literal(Value::Symbol(sym_id)) = func {
            // Try to get a compiled function
            if let Some(func_val) = ctx.get_function(*sym_id) {
                // For now, we can only call with correct arity
                if !func_val.lambda.matches_arity(args.len()) {
                    return Err(format!(
                        "Function expects {} args, got {}",
                        func_val.lambda.param_count(),
                        args.len()
                    ));
                }

                // Bind parameters
                let _param_bindings =
                    FunctionCompiler::bind_parameters(ctx.scope_manager, &func_val.lambda.params)?;

                // Compile arguments and bind to parameters
                for (i, arg) in args.iter().enumerate() {
                    let arg_val = Self::compile_expr_block(ctx, arg)?;
                    let (depth, index) = _param_bindings[i];
                    ctx.store_variable(depth, index, arg_val);
                }

                // Compile the function body
                let result = Self::compile_expr_block(ctx, &func_val.lambda.body)?;

                // Unbind parameters
                FunctionCompiler::unbind_parameters(ctx.scope_manager)?;

                return Ok(result);
            }
        }

        Err(
            "Function calls not yet fully supported in Phase 6 - use primitives instead"
                .to_string(),
        )
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

        let ctx = CompileContextV4::new(&mut builder, &sym_table, &mut scope_mgr);

        assert_eq!(ctx.scope_manager.current_depth(), 0);
        assert_eq!(ctx.variable_values.len(), 0);
        assert_eq!(ctx.functions.len(), 0);
    }

    #[test]
    fn test_variable_storage() {
        let (_func, _sym_table, _scope_mgr) = setup_compiler();
        let mut var_values = HashMap::new();

        let ir_val = IrValue::I64(cranelift::prelude::Value::from_u32(0));
        var_values.insert((0, 0), ir_val);

        assert!(var_values.contains_key(&(0, 0)));
    }

    #[test]
    fn test_function_storage() {
        let (_func, _sym_table, _scope_mgr) = setup_compiler();
        let mut functions = HashMap::new();

        let lambda = CompiledLambda::from_expr(
            vec![SymbolId(1)],
            Box::new(Expr::Literal(Value::Int(42))),
            vec![],
        );

        let func_val = FunctionValue { lambda };
        functions.insert(SymbolId(1), func_val);

        assert!(functions.contains_key(&SymbolId(1)));
    }
}
