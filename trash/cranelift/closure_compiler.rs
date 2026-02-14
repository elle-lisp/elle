// Closure compilation and captured variable handling (Phase 7)
//
// Manages compilation of lambdas with captured variables.
// This module provides:
// - Captured variable tracking and binding
// - Environment packing/unpacking
// - Closure value creation
// - Nested closure support

use super::function_compiler::CompiledLambda;
use super::scoping::ScopeManager;
use crate::compiler::ast::Expr;
use crate::symbol::SymbolTable;
use crate::value::{SymbolId, Value};
use std::collections::HashMap;

/// Represents a captured variable with its location
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CapturedVar {
    /// Symbol ID of the captured variable
    pub sym_id: SymbolId,
    /// Depth in the scope where variable is bound
    pub depth: usize,
    /// Index within that scope level
    pub index: usize,
}

/// Represents an environment of captured variables
#[derive(Debug, Clone)]
pub struct Environment {
    /// Captured variables in order
    pub captures: Vec<CapturedVar>,
    /// Values of captured variables (for runtime)
    pub values: Vec<Value>,
}

impl Environment {
    /// Create a new empty environment
    pub fn new() -> Self {
        Environment {
            captures: Vec::new(),
            values: Vec::new(),
        }
    }

    /// Create an environment from captured variable list
    pub fn from_captures(captures: Vec<CapturedVar>) -> Self {
        Environment {
            captures,
            values: Vec::new(),
        }
    }

    /// Add a captured variable to the environment
    pub fn add_capture(&mut self, var: CapturedVar) {
        self.captures.push(var);
    }

    /// Get the number of captured variables
    pub fn capture_count(&self) -> usize {
        self.captures.len()
    }

    /// Pack values into environment for closure
    pub fn pack_values(&mut self, values: Vec<Value>) -> Result<(), String> {
        if values.len() != self.captures.len() {
            return Err(format!(
                "Expected {} captured values, got {}",
                self.captures.len(),
                values.len()
            ));
        }
        self.values = values;
        Ok(())
    }

    /// Get captured values
    pub fn values(&self) -> &[Value] {
        &self.values
    }

    /// Get captured variables
    pub fn captures(&self) -> &[CapturedVar] {
        &self.captures
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self::new()
    }
}

/// Represents a compiled closure with captured variables
#[derive(Debug, Clone)]
pub struct CompiledClosure {
    /// The underlying lambda
    pub lambda: CompiledLambda,
    /// Captured variables and their values
    pub environment: Environment,
}

impl CompiledClosure {
    /// Create a new compiled closure
    pub fn new(lambda: CompiledLambda, environment: Environment) -> Self {
        CompiledClosure {
            lambda,
            environment,
        }
    }

    /// Get the total number of parameters (params + captures)
    pub fn total_vars(&self) -> usize {
        self.lambda.param_count() + self.environment.capture_count()
    }

    /// Check if this closure has any captured variables
    pub fn has_captures(&self) -> bool {
        self.environment.capture_count() > 0
    }
}

/// Compiler for closures with captured variables (Phase 7)
pub struct ClosureCompiler;

impl ClosureCompiler {
    /// Analyze an expression to find captured variables
    /// Returns a list of (SymbolId, depth, index) tuples for captured vars
    pub fn analyze_captures(
        expr: &Expr,
        _scope_manager: &ScopeManager,
        bound_params: &[SymbolId],
    ) -> Result<Vec<(SymbolId, usize, usize)>, String> {
        let mut captures = Vec::new();
        Self::collect_captures(expr, bound_params, &mut captures)?;

        // Remove duplicates while preserving order
        captures.sort_by_key(|&(sym, d, i)| (sym, d, i));
        captures.dedup();

        Ok(captures)
    }

    /// Recursively collect captured variables from an expression
    fn collect_captures(
        expr: &Expr,
        bound_params: &[SymbolId],
        captures: &mut Vec<(SymbolId, usize, usize)>,
    ) -> Result<(), String> {
        match expr {
            Expr::Var(sym_id, depth, index) => {
                // Check if this is a parameter (don't capture parameters)
                if !bound_params.contains(sym_id) {
                    // This is a free variable - capture it
                    captures.push((*sym_id, *depth, *index));
                }
                Ok(())
            }
            Expr::Literal(_) => Ok(()),
            Expr::Begin(exprs) | Expr::Block(exprs) => {
                for e in exprs {
                    Self::collect_captures(e, bound_params, captures)?;
                }
                Ok(())
            }
            Expr::If { cond, then, else_ } => {
                Self::collect_captures(cond, bound_params, captures)?;
                Self::collect_captures(then, bound_params, captures)?;
                Self::collect_captures(else_, bound_params, captures)?;
                Ok(())
            }
            Expr::Let { bindings, body } => {
                for (_sym, binding_expr) in bindings {
                    Self::collect_captures(binding_expr, bound_params, captures)?;
                }
                Self::collect_captures(body, bound_params, captures)?;
                Ok(())
            }
            Expr::Lambda { params, body, .. } => {
                // For nested lambdas, only analyze body with extended param list
                let mut extended_params = bound_params.to_vec();
                extended_params.extend_from_slice(params);
                Self::collect_captures(body, &extended_params, captures)?;
                Ok(())
            }
            Expr::Call { func, args, .. } => {
                Self::collect_captures(func, bound_params, captures)?;
                for arg in args {
                    Self::collect_captures(arg, bound_params, captures)?;
                }
                Ok(())
            }
            _ => {
                // Other expression types - for now just skip
                Ok(())
            }
        }
    }

    /// Compile a lambda with its captured variables
    pub fn compile_with_captures(
        params: Vec<SymbolId>,
        body: Box<Expr>,
        scope_manager: &ScopeManager,
        _symbol_table: &SymbolTable,
    ) -> Result<CompiledClosure, String> {
        // Analyze captures
        let captures = Self::analyze_captures(&body, scope_manager, &params)?;

        // Create the lambda
        let lambda = CompiledLambda::from_expr(params.clone(), body, captures.clone());

        // Create the environment
        let env = Environment::from_captures(
            captures
                .into_iter()
                .map(|(sym_id, depth, index)| CapturedVar {
                    sym_id,
                    depth,
                    index,
                })
                .collect(),
        );

        Ok(CompiledClosure::new(lambda, env))
    }

    /// Bind captured variables in function scope
    pub fn bind_captures(
        scope_manager: &mut ScopeManager,
        captures: &[CapturedVar],
    ) -> Result<Vec<(usize, usize)>, String> {
        let mut bindings = Vec::new();
        for cap in captures {
            let (depth, index) = scope_manager.bind(cap.sym_id);
            bindings.push((depth, index));
        }
        Ok(bindings)
    }

    /// Create an environment from current variable values
    pub fn pack_environment(
        _scope_manager: &ScopeManager,
        captures: &[CapturedVar],
        _var_values: &HashMap<(usize, usize), crate::compiler::cranelift::compiler_v2::IrValue>,
    ) -> Result<Environment, String> {
        let mut env = Environment::from_captures(captures.to_vec());

        // For compile-time, we don't actually pack values yet
        // This would happen at runtime with actual Values
        let values = vec![Value::Nil; captures.len()];
        env.pack_values(values)?;

        Ok(env)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_environment_creation() {
        let env = Environment::new();
        assert_eq!(env.capture_count(), 0);
        assert!(env.values().is_empty());
    }

    #[test]
    fn test_environment_add_capture() {
        let mut env = Environment::new();
        let cap = CapturedVar {
            sym_id: SymbolId(1),
            depth: 0,
            index: 0,
        };

        env.add_capture(cap);
        assert_eq!(env.capture_count(), 1);
    }

    #[test]
    fn test_environment_pack_values() {
        let mut env = Environment::new();
        let cap = CapturedVar {
            sym_id: SymbolId(1),
            depth: 0,
            index: 0,
        };
        env.add_capture(cap);

        let values = vec![Value::Int(42)];
        let result = env.pack_values(values);

        assert!(result.is_ok());
        assert_eq!(env.values().len(), 1);
    }

    #[test]
    fn test_environment_pack_values_mismatch() {
        let mut env = Environment::new();
        let cap1 = CapturedVar {
            sym_id: SymbolId(1),
            depth: 0,
            index: 0,
        };
        let cap2 = CapturedVar {
            sym_id: SymbolId(2),
            depth: 0,
            index: 1,
        };
        env.add_capture(cap1);
        env.add_capture(cap2);

        // Try to pack wrong number of values
        let values = vec![Value::Int(42)];
        let result = env.pack_values(values);

        assert!(result.is_err());
    }

    #[test]
    fn test_compiled_closure_creation() {
        let lambda = CompiledLambda::from_expr(
            vec![SymbolId(1)],
            Box::new(Expr::Literal(Value::Int(42))),
            vec![],
        );
        let env = Environment::new();

        let closure = CompiledClosure::new(lambda, env);

        assert_eq!(closure.total_vars(), 1);
        assert!(!closure.has_captures());
    }

    #[test]
    fn test_compiled_closure_with_captures() {
        let lambda = CompiledLambda::from_expr(
            vec![SymbolId(1)],
            Box::new(Expr::Literal(Value::Int(42))),
            vec![(SymbolId(2), 0, 0), (SymbolId(3), 0, 1)],
        );

        let mut env = Environment::new();
        env.add_capture(CapturedVar {
            sym_id: SymbolId(2),
            depth: 0,
            index: 0,
        });
        env.add_capture(CapturedVar {
            sym_id: SymbolId(3),
            depth: 0,
            index: 1,
        });

        let closure = CompiledClosure::new(lambda, env);

        assert_eq!(closure.total_vars(), 3); // 1 param + 2 captures
        assert!(closure.has_captures());
    }

    #[test]
    fn test_captured_var_equality() {
        let cap1 = CapturedVar {
            sym_id: SymbolId(1),
            depth: 0,
            index: 0,
        };
        let cap2 = CapturedVar {
            sym_id: SymbolId(1),
            depth: 0,
            index: 0,
        };

        assert_eq!(cap1, cap2);
    }

    #[test]
    fn test_closure_compiler_with_captures() {
        let symbol_table = SymbolTable::new();
        let mut scope_manager = ScopeManager::new();

        // Setup: have some variables in scope
        scope_manager.bind(SymbolId(1));
        scope_manager.bind(SymbolId(2));

        let params = vec![SymbolId(3)];
        let body = Box::new(Expr::Var(SymbolId(1), 0, 0)); // References outer var

        let result =
            ClosureCompiler::compile_with_captures(params, body, &scope_manager, &symbol_table);

        assert!(result.is_ok());
        let closure = result.unwrap();
        assert_eq!(closure.lambda.param_count(), 1);
        assert!(closure.has_captures());
    }
}
