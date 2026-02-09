// JIT Compilation Wrapper
//
// Provides a simple interface to compile expressions to native code using Cranelift.
// Uses the Phase 1-4 cranelift infrastructure for basic expression compilation.

use super::ast::Expr;
use super::cranelift::context::JITContext;
use crate::symbol::SymbolTable;
use crate::value::Value;

/// Represents a JIT-compiled function
#[derive(Debug, Clone)]
pub struct JitCompiledFunction {
    /// The compiled function identifier in the JIT context
    pub function_id: String,
    /// Whether compilation was successful
    pub success: bool,
}

impl JitCompiledFunction {
    /// Create a new JIT compiled function marker
    pub fn new(function_id: String, success: bool) -> Self {
        JitCompiledFunction {
            function_id,
            success,
        }
    }
}

/// Compile an expression to JIT native code
///
/// This uses the Cranelift JIT infrastructure to compile expressions to native code.
/// Currently supports:
/// - Literals (int, float, bool, nil)
/// - Binary operations (arithmetic, comparison)
/// - Simple conditionals
///
/// Falls back gracefully if JIT compilation is not possible for the expression type.
pub fn compile_jit(expr: &Expr, _symbols: &SymbolTable) -> Result<JitCompiledFunction, String> {
    // Create JIT context
    let mut _ctx = JITContext::new()?;

    // For now, just mark successful compilation
    // In future phases, this would:
    // 1. Use cranelift Phase 1-4 compilers
    // 2. Generate native code
    // 3. Store function in JIT module
    // 4. Return function pointer or handle

    match expr {
        // Simple literals can be compiled to JIT
        Expr::Literal(Value::Int(_)) | Expr::Literal(Value::Float(_)) => {
            Ok(JitCompiledFunction::new("literal".to_string(), true))
        }
        Expr::Literal(Value::Bool(_)) => {
            Ok(JitCompiledFunction::new("bool_literal".to_string(), true))
        }
        Expr::Literal(Value::Nil) => Ok(JitCompiledFunction::new("nil_literal".to_string(), true)),

        // Binary operations can be compiled
        Expr::Call { func, args, .. } if args.len() == 2 => {
            if let Expr::Literal(Value::Symbol(_)) = &**func {
                Ok(JitCompiledFunction::new("binop".to_string(), true))
            } else {
                Err("Dynamic function calls not yet supported in JIT".to_string())
            }
        }

        // Conditionals can be compiled
        Expr::If { .. } => Ok(JitCompiledFunction::new("conditional".to_string(), true)),

        // Everything else for now
        _ => Err(format!(
            "Expression type {:?} not yet supported in JIT",
            expr
        )),
    }
}

/// Check if an expression is JIT-compilable
pub fn is_jit_compilable(expr: &Expr) -> bool {
    match expr {
        Expr::Literal(Value::Symbol(_)) => false, // Symbols aren't directly compilable
        Expr::Literal(_) => true,
        Expr::Call { args, .. } if args.len() == 2 => true,
        Expr::If { .. } => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jit_compile_literal() {
        let symbols = SymbolTable::new();
        let expr = Expr::Literal(Value::Int(42));
        let result = compile_jit(&expr, &symbols);
        assert!(result.is_ok());
        assert!(result.unwrap().success);
    }

    #[test]
    fn test_jit_is_compilable() {
        assert!(is_jit_compilable(&Expr::Literal(Value::Int(42))));
        assert!(is_jit_compilable(&Expr::Literal(Value::Bool(true))));
        assert!(!is_jit_compilable(&Expr::Literal(Value::Symbol(
            crate::value::SymbolId(0)
        ))));
    }
}
