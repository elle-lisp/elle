// Function call compilation for Cranelift
//
// Handles compilation of function calls, including:
// - Primitive operations (+, -, *, /, <, >, =, etc.) via constant folding
// - User-defined functions (deferred to Phase 4+)
// - Higher-order functions and closures (Phase 5+)

use super::binop::BinOpCompiler;
use crate::compiler::ast::Expr;
use crate::symbol::SymbolTable;
use crate::value::Value;

/// Represents the result of trying to compile a call
#[derive(Debug, Clone)]
pub enum CallCompileResult {
    /// Successfully compiled to a constant value (constant folding)
    CompiledConstant(Value),
    /// Could not compile (requires runtime evaluation)
    NotCompilable,
}

/// Compiles function calls, with focus on constant folding for primitives.
///
/// Phase 3 milestone: With symbol table integration, we can now perform
/// constant folding on all primitive operations where arguments are literals.
pub struct FunctionCallCompiler;

impl FunctionCallCompiler {
    /// Try to compile a function call at compile-time (constant folding)
    ///
    /// If the function is a primitive operation and all arguments are constants,
    /// this will fold the computation to a single value at compile-time.
    pub fn try_compile_call(
        func: &Expr,
        args: &[Expr],
        symbol_table: &SymbolTable,
    ) -> CallCompileResult {
        // Check if this is a call to a primitive operation
        if let Expr::Literal(Value::Symbol(sym_id)) = func {
            // Get the symbol name from the symbol table
            if let Some(op_name) = symbol_table.name(*sym_id) {
                // Try to extract constant values from all arguments
                if let Ok(arg_values) = Self::extract_constant_args(args) {
                    // Try to compute the operation
                    if let Ok(result) = Self::compute_primitive_op(op_name, &arg_values) {
                        return CallCompileResult::CompiledConstant(result);
                    }
                }
            }
        }

        CallCompileResult::NotCompilable
    }

    /// Extract all arguments as constant values
    pub fn extract_constant_args(args: &[Expr]) -> Result<Vec<Value>, String> {
        let mut values = Vec::new();
        for arg in args {
            match arg {
                Expr::Literal(val) => values.push(val.clone()),
                _ => return Err("Non-literal argument".to_string()),
            }
        }
        Ok(values)
    }

    /// Compute a primitive operation on constant values
    fn compute_primitive_op(op: &str, args: &[Value]) -> Result<Value, String> {
        match op {
            "+" => Self::compute_add(args),
            "-" => Self::compute_sub(args),
            "*" => Self::compute_mul(args),
            "/" => Self::compute_div(args),
            "<" => Self::compute_lt(args),
            ">" => Self::compute_gt(args),
            "=" => Self::compute_eq(args),
            "<=" => Self::compute_lte(args),
            ">=" => Self::compute_gte(args),
            "!=" => Self::compute_neq(args),
            _ => Err(format!("Unknown primitive: {}", op)),
        }
    }

    fn compute_add(args: &[Value]) -> Result<Value, String> {
        if args.is_empty() {
            return Ok(Value::Int(0));
        }
        let mut result = args[0].clone();
        for arg in &args[1..] {
            result = BinOpCompiler::add(&result, arg)?;
        }
        Ok(result)
    }

    fn compute_sub(args: &[Value]) -> Result<Value, String> {
        if args.is_empty() {
            return Err("- requires at least 1 argument".to_string());
        }
        if args.len() == 1 {
            match &args[0] {
                Value::Int(i) => return Ok(Value::Int(-i)),
                Value::Float(f) => return Ok(Value::Float(-f)),
                _ => return Err("Cannot negate non-numeric value".to_string()),
            }
        }
        let mut result = args[0].clone();
        for arg in &args[1..] {
            result = BinOpCompiler::sub(&result, arg)?;
        }
        Ok(result)
    }

    fn compute_mul(args: &[Value]) -> Result<Value, String> {
        if args.is_empty() {
            return Ok(Value::Int(1));
        }
        let mut result = args[0].clone();
        for arg in &args[1..] {
            result = BinOpCompiler::mul(&result, arg)?;
        }
        Ok(result)
    }

    fn compute_div(args: &[Value]) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("/ requires at least 2 arguments".to_string());
        }
        let mut result = args[0].clone();
        for arg in &args[1..] {
            result = BinOpCompiler::div(&result, arg)?;
        }
        Ok(result)
    }

    fn compute_lt(args: &[Value]) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("< requires exactly 2 arguments".to_string());
        }
        BinOpCompiler::lt(&args[0], &args[1])
    }

    fn compute_gt(args: &[Value]) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("> requires exactly 2 arguments".to_string());
        }
        BinOpCompiler::gt(&args[0], &args[1])
    }

    fn compute_eq(args: &[Value]) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("= requires exactly 2 arguments".to_string());
        }
        BinOpCompiler::eq(&args[0], &args[1])
    }

    fn compute_lte(args: &[Value]) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("<= requires exactly 2 arguments".to_string());
        }
        BinOpCompiler::lte(&args[0], &args[1])
    }

    fn compute_gte(args: &[Value]) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(">= requires exactly 2 arguments".to_string());
        }
        BinOpCompiler::gte(&args[0], &args[1])
    }

    fn compute_neq(args: &[Value]) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("!= requires exactly 2 arguments".to_string());
        }
        BinOpCompiler::neq(&args[0], &args[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::SymbolId;

    #[test]
    fn test_constant_fold_add() {
        let mut symbol_table = SymbolTable::new();
        let add_sym = symbol_table.intern("+");

        let result = FunctionCallCompiler::try_compile_call(
            &Expr::Literal(Value::Symbol(add_sym)),
            &[Expr::Literal(Value::Int(1)), Expr::Literal(Value::Int(2))],
            &symbol_table,
        );
        match result {
            CallCompileResult::CompiledConstant(Value::Int(3)) => (),
            _ => panic!("Expected CompiledConstant(3), got {:?}", result),
        }
    }

    #[test]
    fn test_constant_fold_mul() {
        let mut symbol_table = SymbolTable::new();
        let mul_sym = symbol_table.intern("*");

        let result = FunctionCallCompiler::try_compile_call(
            &Expr::Literal(Value::Symbol(mul_sym)),
            &[Expr::Literal(Value::Int(3)), Expr::Literal(Value::Int(4))],
            &symbol_table,
        );
        match result {
            CallCompileResult::CompiledConstant(Value::Int(12)) => (),
            _ => panic!("Expected CompiledConstant(12), got {:?}", result),
        }
    }

    #[test]
    fn test_constant_fold_comparison() {
        let mut symbol_table = SymbolTable::new();
        let lt_sym = symbol_table.intern("<");

        let result = FunctionCallCompiler::try_compile_call(
            &Expr::Literal(Value::Symbol(lt_sym)),
            &[Expr::Literal(Value::Int(1)), Expr::Literal(Value::Int(5))],
            &symbol_table,
        );
        match result {
            CallCompileResult::CompiledConstant(Value::Bool(true)) => (),
            _ => panic!("Expected CompiledConstant(true), got {:?}", result),
        }
    }

    #[test]
    fn test_extract_constant_args() {
        let args = vec![Expr::Literal(Value::Int(1)), Expr::Literal(Value::Int(2))];
        let result = FunctionCallCompiler::extract_constant_args(&args);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 2);
    }

    #[test]
    fn test_extract_args_with_non_literal() {
        let args = vec![Expr::Literal(Value::Int(1)), Expr::Var(SymbolId(0), 0, 0)];
        let result = FunctionCallCompiler::extract_constant_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_not_compilable_with_variable() {
        let mut symbol_table = SymbolTable::new();
        let add_sym = symbol_table.intern("+");

        let result = FunctionCallCompiler::try_compile_call(
            &Expr::Literal(Value::Symbol(add_sym)),
            &[Expr::Literal(Value::Int(1)), Expr::Var(SymbolId(0), 0, 0)],
            &symbol_table,
        );
        match result {
            CallCompileResult::NotCompilable => (),
            _ => panic!("Expected NotCompilable, got {:?}", result),
        }
    }
}
