// User-defined function compilation (Phase 6)
//
// Compiles Lambda expressions into callable function objects.
// This module provides the foundation for:
// - Lambda expression compilation
// - Parameter binding as local variables
// - Function return values
// - Closure capture support (Phase 7+)

use super::scoping::ScopeManager;
use crate::compiler::ast::Expr;
use crate::symbol::SymbolTable;
use crate::value::{Arity, SymbolId};

/// Represents compiled lambda information during JIT
#[derive(Debug, Clone)]
pub struct CompiledLambda {
    /// Parameter symbols
    pub params: Vec<SymbolId>,
    /// Body expression
    pub body: Box<Expr>,
    /// Captured variables (depth, index)
    pub captures: Vec<(SymbolId, usize, usize)>,
    /// Expected arity
    pub arity: Arity,
}

impl CompiledLambda {
    /// Create a new compiled lambda from AST Lambda expression
    pub fn from_expr(
        params: Vec<SymbolId>,
        body: Box<Expr>,
        captures: Vec<(SymbolId, usize, usize)>,
    ) -> Self {
        let arity = Arity::Exact(params.len());
        CompiledLambda {
            params,
            body,
            captures,
            arity,
        }
    }

    /// Get the number of parameters
    pub fn param_count(&self) -> usize {
        self.params.len()
    }

    /// Get the number of captured variables
    pub fn capture_count(&self) -> usize {
        self.captures.len()
    }

    /// Check if this lambda matches the given arity
    pub fn matches_arity(&self, arg_count: usize) -> bool {
        self.arity.matches(arg_count)
    }
}

/// Compiler for user-defined functions (Phase 6)
pub struct FunctionCompiler;

impl FunctionCompiler {
    /// Compile a lambda expression
    /// Returns a CompiledLambda that can be invoked later
    pub fn compile_lambda(
        params: Vec<SymbolId>,
        body: Box<Expr>,
        captures: Vec<(SymbolId, usize, usize)>,
        symbol_table: &SymbolTable,
    ) -> Result<CompiledLambda, String> {
        // Validate parameter count
        if params.is_empty() {
            // Zero-argument functions are allowed
        }

        // Validate body expression is compilable
        Self::validate_expr(&body, symbol_table)?;

        // Create the compiled lambda
        Ok(CompiledLambda::from_expr(params, body, captures))
    }

    /// Validate that an expression can be compiled
    fn validate_expr(expr: &Expr, _symbol_table: &SymbolTable) -> Result<(), String> {
        match expr {
            // These are always valid
            Expr::Literal(_) => Ok(()),
            Expr::Var(_, _, _) => Ok(()),
            Expr::Begin(exprs) => {
                for e in exprs {
                    Self::validate_expr(e, _symbol_table)?;
                }
                Ok(())
            }
            Expr::Block(exprs) => {
                for e in exprs {
                    Self::validate_expr(e, _symbol_table)?;
                }
                Ok(())
            }
            Expr::If { cond, then, else_ } => {
                Self::validate_expr(cond, _symbol_table)?;
                Self::validate_expr(then, _symbol_table)?;
                Self::validate_expr(else_, _symbol_table)?;
                Ok(())
            }
            Expr::Let { bindings, body } => {
                for (_sym, expr) in bindings {
                    Self::validate_expr(expr, _symbol_table)?;
                }
                Self::validate_expr(body, _symbol_table)?;
                Ok(())
            }
            Expr::Lambda { .. } => {
                // Nested lambdas are allowed (Phase 6+)
                Ok(())
            }
            Expr::Call { func, args, .. } => {
                Self::validate_expr(func, _symbol_table)?;
                for arg in args {
                    Self::validate_expr(arg, _symbol_table)?;
                }
                Ok(())
            }
            // Other expression types not yet supported
            _ => Err(format!(
                "Expression type not yet supported in JIT: {:?}",
                expr
            )),
        }
    }

    /// Setup parameter bindings in scope for function invocation
    pub fn bind_parameters(
        scope_manager: &mut ScopeManager,
        params: &[SymbolId],
    ) -> Result<Vec<(usize, usize)>, String> {
        // Create a new scope for the function
        scope_manager.push_scope();

        // Bind each parameter in the new scope
        let mut bindings = Vec::new();
        for param in params {
            let (depth, index) = scope_manager.bind(*param);
            bindings.push((depth, index));
        }

        Ok(bindings)
    }

    /// Cleanup parameter bindings (pop function scope)
    pub fn unbind_parameters(scope_manager: &mut ScopeManager) -> Result<(), String> {
        scope_manager.pop_scope()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compiled_lambda_creation() {
        let params = vec![SymbolId(1), SymbolId(2)];
        let body = Box::new(Expr::Literal(crate::value::Value::Int(42)));
        let captures = vec![];

        let lambda = CompiledLambda::from_expr(params.clone(), body, captures);

        assert_eq!(lambda.param_count(), 2);
        assert_eq!(lambda.capture_count(), 0);
        assert!(lambda.matches_arity(2));
        assert!(!lambda.matches_arity(1));
        assert!(!lambda.matches_arity(3));
    }

    #[test]
    fn test_compiled_lambda_zero_args() {
        let params = vec![];
        let body = Box::new(Expr::Literal(crate::value::Value::Int(42)));
        let captures = vec![];

        let lambda = CompiledLambda::from_expr(params, body, captures);

        assert_eq!(lambda.param_count(), 0);
        assert!(lambda.matches_arity(0));
        assert!(!lambda.matches_arity(1));
    }

    #[test]
    fn test_compiled_lambda_with_captures() {
        let params = vec![SymbolId(1)];
        let body = Box::new(Expr::Literal(crate::value::Value::Int(42)));
        let captures = vec![(SymbolId(10), 0, 0), (SymbolId(11), 0, 1)];

        let lambda = CompiledLambda::from_expr(params, body, captures);

        assert_eq!(lambda.param_count(), 1);
        assert_eq!(lambda.capture_count(), 2);
    }

    #[test]
    fn test_function_compiler_simple_lambda() {
        let symbol_table = SymbolTable::new();
        let params = vec![SymbolId(1), SymbolId(2)];
        let body = Box::new(Expr::Literal(crate::value::Value::Int(42)));
        let captures = vec![];

        let result = FunctionCompiler::compile_lambda(params, body, captures, &symbol_table);

        assert!(result.is_ok());
        let lambda = result.unwrap();
        assert_eq!(lambda.param_count(), 2);
    }

    #[test]
    fn test_function_compiler_with_body_validation() {
        let symbol_table = SymbolTable::new();
        let params = vec![SymbolId(1)];
        let body = Box::new(Expr::Begin(vec![
            Expr::Literal(crate::value::Value::Int(1)),
            Expr::Literal(crate::value::Value::Int(2)),
        ]));
        let captures = vec![];

        let result = FunctionCompiler::compile_lambda(params, body, captures, &symbol_table);

        assert!(result.is_ok());
    }

    #[test]
    fn test_parameter_binding() {
        let mut scope_manager = ScopeManager::new();
        let params = vec![SymbolId(1), SymbolId(2), SymbolId(3)];

        let result = FunctionCompiler::bind_parameters(&mut scope_manager, &params);

        assert!(result.is_ok());
        let bindings = result.unwrap();
        assert_eq!(bindings.len(), 3);

        // Verify each binding is at the same depth but different indices
        for (i, (depth, index)) in bindings.iter().enumerate() {
            assert_eq!(*depth, 1); // Function scope is at depth 1
            assert_eq!(*index, i);
        }

        // Verify we can unbind
        let unbind_result = FunctionCompiler::unbind_parameters(&mut scope_manager);
        assert!(unbind_result.is_ok());
        assert_eq!(scope_manager.current_depth(), 0);
    }

    #[test]
    fn test_parameter_binding_zero_args() {
        let mut scope_manager = ScopeManager::new();
        let params = vec![];

        let result = FunctionCompiler::bind_parameters(&mut scope_manager, &params);

        assert!(result.is_ok());
        let bindings = result.unwrap();
        assert_eq!(bindings.len(), 0);
        assert_eq!(scope_manager.current_depth(), 1);

        let unbind_result = FunctionCompiler::unbind_parameters(&mut scope_manager);
        assert!(unbind_result.is_ok());
    }
}
